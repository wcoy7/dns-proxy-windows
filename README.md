# DNS Proxy

**A Windows DNS proxy with selectable public resolvers and built-in domain blocking.**

Point your machine at `127.0.0.1`, pick Google / Cloudflare / OpenDNS (or your own upstreams), and block YouTube, Roblox, social media, or any custom list — while everything else resolves normally through your chosen upstream.

<p align="center">
  <img src="assets/dns-proxy-icon.png" alt="DNS Proxy icon" width="128" height="128" />
</p>

**Current version: 0.2.0**

## Features

- **Local DNS service** — listens on `127.0.0.1:53` as a proper Windows service
- **Upstream presets** — Google (`8.8.8.8` / `8.8.4.4`), Cloudflare (`1.1.1.1` / `1.0.0.1`), OpenDNS (`208.67.222.222` / `208.67.220.220`)
- **Custom upstreams** — add any IPv4/IPv6 resolvers you want
- **Category blocking** — YouTube, Roblox, social media toggles plus a free-form custom list
- **Block modes** — answer with **NXDOMAIN** or a **sinkhole** IP
- **System tray control panel** — lives in the notification area; close hides to tray, Quit exits
- **Auto DNS switch** — one click to point active adapters at `127.0.0.1`, with restore of the previous settings
- **Shared config** — JSON under `%ProgramData%\DNS Proxy\` (all Windows logins)
- **Windows installer** — Inno Setup packaging to `dist\DNSProxySetup-0.2.0.exe`

## Workspace layout

| Crate | Binary | Role |
|---|---|---|
| `dns-proxy-core` | — | Shared config, presets, block engine, ProgramData paths |
| `dns-proxy-service` | `dns-proxy-service.exe` | UDP DNS proxy + Windows service |
| `dns-proxy-ui` | `dns-proxy-ui.exe` | egui control panel + tray + DNS switch |

## Prerequisites

- Windows 10 / 11
- [Rust](https://rustup.rs/) (stable, edition 2021+)
- **Administrator** privileges to bind UDP port `53`, install/control the Windows service, and change adapter DNS

> Binding to port 53, registering a Windows service, and switching adapter DNS all require elevation (UAC).

## Build

```powershell
cd "C:\Users\Warren\Documents\Projects\DNS Proxy"
cargo build --release
```

Binaries land in `target\release\`:

- `dns-proxy-service.exe`
- `dns-proxy-ui.exe`

Icon assets live in [`assets/`](assets/) (`dns-proxy.ico`, `dns-proxy-icon.png`) and are embedded into the Windows binaries at build time.

## Windows installer (Inno Setup)

Packaging lives under [`installer/`](installer/):

| Path | Role |
|---|---|
| [`installer/dns-proxy.iss`](installer/dns-proxy.iss) | Inno Setup script (AppId, icons, files, shortcuts, ProgramData ACLs, optional service) |
| [`installer/post-install-notes.txt`](installer/post-install-notes.txt) | Shown in the Setup wizard before install |
| [`scripts/build-installer.ps1`](scripts/build-installer.ps1) | `cargo build --release` + compile with `ISCC.exe` |

The Setup EXE installs UI + service binaries to **Program Files\DNS Proxy**, creates `%ProgramData%\DNS Proxy` with **Users Modify** ACL, adds a Start Menu shortcut (optional desktop shortcut), requests **Administrator** elevation, and can optionally register/start the `DnsProxy` Windows service. Uninstall (Apps & features) stops and removes the service when present.

### Build-time prerequisites

- Release binaries from `cargo build --release` (the `.iss` script errors if they are missing)
- [Inno Setup 6](https://jrsoftware.org/isinfo.php) — needed only to **compile** the installer, not to run the installed app

```powershell
winget install --id JRSoftware.InnoSetup -e
# or: choco install innosetup -y
```

### Build the installer

```powershell
# One-shot: release build + compile Setup EXE
.\scripts\build-installer.ps1
```

Or manually:

```powershell
cargo build --release
# Typical ISCC paths (pick the one that exists on your machine):
& "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe" .\installer\dns-proxy.iss
# or: & "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe" .\installer\dns-proxy.iss
```

Output (gitignored under `dist/`):

```text
dist\DNSProxySetup-0.2.0.exe
```

### Install and run

1. Run `dist\DNSProxySetup-0.2.0.exe` (UAC elevation required).
2. On **Additional Tasks**, optionally enable:
   - desktop shortcut
   - **Install and start the DNS Proxy Windows service**
3. Finish the wizard; optionally launch the control panel from the final page (or Start Menu → **DNS Proxy**).
4. If you skipped the service task, from an elevated PowerShell in `C:\Program Files\DNS Proxy`:

   ```powershell
   .\dns-proxy-service.exe init-config
   .\dns-proxy-service.exe install
   .\dns-proxy-service.exe start
   ```

5. In the control panel, enable **Use DNS Proxy** (or set adapter DNS manually — see below).

## Quick start (console)

Useful for smoke-testing without installing a service:

```powershell
# Elevated PowerShell recommended
.\target\release\dns-proxy-service.exe init-config
.\target\release\dns-proxy-service.exe console
```

Then set your adapter DNS to `127.0.0.1` (UI checkbox or manually) and try:

```powershell
nslookup example.com 127.0.0.1
nslookup youtube.com 127.0.0.1
```

## Install as a Windows service

From an **elevated** PowerShell:

```powershell
cd "C:\Users\Warren\Documents\Projects\DNS Proxy\target\release"

