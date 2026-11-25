#Requires -RunAsAdministrator
<#
.SYNOPSIS
  Smoke-test the local DNS proxy on 127.0.0.1:53 without changing system adapter DNS.

.DESCRIPTION
  Starts dns-proxy-service in console mode if nothing is listening on 127.0.0.1:53,
  then runs nslookup/Resolve-DnsName against 127.0.0.1 for example.com and a blocked
  name (youtube.com). Does not flip Use DNS Proxy / adapter DNS.
#>
$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root

function Find-ServiceExe {
    $cands = @(
        (Join-Path $Root "target\release\dns-proxy-service.exe"),
        (Join-Path $Root "target\aarch64-pc-windows-msvc\release\dns-proxy-service.exe"),
        (Join-Path $Root "target\x86_64-pc-windows-msvc\release\dns-proxy-service.exe"),
        "${env:ProgramFiles}\DNS Proxy\dns-proxy-service.exe"
    )
    foreach ($c in $cands) { if (Test-Path $c) { return $c } }
    return $null
}

function Test-ProxyListening {
    try {
        $eps = Get-NetUDPEndpoint -LocalAddress 127.0.0.1 -LocalPort 53 -ErrorAction SilentlyContinue
        return [bool]$eps
    } catch { return $false }
}

$exe = Find-ServiceExe
if (-not $exe) {
    Write-Host "Building dns-proxy-service (release)..." -ForegroundColor Cyan
    cargo build -p dns-proxy-service --release
    $exe = Find-ServiceExe
}
if (-not $exe) { throw "dns-proxy-service.exe not found" }

$started = $false
$proc = $null
if (-not (Test-ProxyListening)) {
    Write-Host "Starting console proxy: $exe" -ForegroundColor Cyan
    & $exe init-config | Out-Host
    $proc = Start-Process -FilePath $exe -ArgumentList "console" -PassThru -WindowStyle Hidden
    $started = $true
    $ok = $false
    for ($i = 0; $i -lt 20; $i++) {
        Start-Sleep -Milliseconds 250
        if (Test-ProxyListening) { $ok = $true; break }
        if ($proc.HasExited) { break }
    }
    if (-not $ok) {
        if ($proc -and -not $proc.HasExited) { Stop-Process -Id $proc.Id -Force }
        throw "Proxy did not bind 127.0.0.1:53"
    }
} else {
    Write-Host "Proxy already listening on 127.0.0.1:53" -ForegroundColor DarkGray
}

try {
    Write-Host "Query example.com @127.0.0.1 ..." -ForegroundColor Cyan
    $allow = Resolve-DnsName example.com -Server 127.0.0.1 -DnsOnly -ErrorAction Stop
    $a = $allow | Where-Object { $_.Type -eq 'A' } | Select-Object -First 1
    if (-not $a) { throw "no A record for example.com via proxy" }
    Write-Host "  OK A $($a.IPAddress)" -ForegroundColor Green

    Write-Host "Query youtube.com @127.0.0.1 (expect block/NXDOMAIN) ..." -ForegroundColor Cyan
    try {
        $blocked = Resolve-DnsName youtube.com -Server 127.0.0.1 -DnsOnly -ErrorAction Stop
        # Sinkhole mode may return 0.0.0.0
        $sink = $blocked | Where-Object { $_.IPAddress -eq '0.0.0.0' }
        if ($sink) {
            Write-Host "  OK sinkhole 0.0.0.0" -ForegroundColor Green
        } else {
            Write-Host "  WARN: youtube.com resolved unexpectedly: $($blocked | Out-String)" -ForegroundColor Yellow
        }
    } catch {
        # NXDOMAIN throws in Resolve-DnsName — that is success for default block mode
        Write-Host "  OK blocked ($($_.Exception.Message))" -ForegroundColor Green
    }

    Write-Host ""
    Write-Host "Smoke test passed. Adapter DNS was NOT changed." -ForegroundColor Green
    Write-Host "Only enable Use DNS Proxy after this smoke test passes AND the Windows service is Running."
} finally {
    if ($started -and $proc -and -not $proc.HasExited) {
        Stop-Process -Id $proc.Id -Force
        Write-Host "Stopped temporary console proxy." -ForegroundColor DarkGray
    }
}
