#Requires -RunAsAdministrator
$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root

Write-Host "Building release binaries..." -ForegroundColor Cyan
cargo build --release
if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }

$svc = Join-Path $Root "target\release\dns-proxy-service.exe"
if (-not (Test-Path $svc)) { throw "Missing $svc" }

Write-Host "Initializing config (if needed)..." -ForegroundColor Cyan
& $svc init-config

Write-Host "Installing Windows service 'DnsProxy'..." -ForegroundColor Cyan
& $svc install
& $svc start
& $svc status

Write-Host ""
Write-Host "Done. Set your adapter DNS to 127.0.0.1, then: ipconfig /flushdns" -ForegroundColor Green
