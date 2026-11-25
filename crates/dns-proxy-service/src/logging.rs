//! File logging + panic hook for the Windows service (LocalSystem has no console).

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

use tracing_subscriber::EnvFilter;

static LOG_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);

pub fn service_log_path() -> Option<PathBuf> {
    dns_proxy_core::default_config_dir()
        .ok()
        .map(|d| d.join("service.log"))
}

/// Append-only log under `%ProgramData%\DNS Proxy\service.log`.
pub fn init_service_file_logging() {
    let path = match service_log_path() {
        Some(p) => p,
        None => return,
    };
    let _ = dns_proxy_core::ensure_config_dir();
    if let Ok(mut guard) = LOG_PATH.lock() {
        *guard = Some(path.clone());
    }

    let Ok(file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    else {
        return;
    };

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_ansi(false)
        .with_writer(Mutex::new(file))
        .finish();
    // Console `main` may have already set a subscriber — ignore that error.
    let _ = tracing::subscriber::set_global_default(subscriber);

    tracing::info!("service logging to {}", path.display());
}

pub fn install_panic_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let msg = format!("PANIC: {info}");
        let _ = append_crash_line(&msg);
        if let Ok(dir) = dns_proxy_core::default_config_dir() {
            let _ = std::fs::write(dir.join("service-panic.txt"), format!("{msg}\n"));
        }
        default(info);
    }));
}

pub fn append_crash_line(line: &str) -> std::io::Result<()> {
    let path = LOG_PATH
        .lock()
        .ok()
        .and_then(|g| g.clone())
        .or_else(service_log_path);
    let Some(path) = path else {
        return Ok(());
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut f = OpenOptions::new().create(true).append(true).open(path)?;
    let ts = match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => format!("unix={}", d.as_secs()),
        Err(_) => "unix=?".into(),
    };
    writeln!(f, "{ts} {line}")?;
    Ok(())
}

/// Best-effort: if adapters still point at 127.0.0.1, reset them to DHCP.
/// Called when the service is about to exit unexpectedly.
pub fn emergency_restore_loopback_dns() {
    let script = r#"
$ErrorActionPreference = 'SilentlyContinue'
$hits = Get-DnsClientServerAddress -AddressFamily IPv4 |
  Where-Object { $_.ServerAddresses -contains '127.0.0.1' }
foreach ($h in $hits) {
  Set-DnsClientServerAddress -InterfaceAlias $h.InterfaceAlias -ResetServerAddresses
}
$backup = Join-Path $env:ProgramData 'DNS Proxy\dns-backup.json'
if (Test-Path $backup) { Remove-Item $backup -Force }
ipconfig /flushdns | Out-Null
"#;
    let _ = std::process::Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ])
        .status();
    let _ = append_crash_line(
        "emergency_restore_loopback_dns: reset adapters with 127.0.0.1 to DHCP",
    );
}
