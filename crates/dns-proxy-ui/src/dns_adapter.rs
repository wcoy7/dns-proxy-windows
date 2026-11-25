//! Detect / set / restore per-adapter IPv4 DNS on Windows.
//!
//! Uses IP Helper (`GetAdaptersAddresses`, `GetInterfaceDnsSettings`,
//! `SetInterfaceDnsSettings`). Changing DNS typically requires elevation;
//! callers should check [`is_elevated`] and surface a clear error.

use std::fs;
use std::net::Ipv4Addr;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use windows::core::{GUID, PWSTR};
use windows::Win32::Foundation::{ERROR_BUFFER_OVERFLOW, ERROR_SUCCESS, WIN32_ERROR};
use windows::Win32::NetworkManagement::IpHelper::{
    FreeMibTable, GetAdaptersAddresses, GetIfEntry2, GetInterfaceDnsSettings,
    SetInterfaceDnsSettings, DNS_INTERFACE_SETTINGS, DNS_INTERFACE_SETTINGS_VERSION1,
    DNS_SETTING_NAMESERVER, GAA_FLAG_INCLUDE_PREFIX, IF_TYPE_SOFTWARE_LOOPBACK,
    IP_ADAPTER_ADDRESSES_LH, MIB_IF_ROW2,
};
use windows::Win32::Networking::WinSock::{AF_INET, SOCKADDR, SOCKADDR_IN};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::Win32::UI::Shell::IsUserAnAdmin;

/// IPv4 address we point adapters at when enabling the local proxy.
pub const PROXY_DNS: &str = "127.0.0.1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdapterDnsInfo {
    pub guid: String,
    pub friendly_name: String,
    pub if_index: u32,
    /// True when NameServer is empty (Windows uses DHCP / RA-provided DNS).
    pub dhcp_dns: bool,
    /// Configured static IPv4 DNS servers (may be empty when `dhcp_dns`).
    pub servers: Vec<String>,
    pub operational: bool,
}

impl AdapterDnsInfo {
    pub fn points_at_proxy(&self) -> bool {
        self.servers.iter().any(|s| s == PROXY_DNS)
    }

