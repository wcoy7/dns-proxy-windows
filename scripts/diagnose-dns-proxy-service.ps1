#Requires -RunAsAdministrator
<#
.SYNOPSIS
  Diagnose why the DnsProxy Windows service stays Stopped / fails to start.

.DESCRIPTION
  Prints sc query/qc, binary path + PE machine type, ProgramData config,
  who owns UDP/TCP :53, recent Service Control Manager events, and tries
  a short console bind test without leaving the service running.
#>
$ErrorActionPreference = "Continue"

function Get-PeMachine([string]$path) {
    if (-not (Test-Path $path)) { return "MISSING" }
    $fs = [IO.File]::OpenRead((Resolve-Path $path))
    $br = New-Object IO.BinaryReader($fs)
    $fs.Seek(0x3C, 'Begin') | Out-Null
    $pe = $br.ReadInt32()
    $fs.Seek($pe, 'Begin') | Out-Null
    $null = $br.ReadBytes(4)
    $m = $br.ReadUInt16()
    $br.Close(); $fs.Close()
    switch ($m) {
        0x014c { 'i386' }
        0x8664 { 'AMD64' }
        0xAA64 { 'ARM64' }
        default { ("0x{0:X4}" -f $m) }
    }
}

$osArch = [Runtime.InteropServices.RuntimeInformation]::OSArchitecture
Write-Host "OS architecture: $osArch  PROCESSOR_ARCHITECTURE=$($env:PROCESSOR_ARCHITECTURE)" -ForegroundColor Cyan
Write-Host ""

Write-Host "=== sc query / qc DnsProxy ===" -ForegroundColor Cyan
sc.exe query DnsProxy
sc.exe qc DnsProxy
$svc = Get-CimInstance Win32_Service -Filter "Name='DnsProxy'" -ErrorAction SilentlyContinue
if ($svc) {
    $svc | Format-List Name, State, Status, StartMode, PathName, ProcessId, ExitCode, ServiceSpecificExitCode
    $bin = ($svc.PathName -replace '^"([^"]+)".*', '$1' -replace '^([^\s]+).*', '$1')
    Write-Host "Binary path: $bin"
    Write-Host "Binary PE:   $(Get-PeMachine $bin)"
    $expected = if ($osArch -match 'Arm64') { 'ARM64' } else { 'AMD64' }
    if ((Get-PeMachine $bin) -ne $expected -and (Get-PeMachine $bin) -ne 'MISSING') {
        Write-Host "MISMATCH: OS wants $expected but service binary is $(Get-PeMachine $bin) — SCM start often fails (error 216)." -ForegroundColor Red
        Write-Host "Fix: install DNSProxySetup-*-x64.exe on Intel/AMD, or *-arm64.exe on Windows on Arm; then reinstall the service." -ForegroundColor Yellow
    }
} else {
    Write-Host "DnsProxy is NOT installed (OpenService 1060)." -ForegroundColor Yellow
    Write-Host "Fix: elevated UI → Install, or: dns-proxy-service.exe install && dns-proxy-service.exe start"
}

Write-Host ""
Write-Host "=== ProgramData config ===" -ForegroundColor Cyan
$cfgDir = Join-Path $env:ProgramData "DNS Proxy"
$cfg = Join-Path $cfgDir "config.json"
if (Test-Path $cfg) {
    Get-Item $cfg | Format-List FullName, Length, LastWriteTime
    Get-Content $cfg
} else {
    Write-Host "MISSING $cfg — run: dns-proxy-service.exe init-config" -ForegroundColor Yellow
}

Write-Host ""
Write-Host "=== Who owns port 53? ===" -ForegroundColor Cyan
$owners = @()
Get-NetUDPEndpoint -LocalPort 53 -ErrorAction SilentlyContinue | ForEach-Object {
    $owners += [pscustomobject]@{ Proto = 'UDP'; Addr = $_.LocalAddress; Pid = $_.OwningProcess }
}
Get-NetTCPConnection -LocalPort 53 -ErrorAction SilentlyContinue | ForEach-Object {
    $owners += [pscustomobject]@{ Proto = 'TCP'; Addr = $_.LocalAddress; Pid = $_.OwningProcess }
}
if (-not $owners) {
    Write-Host "Nothing listening on :53 (good — free for DnsProxy)."
} else {
    $owners | Format-Table -AutoSize
    foreach ($o in ($owners.Pid | Select-Object -Unique)) {
        $p = Get-Process -Id $o -ErrorAction SilentlyContinue
        if ($p) {
            Write-Host ("PID {0} = {1}  {2}" -f $o, $p.ProcessName, $p.Path) -ForegroundColor Yellow
        }
    }
    Write-Host "If another process holds :53, DnsProxy bind fails and the service stays/returns Stopped." -ForegroundColor Yellow
}

