; DNS Proxy — Inno Setup script
; Build (from repo root, after cargo build --release):
;   iscc installer\dns-proxy.iss
; Or:
;   .\scripts\build-installer.ps1

#define MyAppName "DNS Proxy"
#define MyAppVersion "0.2.0"
#define MyAppPublisher "Warren Coykendall (wcoy7)"
#define MyAppURL "https://github.com/wcoy7/dns-proxy-windows"
#define MyAppExeName "dns-proxy-ui.exe"
#define MyAppServiceExe "dns-proxy-service.exe"
; Stable AppId — do not change between releases
#define MyAppId "{{A7C3E9F2-4B1D-4E8A-9C6F-2D5B8A1E3F70}"

#ifexist "..\target\release\dns-proxy-ui.exe"
  #define SourceRoot "..\target\release"
#else
  #error "Release binaries missing. Run: cargo build --release"
#endif

[Setup]
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
OutputBaseFilename=DNSProxySetup-{#MyAppVersion}
SetupIconFile=..\assets\dns-proxy.ico
UninstallDisplayIcon={app}\{#MyAppExeName}
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
PrivilegesRequired=admin
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
VersionInfoVersion={#MyAppVersion}.0
VersionInfoCompany={#MyAppPublisher}
VersionInfoDescription={#MyAppName} Setup
VersionInfoProductName={#MyAppName}
CloseApplications=yes
RestartApplications=no

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: "installservice"; Description: "Install and start the DNS Proxy Windows service"; GroupDescription: "Windows service:"; Flags: unchecked

[Files]
; Application binaries (built with cargo build --release)
Source: "{#SourceRoot}\{#MyAppExeName}"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceRoot}\{#MyAppServiceExe}"; DestDir: "{app}"; Flags: ignoreversion
; Assets / docs / examples
Source: "..\assets\dns-proxy.ico"; DestDir: "{app}\assets"; Flags: ignoreversion
Source: "..\assets\dns-proxy-icon.png"; DestDir: "{app}\assets"; Flags: ignoreversion
Source: "..\config.example.json"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion
; Helper scripts (paths assume binaries in same folder as install dir when adapted)
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
; Always ensure a default shared ProgramData config exists (migrates LOCALAPPDATA once)
Filename: "{app}\{#MyAppServiceExe}"; Parameters: "init-config"; StatusMsg: "Initializing shared config..."; Flags: runhidden waituntilterminated
; Optional service registration (requires the admin elevation already granted to Setup)
Filename: "{app}\{#MyAppServiceExe}"; Parameters: "install"; StatusMsg: "Installing Windows service..."; Flags: runhidden waituntilterminated; Tasks: installservice
Filename: "{app}\{#MyAppServiceExe}"; Parameters: "start"; StatusMsg: "Starting Windows service..."; Flags: runhidden waituntilterminated; Tasks: installservice
Filename: "{app}\{#MyAppExeName}"; Description: "Launch {#MyAppName} control panel"; Flags: nowait postinstall skipifsilent

[UninstallRun]
; Stop and remove the service before deleting files (best-effort).
; Prefer Quit / uncheck "Use DNS Proxy" in the UI first so adapter DNS is restored.
Filename: "{app}\{#MyAppServiceExe}"; Parameters: "stop"; RunOnceId: "StopDnsProxyService"; Flags: runhidden waituntilterminated skipifdoesntexist
Filename: "{app}\{#MyAppServiceExe}"; Parameters: "uninstall"; RunOnceId: "UninstallDnsProxyService"; Flags: runhidden waituntilterminated skipifdoesntexist
