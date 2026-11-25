; DNS Proxy — Inno Setup script
; Build (from repo root):
;   .\scripts\build-installer.ps1
; Or manually (after cargo build --release --target <triple>):
;   & "$env:LOCALAPPDATA\Programs\Inno Setup 7\ISCC.exe" `
;       /DAppVersion=0.2.1 /DTargetArch=arm64 `
;       /DSourceRoot=..\target\aarch64-pc-windows-msvc\release `
;       installer\dns-proxy.iss
;
; TargetArch (set by build-installer.ps1; override with ISCC /D):
;   arm64 — native ARM64 binaries (this PC / Windows on Arm)
;   x64   — native AMD64 binaries
;
; CreateProcess 216 note:
;   A 32-bit (x86) Setup cannot reliably CreateProcess ARM64 app binaries
;   (ERROR_EXE_MACHINE_TYPE_MISMATCH). We require Inno Setup 7 and build a
;   64-bit (x64) Setup via SetupArchitecture=x64. On Arm64 Windows 11 that
;   Setup runs under x64 emulation and can launch native Arm64 EXEs in [Run].

#define MyAppName "DNS Proxy"
#ifndef AppVersion
  #define AppVersion "0.2.1"
#endif
#define MyAppVersion AppVersion
#define MyAppPublisher "Warren Coykendall (wcoy7)"
#define MyAppURL "https://github.com/wcoy7/dns-proxy-windows"
#define MyAppExeName "dns-proxy-ui.exe"
#define MyAppServiceExe "dns-proxy-service.exe"
; Stable AppId — do not change between releases
#define MyAppId "{{A7C3E9F2-4B1D-4E8A-9C6F-2D5B8A1E3F70}"

#ifndef TargetArch
  #define TargetArch "arm64"
#endif

#ifndef SourceRoot
  #ifexist "..\target\release\dns-proxy-ui.exe"
    #define SourceRoot "..\target\release"
  #elifexist "..\target\aarch64-pc-windows-msvc\release\dns-proxy-ui.exe"
    #define SourceRoot "..\target\aarch64-pc-windows-msvc\release"
  #elifexist "..\target\x86_64-pc-windows-msvc\release\dns-proxy-ui.exe"
    #define SourceRoot "..\target\x86_64-pc-windows-msvc\release"
  #else
    #error "Release binaries missing. Run: .\scripts\build-installer.ps1"
  #endif
#endif

[Setup]
; 64-bit Setup (Inno 7+) so [Run] can CreateProcess Arm64/x64 app binaries.
SetupArchitecture=x64
AppId={#MyAppId}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppVerName={#MyAppName} {#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}
AppUpdatesURL={#MyAppURL}
DefaultDirName={autopf}\{#MyAppName}
DefaultGroupName={#MyAppName}
DisableProgramGroupPage=yes
LicenseFile=..\LICENSE
InfoBeforeFile=post-install-notes.txt
OutputDir=..\dist
OutputBaseFilename=DNSProxySetup-{#MyAppVersion}-{#TargetArch}
SetupIconFile=..\assets\dns-proxy.ico
UninstallDisplayIcon={app}\{#MyAppExeName}
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
PrivilegesRequired=admin
#if TargetArch == "arm64"
ArchitecturesAllowed=arm64
ArchitecturesInstallIn64BitMode=arm64
#else
ArchitecturesAllowed=x64compatible and not arm64
ArchitecturesInstallIn64BitMode=x64compatible
#endif
MinVersion=10.0
VersionInfoVersion={#MyAppVersion}.0
VersionInfoCompany={#MyAppPublisher}
VersionInfoDescription={#MyAppName} Setup ({#TargetArch})
VersionInfoProductName={#MyAppName}
CloseApplications=yes
RestartApplications=no

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: "installservice"; Description: "Install and start the DNS Proxy Windows service"; GroupDescription: "Windows service:"; Flags: unchecked

[Files]
; Application binaries (built for TargetArch)
Source: "{#SourceRoot}\{#MyAppExeName}"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceRoot}\{#MyAppServiceExe}"; DestDir: "{app}"; Flags: ignoreversion
; Assets / docs / examples
Source: "..\assets\dns-proxy.ico"; DestDir: "{app}\assets"; Flags: ignoreversion
Source: "..\assets\dns-proxy-icon.png"; DestDir: "{app}\assets"; Flags: ignoreversion
Source: "..\config.example.json"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion
; Helper scripts
Source: "..\scripts\install-service.ps1"; DestDir: "{app}\scripts"; Flags: ignoreversion
Source: "..\scripts\uninstall-service.ps1"; DestDir: "{app}\scripts"; Flags: ignoreversion

[Icons]
Name: "{group}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; IconFilename: "{app}\assets\dns-proxy.ico"
Name: "{group}\Uninstall {#MyAppName}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; IconFilename: "{app}\assets\dns-proxy.ico"; Tasks: desktopicon

[Dirs]
; Shared machine-wide config (Users get Modify so any login can edit settings)
Name: "{commonappdata}\{#MyAppName}"; Permissions: users-modify

[Run]
; Always ensure a default shared ProgramData config exists (migrates LOCALAPPDATA once).
; Launch via cmd.exe so CreateProcess goes through a same-bitness system binary first
; (extra safety on Arm64 when Setup itself is x64-emulated).
Filename: "{sys}\cmd.exe"; Parameters: "/C ""{app}\{#MyAppServiceExe}"" init-config"; StatusMsg: "Initializing shared config..."; Flags: runhidden waituntilterminated
; Optional service registration (requires the admin elevation already granted to Setup)
Filename: "{sys}\cmd.exe"; Parameters: "/C ""{app}\{#MyAppServiceExe}"" install"; StatusMsg: "Installing Windows service..."; Flags: runhidden waituntilterminated; Tasks: installservice
Filename: "{sys}\cmd.exe"; Parameters: "/C ""{app}\{#MyAppServiceExe}"" start"; StatusMsg: "Starting Windows service..."; Flags: runhidden waituntilterminated; Tasks: installservice
Filename: "{app}\{#MyAppExeName}"; Description: "Launch {#MyAppName} control panel"; Flags: nowait postinstall skipifsilent

[UninstallRun]
; Stop and remove the service before deleting files (best-effort).
; Prefer Quit / uncheck "Use DNS Proxy" in the UI first so adapter DNS is restored.
Filename: "{sys}\cmd.exe"; Parameters: "/C ""{app}\{#MyAppServiceExe}"" stop"; RunOnceId: "StopDnsProxyService"; Flags: runhidden waituntilterminated skipifdoesntexist
Filename: "{sys}\cmd.exe"; Parameters: "/C ""{app}\{#MyAppServiceExe}"" uninstall"; RunOnceId: "UninstallDnsProxyService"; Flags: runhidden waituntilterminated skipifdoesntexist