Write-Host ""
Write-Host "=== Recent Service Control Manager events (DnsProxy) ===" -ForegroundColor Cyan
$since = (Get-Date).AddDays(-2)
Get-WinEvent -FilterHashtable @{ LogName = 'System'; StartTime = $since; ProviderName = 'Service Control Manager' } -ErrorAction SilentlyContinue |
    Where-Object { $_.Message -match 'DnsProxy|DNS Proxy' } |
    Select-Object -First 15 TimeCreated, Id, Message |
    Format-List

Write-Host "=== Application Error / .NET Runtime (dns-proxy) ===" -ForegroundColor Cyan
Get-WinEvent -FilterHashtable @{ LogName = 'Application'; StartTime = $since } -ErrorAction SilentlyContinue |
    Where-Object { $_.Message -match 'dns-proxy|DnsProxy' } |
    Select-Object -First 10 TimeCreated, Id, ProviderName, Message |
    Format-List

Write-Host ""
Write-Host "=== Console bind smoke (10s, then stop) ===" -ForegroundColor Cyan
$cands = @(
    "${env:ProgramFiles}\DNS Proxy\dns-proxy-service.exe",
    (Join-Path (Split-Path -Parent $PSScriptRoot) "target\release\dns-proxy-service.exe"),
    (Join-Path (Split-Path -Parent $PSScriptRoot) "target\x86_64-pc-windows-msvc\release\dns-proxy-service.exe"),
    (Join-Path (Split-Path -Parent $PSScriptRoot) "target\aarch64-pc-windows-msvc\release\dns-proxy-service.exe")
)
$exe = $cands | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $exe) {
    Write-Host "No dns-proxy-service.exe found to console-test." -ForegroundColor Yellow
} else {
    Write-Host "Using $exe  PE=$(Get-PeMachine $exe)"
    if (Get-NetUDPEndpoint -LocalPort 53 -EA SilentlyContinue) {
        Write-Host "Skip console test — :53 already in use." -ForegroundColor Yellow
    } else {
        $p = Start-Process -FilePath $exe -ArgumentList "console" -PassThru -WindowStyle Hidden
        Start-Sleep 2
        if ($p.HasExited) {
            Write-Host "Console exited immediately (exit $($p.ExitCode)) — bind/config failure. Check console output by running:" -ForegroundColor Red
            Write-Host "  & '$exe' console"
        } else {
            $ep = Get-NetUDPEndpoint -LocalAddress 127.0.0.1 -LocalPort 53 -EA SilentlyContinue
            if ($ep) {
                Write-Host "Console bind OK on 127.0.0.1:53" -ForegroundColor Green
                try {
                    Resolve-DnsName example.com -Server 127.0.0.1 -DnsOnly -EA Stop | Select-Object -First 1 | Format-Table
                } catch {
                    Write-Host "Bound but resolve failed: $_" -ForegroundColor Yellow
                }
            } else {
                Write-Host "Process alive but not listening on 127.0.0.1:53" -ForegroundColor Red
            }
            Stop-Process -Id $p.Id -Force
        }
    }
}

Write-Host ""
Write-Host "=== Suggested next steps ===" -ForegroundColor Cyan
Write-Host "1. Paste this script's output (especially WIN32_EXIT_CODE, PathName PE, :53 owners, SCM events)."
Write-Host "2. If PE mismatch: reinstall matching Setup (x64 on i5), then:"
Write-Host "     sc stop DnsProxy; sc delete DnsProxy"
Write-Host "     & 'C:\Program Files\DNS Proxy\dns-proxy-service.exe' install"
Write-Host "     & 'C:\Program Files\DNS Proxy\dns-proxy-service.exe' start"
Write-Host "3. If :53 busy: stop the other DNS product, then sc start DnsProxy"
Write-Host "4. If config missing: dns-proxy-service.exe init-config"
Write-Host "5. Do NOT reboot until DNS adapters are not stuck on 127.0.0.1 (run restore-system-dns.ps1 first if needed)."