    pub fn summary(&self) -> String {
        let dns = if self.points_at_proxy() {
            format!("{PROXY_DNS} (proxy)")
        } else if self.dhcp_dns && self.servers.is_empty() {
            "DHCP / automatic".into()
        } else if self.servers.is_empty() {
            "(none)".into()
        } else {
            self.servers.join(", ")
        };
        let up = if self.operational { "up" } else { "down" };
        format!("{} [{up}] — DNS: {dns}", self.friendly_name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct DnsBackup {
    pub adapters: Vec<AdapterDnsSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdapterDnsSnapshot {
    pub guid: String,
    pub friendly_name: String,
    pub dhcp_dns: bool,
    pub servers: Vec<String>,
}

fn backup_path() -> Result<PathBuf> {
    Ok(dns_proxy_core::default_config_dir()?.join("dns-backup.json"))
}

impl DnsBackup {
    pub fn load() -> Result<Option<Self>> {
        let path = backup_path()?;
        if !path.exists() {
            return Ok(None);
        }
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let b: Self = serde_json::from_str(&raw)
            .with_context(|| format!("failed to parse {}", path.display()))?;
        Ok(Some(b))
    }

    pub fn save(&self) -> Result<()> {
        dns_proxy_core::ensure_config_dir()?;
        let path = backup_path()?;
        let raw = serde_json::to_string_pretty(self)?;
        fs::write(&path, raw + "\n")
            .with_context(|| format!("failed to write {}", path.display()))?;
        Ok(())
    }

    pub fn clear_file() -> Result<()> {
        let path = backup_path()?;
        if path.exists() {
            fs::remove_file(&path)
                .with_context(|| format!("failed to remove {}", path.display()))?;
        }
        Ok(())
    }

    pub fn is_active() -> bool {
        backup_path().ok().is_some_and(|p| p.exists())
    }
}

pub fn is_elevated() -> bool {
    unsafe { IsUserAnAdmin().as_bool() }
}

/// List non-loopback IPv4-capable adapters and their configured DNS.
pub fn list_adapters() -> Result<Vec<AdapterDnsInfo>> {
    let addresses = adapter_addresses()?;
    let mut out = Vec::new();

    for addr in &addresses {
        if addr.if_type == IF_TYPE_SOFTWARE_LOOPBACK {
            continue;
        }
        let has_ipv4 = addr.unicast.iter().any(|u| u.parse::<Ipv4Addr>().is_ok());
        if !has_ipv4 && !addr.operational {
            continue;
        }

        let (dhcp_dns, servers) = read_interface_dns(&addr.guid)?;
        out.push(AdapterDnsInfo {
            guid: addr.guid.clone(),
            friendly_name: addr.friendly_name.clone(),
            if_index: addr.if_index,
            dhcp_dns,
            servers,
            operational: addr.operational,
        });
    }

    out.sort_by(|a, b| {
        b.operational
            .cmp(&a.operational)
            .then(a.friendly_name.cmp(&b.friendly_name))
    });
    Ok(out)
}

/// Active (operational) adapters — the ones we switch by default.
pub fn active_adapters() -> Result<Vec<AdapterDnsInfo>> {
    Ok(list_adapters()?
        .into_iter()
        .filter(|a| a.operational)
        .collect())
}

/// Snapshot current DNS on active adapters, point them at `127.0.0.1`, persist backup.
pub fn enable_proxy_dns() -> Result<String> {
    if !is_elevated() {
        bail!(
            "Administrator elevation is required to change adapter DNS. \
             Right-click dns-proxy-ui.exe → Run as administrator."
        );
    }

    let adapters = active_adapters()?;
    if adapters.is_empty() {
        bail!("no operational network adapters found");
    }

    // Keep the original restore point if a backup already exists this session.
    let mut backup = DnsBackup::load()?.unwrap_or_default();
    if backup.adapters.is_empty() {
        for a in &adapters {
            backup.adapters.push(AdapterDnsSnapshot {
                guid: a.guid.clone(),
                friendly_name: a.friendly_name.clone(),
                dhcp_dns: a.dhcp_dns,
                servers: a.servers.clone(),
            });
        }
        backup.save()?;
    }

    let mut changed = Vec::new();
    let mut errors = Vec::new();
    for a in &adapters {
        if a.points_at_proxy() {
            changed.push(format!("{} (already proxy)", a.friendly_name));
            continue;
        }
        match set_static_dns(&a.guid, &[PROXY_DNS.to_string()]) {
            Ok(()) => changed.push(a.friendly_name.clone()),
            Err(err) => errors.push(format!("{}: {err}", a.friendly_name)),
        }
    }

    let _ = flush_dns_cache();

    if !errors.is_empty() {
        if changed.is_empty() || changed.iter().all(|c| c.contains("already")) {
            let _ = restore_proxy_dns();
        }
        bail!(
            "failed to set DNS on some adapters: {}. Changed: {}",
            errors.join("; "),
            if changed.is_empty() {
                "(none)".into()
            } else {
                changed.join(", ")
            }
        );
    }

    Ok(format!(
        "Set preferred DNS to {PROXY_DNS} on: {}",
        changed.join(", ")
    ))
}

/// Restore DNS from the persisted backup (DHCP or previous static list).
pub fn restore_proxy_dns() -> Result<String> {
    if !is_elevated() {
        bail!(
            "Administrator elevation is required to restore adapter DNS. \
             Right-click dns-proxy-ui.exe → Run as administrator."
        );
    }

    let Some(backup) = DnsBackup::load()? else {
        bail!("no DNS backup found — nothing to restore");
    };

    let mut restored = Vec::new();
    let mut errors = Vec::new();
    for snap in &backup.adapters {
        let result = if snap.dhcp_dns || snap.servers.is_empty() {
            set_dhcp_dns(&snap.guid)
        } else {
            set_static_dns(&snap.guid, &snap.servers)
        };
        match result {
            Ok(()) => restored.push(snap.friendly_name.clone()),
            Err(err) => errors.push(format!("{}: {err}", snap.friendly_name)),
        }
    }

    let _ = flush_dns_cache();

    if !errors.is_empty() {
        bail!(
            "restore incomplete: {}. Restored: {}",
            errors.join("; "),
            if restored.is_empty() {
                "(none)".into()
            } else {
                restored.join(", ")
            }
        );
    }

    DnsBackup::clear_file()?;
    Ok(format!("Restored DNS on: {}", restored.join(", ")))
}

// --- Win32 helpers ----------------------------------------------------------

struct RawAdapter {
    guid: String,
    friendly_name: String,
    if_index: u32,
    if_type: u32,
    operational: bool,
    unicast: Vec<String>,
}

fn adapter_addresses() -> Result<Vec<RawAdapter>> {
    let family = u32::from(AF_INET.0);
    let flags = GAA_FLAG_INCLUDE_PREFIX;

    let mut size: u32 = 0;
    let first = unsafe { GetAdaptersAddresses(family, flags, None, None, &mut size) };
    if WIN32_ERROR(first) != ERROR_BUFFER_OVERFLOW {
        bail!("GetAdaptersAddresses size probe failed: {first}");
    }

    let mut buf = vec![0u8; size as usize];
    let head = buf.as_mut_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>();
    let status = unsafe { GetAdaptersAddresses(family, flags, None, Some(head), &mut size) };
    if status != 0 {
        bail!("GetAdaptersAddresses failed: {status}");
    }

    let mut out = Vec::new();
    let mut cur = head;
    while !cur.is_null() {
        let a = unsafe { &*cur };
        let guid_raw = unsafe { pstr_to_string(a.AdapterName) };
        let guid = normalize_guid(&guid_raw);
        let friendly_name = unsafe { pwstr_to_string(a.FriendlyName) };
        let if_index = unsafe { a.Anonymous1.Anonymous.IfIndex };
        let operational = is_operational(if_index).unwrap_or(a.OperStatus.0 == 1);

        let mut unicast = Vec::new();
        let mut u = a.FirstUnicastAddress;
        while !u.is_null() {
            let ua = unsafe { &*u };
            if let Some(ip) = sockaddr_ipv4(ua.Address.lpSockaddr) {
                unicast.push(ip.to_string());
            }
            u = ua.Next;
        }

        out.push(RawAdapter {
            guid,
            friendly_name,
            if_index,
            if_type: a.IfType,
            operational,
            unicast,
        });

        cur = a.Next;
    }

    Ok(out)
}

fn is_operational(if_index: u32) -> Result<bool> {
    let mut row = MIB_IF_ROW2 {
        InterfaceIndex: if_index,
        ..Default::default()
    };
    let status = unsafe { GetIfEntry2(&mut row) };
    if status != ERROR_SUCCESS {
        bail!("GetIfEntry2 failed: {status:?}");
    }
    Ok(row.OperStatus.0 == 1)
}

fn normalize_guid(s: &str) -> String {
    let t = s.trim();
    if t.starts_with('{') {
        t.to_ascii_uppercase()
    } else {
        format!("{{{}}}", t.to_ascii_uppercase())
    }
}

fn parse_guid(s: &str) -> Result<GUID> {
    let normalized = normalize_guid(s);
    let hex: String = normalized
        .chars()
        .filter(|c| c.is_ascii_hexdigit())
        .collect();
    if hex.len() != 32 {
        bail!("invalid GUID '{s}'");
    }
    let u = u128::from_str_radix(&hex, 16).with_context(|| format!("invalid GUID '{s}'"))?;
    Ok(GUID::from_u128(u))
}

fn read_interface_dns(guid_str: &str) -> Result<(bool, Vec<String>)> {
    let guid = parse_guid(guid_str)?;
    let mut settings = DNS_INTERFACE_SETTINGS {
        Version: DNS_INTERFACE_SETTINGS_VERSION1,
        Flags: u64::from(DNS_SETTING_NAMESERVER),
        ..Default::default()
    };
    let status = unsafe { GetInterfaceDnsSettings(guid, &mut settings) };
    if status != ERROR_SUCCESS {
        tracing::debug!("GetInterfaceDnsSettings({guid_str}) failed: {status:?}");
        return Ok((true, Vec::new()));
    }

    let servers_raw = unsafe { pwstr_to_string(settings.NameServer) };
    unsafe {
        free_dns_settings_strings(&mut settings);
    }

    let servers: Vec<String> = servers_raw
        .split(|c: char| c == ',' || c == ' ' || c == ';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();

    Ok((servers.is_empty(), servers))
}

unsafe fn free_dns_settings_strings(settings: &mut DNS_INTERFACE_SETTINGS) {
    // MSDN: free NameServer / SearchList / Domain via FreeMibTable.
    for ptr in [
        &mut settings.NameServer,
        &mut settings.SearchList,
        &mut settings.Domain,
    ] {
        if !ptr.is_null() {
            FreeMibTable(ptr.0 as *const std::ffi::c_void);
            *ptr = PWSTR::null();
        }
    }
}

fn set_static_dns(guid_str: &str, servers: &[String]) -> Result<()> {
    let guid = parse_guid(guid_str)?;
    let joined = servers.join(",");
    let mut wide: Vec<u16> = joined.encode_utf16().chain(std::iter::once(0)).collect();
    let settings = DNS_INTERFACE_SETTINGS {
        Version: DNS_INTERFACE_SETTINGS_VERSION1,
        Flags: u64::from(DNS_SETTING_NAMESERVER),
        NameServer: PWSTR(wide.as_mut_ptr()),
        ..Default::default()
    };
    let status = unsafe { SetInterfaceDnsSettings(guid, &settings) };
    if status != ERROR_SUCCESS {
        bail!("SetInterfaceDnsSettings({guid_str}) static failed: {status:?}");
    }
    Ok(())
}

fn set_dhcp_dns(guid_str: &str) -> Result<()> {
    let guid = parse_guid(guid_str)?;
    let mut wide: Vec<u16> = vec![0];
    let settings = DNS_INTERFACE_SETTINGS {
        Version: DNS_INTERFACE_SETTINGS_VERSION1,
        Flags: u64::from(DNS_SETTING_NAMESERVER),
        NameServer: PWSTR(wide.as_mut_ptr()),
        ..Default::default()
    };
    let status = unsafe { SetInterfaceDnsSettings(guid, &settings) };
    if status != ERROR_SUCCESS {
        bail!("SetInterfaceDnsSettings({guid_str}) DHCP failed: {status:?}");
    }
    Ok(())
}

fn flush_dns_cache() -> Result<()> {
    type DnsFlushFn = unsafe extern "system" fn() -> i32;
    let lib = unsafe { LoadLibraryW(windows::core::w!("dnsapi.dll")) }?;
    let proc = unsafe { GetProcAddress(lib, windows::core::s!("DnsFlushResolverCache")) };
    if let Some(proc) = proc {
        let f: DnsFlushFn = unsafe { std::mem::transmute(proc) };
        unsafe {
            f();
        }
    }
    Ok(())
}

unsafe fn pstr_to_string(p: windows::core::PSTR) -> String {
    if p.is_null() {
        return String::new();
    }
    let mut bytes = Vec::new();
    let mut i = 0isize;
    loop {
        let b = *p.0.offset(i);
        if b == 0 {
            break;
        }
        bytes.push(b);
        i += 1;
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

unsafe fn pwstr_to_string(p: PWSTR) -> String {
    if p.is_null() {
        return String::new();
    }
    p.to_string().unwrap_or_default()
}

fn sockaddr_ipv4(addr: *const SOCKADDR) -> Option<Ipv4Addr> {
    if addr.is_null() {
        return None;
    }
    unsafe {
        let sa = &*(addr as *const SOCKADDR_IN);
        if i32::from(sa.sin_family.0) != i32::from(AF_INET.0) {
            return None;
        }
        let octets = sa.sin_addr.S_un.S_addr.to_ne_bytes();
        Some(Ipv4Addr::new(octets[0], octets[1], octets[2], octets[3]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_guid_adds_braces() {
        assert_eq!(
            normalize_guid("aabbccdd-eeff-1122-3344-556677889900"),
            "{AABBCCDD-EEFF-1122-3344-556677889900}"
        );
    }

    #[test]
    fn parse_guid_accepts_braced() {
        assert!(parse_guid("{AABBCCDD-EEFF-1122-3344-556677889900}").is_ok());
    }
}
