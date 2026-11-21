#Requires -RunAsAdministrator
$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
$svc = Join-Path $Root "target\release\dns-proxy-service.exe"

if (Test-Path $svc) {
    & $svc stop
    & $svc uninstall
} else {
    Write-Host "Binary not found; trying sc.exe delete..." -ForegroundColor Yellow
    sc.exe stop DnsProxy | Out-Null
    sc.exe delete DnsProxy
}

Write-Host "Service removed. Remember to restore your adapter DNS settings." -ForegroundColor Green
