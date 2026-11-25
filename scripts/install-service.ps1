#Requires -RunAsAdministrator
$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root

$osArch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
$triple = if ($osArch -match 'Arm64' -or $env:PROCESSOR_ARCHITECTURE -eq 'ARM64') {
    "aarch64-pc-windows-msvc"
} else {
    "x86_64-pc-windows-msvc"
}

Write-Host "Building release binaries ($triple)..." -ForegroundColor Cyan
rustup target add $triple | Out-Null
cargo build --release --target $triple
if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }

$candidates = @(
    (Join-Path $Root "target\$triple\release\dns-proxy-service.exe"),
    (Join-Path $Root "target\release\dns-proxy-service.exe")
)
$svc = $candidates | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $svc) { throw "Missing dns-proxy-service.exe under target\$triple\release or target\release" }

Write-Host "Initializing config (if needed)..." -ForegroundColor Cyan
& $svc init-config

Write-Host "Installing Windows service 'DnsProxy'..." -ForegroundColor Cyan
& $svc install
& $svc start
& $svc status

Write-Host ""
Write-Host "Done. Set your adapter DNS to 127.0.0.1, then: ipconfig /flushdns" -ForegroundColor Green
