#Requires -Version 5.1
<#
.SYNOPSIS
  Build release binaries and compile the Inno Setup installer.

.DESCRIPTION
  Runs `cargo build --release`, locates ISCC.exe (Inno Setup 6), and
  compiles installer\dns-proxy.iss. Output: dist\DNSProxySetup-0.1.0.exe
#>
$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root

function Find-Iscc {
    $candidates = @(
        "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
        "${env:ProgramFiles}\Inno Setup 6\ISCC.exe",
        "${env:LOCALAPPDATA}\Programs\Inno Setup 6\ISCC.exe"
    )
    foreach ($p in $candidates) {
        if (Test-Path $p) { return $p }
    }
    $cmd = Get-Command iscc -ErrorAction SilentlyContinue
    if ($cmd) { return $cmd.Source }
    return $null
}

Write-Host "Building release binaries..." -ForegroundColor Cyan
cargo build --release
if ($LASTEXITCODE -ne 0) { throw "cargo build --release failed" }

$ui = Join-Path $Root "target\release\dns-proxy-ui.exe"
$svc = Join-Path $Root "target\release\dns-proxy-service.exe"
if (-not (Test-Path $ui)) { throw "Missing $ui" }
if (-not (Test-Path $svc)) { throw "Missing $svc" }

$iscc = Find-Iscc
if (-not $iscc) {
    throw @"
Inno Setup Compiler (ISCC.exe) not found.
Install Inno Setup 6, then re-run this script. Examples:
  winget install --id JRSoftware.InnoSetup -e
  choco install innosetup -y
"@
}

$iss = Join-Path $Root "installer\dns-proxy.iss"
Write-Host "Compiling installer with $iscc ..." -ForegroundColor Cyan
& $iscc $iss
if ($LASTEXITCODE -ne 0) { throw "ISCC failed with exit code $LASTEXITCODE" }

$out = Get-ChildItem (Join-Path $Root "dist\DNSProxySetup-*.exe") -ErrorAction SilentlyContinue |
    Sort-Object LastWriteTime -Descending |
    Select-Object -First 1
if (-not $out) { throw "Installer EXE not found under dist\" }

Write-Host ""
Write-Host "Installer ready: $($out.FullName)" -ForegroundColor Green