# Register the service (LocalSystem, auto-start)
.\dns-proxy-service.exe install

# Start / stop / status
.\dns-proxy-service.exe start
.\dns-proxy-service.exe status
.\dns-proxy-service.exe stop

# Remove
.\dns-proxy-service.exe uninstall
```

Equivalent SCM commands:

```powershell
sc.exe start DnsProxy
sc.exe stop DnsProxy
sc.exe delete DnsProxy
```

Or use the control panel / tray menu (also needs elevation for install/uninstall / DNS switch).

### Helper scripts

```powershell
.\scripts\install-service.ps1   # build release (if needed) + install + start
.\scripts\uninstall-service.ps1
```

## Control panel & system tray

```powershell
# Prefer elevated so service + DNS switch work
.\target\release\dns-proxy-ui.exe
```

### Tray behavior

- The app icon lives in the **notification area** (system tray).
- **Left-click** the tray icon (or choose **Open / Show**) brings up the settings window.
- **Closing** the window **hides to tray** — it does not quit.
- Tray menu: **Open / Show**, **Start service**, **Stop service**, **Quit**.
- **Start minimized to tray** defaults on (toggle in *Tray & window*); preference is saved under ProgramData.
- **Quit** exits the process. With **Restore DNS on Quit** enabled (default), previous adapter DNS is restored when a backup exists.

### DNS switch / restore

In **Network DNS**:

1. Ensure the Windows service is **running**.
2. Check **Use DNS Proxy** — active adapters’ preferred IPv4 DNS become `127.0.0.1`.
3. Previous settings are written to `%ProgramData%\DNS Proxy\dns-backup.json`.
4. Uncheck the box, click **Restore previous DNS**, stop the service from the UI/tray, or **Quit** to put DNS back.

The UI shows each adapter’s current DNS, whether it points at the proxy, elevation status, and the last error.

**Elevation:** changing adapter DNS requires Administrator. If the UI is not elevated, the switch fails with a clear message — right-click `dns-proxy-ui.exe` → **Run as administrator**.

**Safety:** the UI refuses to switch DNS while the proxy service is not running (avoids breaking name resolution). Stop / uninstall paths restore DNS from the backup when present.

### Other UI actions

1. Choose a preset or custom upstream
2. Toggle YouTube / Roblox / social media blocks and edit the custom list
3. Switch between NXDOMAIN and sinkhole responses
4. **Save configuration** — writes shared ProgramData config and **restarts** a running service so changes apply without a full reinstall

## Configuration

Default path (shared, all logins): **`%ProgramData%\DNS Proxy\config.json`**

Typically `C:\ProgramData\DNS Proxy\config.json`.

The first elevated create of that folder grants **BUILTIN\Users** modify rights (`icacls … /grant *S-1-5-32-545:(OI)(CI)M`). The installer also creates the folder with Users-modify permissions. If a non-elevated process cannot write there, run the UI or `init-config` once as Administrator.

### Migration from 0.1.x

If `%ProgramData%\DNS Proxy\config.json` is missing but `%LOCALAPPDATA%\DNS Proxy\config.json` exists, the next `ensure_config_dir` / `init-config` / UI start **copies** it (and `dns-backup.json` / `ui-settings.json` when present) into ProgramData once. The legacy files are left in place.

```json
{
  "listen": "127.0.0.1:53",
  "upstream": {
    "type": "preset",
    "preset": "cloudflare"
  },
  "block": {
    "youtube": true,
    "roblox": true,
    "social": true,
    "custom": ["example-ads.test"],
    "mode": "nxdomain",
    "sinkhole_ip": "0.0.0.0"
  },
  "verbose_logging": false
}
```

Custom upstream example:

```json
"upstream": {
  "type": "custom",
  "servers": ["9.9.9.9", "149.112.112.112"]
}
```

Related files in the same folder:

| File | Purpose |
|---|---|
| `config.json` | Service + UI shared settings |
| `ui-settings.json` | Tray start-minimized / restore-on-quit |
| `dns-backup.json` | Previous adapter DNS (while switched) |

## Point Windows at the proxy

**Recommended:** elevated UI → start service → enable **Use DNS Proxy**.

Manual:

1. Install & start the service (or run `console`)
2. Open **Settings → Network & Internet → your adapter → DNS server assignment → Manual**
3. Set Preferred DNS to `127.0.0.1` (leave alternate empty or set a fallback you accept)
4. Flush the cache: `ipconfig /flushdns`

To undo, use **Restore previous DNS** / uncheck **Use DNS Proxy**, or set DNS back to Automatic (DHCP), then stop/uninstall the service.

## How to test (0.2.0)

### Tray

1. `cargo build --release` then run `.\target\release\dns-proxy-ui.exe` (elevated recommended).
2. Confirm the tray icon appears; close the window — app stays running, icon remains.
3. Left-click tray / **Open / Show** — window returns.
4. Tray **Start** / **Stop** service (elevated); **Quit** exits.

### DNS switch / restore

1. Install + start the service while elevated.
2. Note current adapter DNS (`ipconfig /all` or the Network DNS panel).
3. Enable **Use DNS Proxy** — adapters show `127.0.0.1 (proxy)`; `%ProgramData%\DNS Proxy\dns-backup.json` appears.
4. `nslookup example.com` should hit the local proxy.
5. Uncheck / **Restore previous DNS** / **Quit** — DNS returns to the backup; backup file removed on success.
6. Without elevation, the switch must fail with an Administrator message.
7. With the service stopped, enabling the checkbox must refuse and warn.

### ProgramData config

1. Save settings from the UI; confirm `C:\ProgramData\DNS Proxy\config.json` updates.
2. Optionally place an old config under `%LOCALAPPDATA%\DNS Proxy\`, delete the ProgramData copy, restart UI/`init-config`, and confirm migration.

## Security & admin notes

| Action | Elevation needed? |
|---|---|
| Build / run UI (view-only) | No |
| Bind `127.0.0.1:53` | Yes (or grant capability) |
| Install / uninstall service | Yes |
| Start / stop service | Yes (or appropriate rights) |
| Switch / restore adapter DNS | Yes |
| Create ProgramData folder + Users ACL | Yes (once) |
| Edit shared `config.json` | No once Users-modify ACL is set |

This project is a **local** filtering proxy. It does not encrypt DNS on the wire to upstreams (plain UDP/53). For DNS-over-HTTPS/TLS, terminate that on a separate forwarder and point DNS Proxy’s custom upstream at it.

## License

MIT — see [LICENSE](LICENSE).
