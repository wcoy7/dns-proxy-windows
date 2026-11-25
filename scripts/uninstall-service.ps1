#Requires -RunAsAdministrator
$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot

$osArch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
$triple = if ($osArch -match 'Arm64' -or $env:PROCESSOR_ARCHITECTURE -eq 'ARM64') {
    "aarch64-pc-windows-msvc"
} else {
    "x86_64-pc-windows-msvc"
}

$candidates = @(
    (Join-Path $Root "target\$triple\release\dns-proxy-service.exe"),
    (Join-Path $Root "target\release\dns-proxy-service.exe"),
    "${env:ProgramFiles}\DNS Proxy\dns-proxy-service.exe"
)
$svc = $candidates | Where-Object { Test-Path $_ } | Select-Object -First 1

if ($svc) {
    & $svc stop
    & $svc uninstall
} else {
    Write-Host "Binary not found; trying sc.exe delete..." -ForegroundColor Yellow
    sc.exe stop DnsProxy | Out-Null
    sc.exe delete DnsProxy
}

Write-Host "Service removed. Remember to restore your adapter DNS settings." -ForegroundColor Green
