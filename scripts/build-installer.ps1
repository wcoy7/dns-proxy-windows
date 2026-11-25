#Requires -Version 5.1
<#
.SYNOPSIS
  Build release binaries for this machine's Windows architecture and compile the Inno Setup installer.

.DESCRIPTION
  Detects ARM64 vs AMD64, runs `cargo build --release --target <triple>`, locates
  Inno Setup 7+ ISCC.exe (required for SetupArchitecture=x64), and compiles
  installer\dns-proxy.iss.

  Output: dist\DNSProxySetup-<version>-<arch>.exe
    e.g. dist\DNSProxySetup-0.2.2-arm64.exe on Windows on Arm.

.PARAMETER TargetArch
  Optional override: arm64 | x64. Default = this OS architecture.
#>
param(
    [ValidateSet("arm64", "x64")]
    [string] $TargetArch
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root

function Get-HostTargetArch {
    $osArch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
    switch -Regex ($osArch) {
        'Arm64' { return "arm64" }
        'X64'   { return "x64" }
        default {
            # Fallback for older PowerShell / odd hosts
            if ($env:PROCESSOR_ARCHITECTURE -eq "ARM64") { return "arm64" }
            if ($env:PROCESSOR_ARCHITECTURE -eq "AMD64") { return "x64" }
            if ($env:PROCESSOR_ARCHITEW6432 -eq "AMD64") { return "x64" }
            if ($env:PROCESSOR_ARCHITEW6432 -eq "ARM64") { return "arm64" }
            throw "Unsupported host architecture: OSArchitecture=$osArch PROCESSOR_ARCHITECTURE=$($env:PROCESSOR_ARCHITECTURE)"
        }
    }
}

function Get-RustTriple([string] $Arch) {
    switch ($Arch) {
        "arm64" { return "aarch64-pc-windows-msvc" }
        "x64"   { return "x86_64-pc-windows-msvc" }
        default { throw "Unknown TargetArch: $Arch" }
    }
}

function Find-Iscc {
    $candidates = @(
        "${env:LOCALAPPDATA}\Programs\Inno Setup 7\ISCC.exe",
        "${env:ProgramFiles}\Inno Setup 7\ISCC.exe",
        "${env:ProgramFiles(x86)}\Inno Setup 7\ISCC.exe",
        # Inno 6 cannot emit SetupArchitecture=x64 — keep as last-resort probe for a clear error
        "${env:LOCALAPPDATA}\Programs\Inno Setup 6\ISCC.exe",
        "${env:ProgramFiles}\Inno Setup 6\ISCC.exe",
        "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe"
    )
    foreach ($p in $candidates) {
        if (Test-Path $p) { return $p }
    }
    $cmd = Get-Command iscc -ErrorAction SilentlyContinue
    if ($cmd) { return $cmd.Source }
    return $null
}

function Get-AppVersion {
    $cargo = Join-Path $Root "Cargo.toml"
    $line = Select-String -Path $cargo -Pattern '^\s*version\s*=\s*"([^"]+)"' | Select-Object -First 1
    if (-not $line) { throw "Could not read workspace version from Cargo.toml" }
    return $line.Matches[0].Groups[1].Value
}

if (-not $TargetArch) {
    $TargetArch = Get-HostTargetArch
}
$Triple = Get-RustTriple $TargetArch
$Version = Get-AppVersion
$SourceRoot = Join-Path $Root "target\$Triple\release"

Write-Host "Host/OS target arch : $TargetArch" -ForegroundColor Cyan
Write-Host "Rust target triple  : $Triple" -ForegroundColor Cyan
Write-Host "App version         : $Version" -ForegroundColor Cyan

Write-Host "Ensuring Rust target is installed..." -ForegroundColor Cyan
rustup target add $Triple | Out-Host

Write-Host "Building release binaries..." -ForegroundColor Cyan
cargo build --release --target $Triple
if ($LASTEXITCODE -ne 0) { throw "cargo build --release --target $Triple failed" }

$ui = Join-Path $SourceRoot "dns-proxy-ui.exe"
$svc = Join-Path $SourceRoot "dns-proxy-service.exe"
if (-not (Test-Path $ui)) { throw "Missing $ui" }
if (-not (Test-Path $svc)) { throw "Missing $svc" }

# Sanity-check PE machine type matches the requested arch
Add-Type -TypeDefinition @"
using System;
using System.IO;
public static class PeArch {
  public static string Machine(string path) {
    using (var fs = File.OpenRead(path))
    using (var br = new BinaryReader(fs)) {
      fs.Seek(0x3C, SeekOrigin.Begin);
      int pe = br.ReadInt32();
      fs.Seek(pe, SeekOrigin.Begin);
      br.ReadUInt32(); // signature
      ushort m = br.ReadUInt16();
      if (m == 0x8664) return "AMD64";
      if (m == 0xAA64) return "ARM64";
      if (m == 0x014C) return "i386";
      return "0x" + m.ToString("X4");
    }
  }
}
"@
$expectedPe = if ($TargetArch -eq "arm64") { "ARM64" } else { "AMD64" }
foreach ($bin in @($ui, $svc)) {
    $got = [PeArch]::Machine($bin)
    if ($got -ne $expectedPe) {
        throw "PE machine mismatch for $bin : got $got, expected $expectedPe (target $Triple)"
    }
    Write-Host "OK PE $($got): $bin" -ForegroundColor DarkGray
}

$iscc = Find-Iscc
if (-not $iscc) {
    throw @"
Inno Setup Compiler (ISCC.exe) not found.
Install Inno Setup 7 (required for 64-bit Setup / Arm64 [Run]), then re-run:
  winget install --id JRSoftware.InnoSetup -e
"@
}

# Reject Inno Setup 6 — SetupArchitecture=x64 needs IS 7+
$isccDir = Split-Path $iscc -Parent
if ($isccDir -match 'Inno Setup 6') {
    throw @"
Found Inno Setup 6 at $iscc
DNS Proxy Setup requires Inno Setup 7+ (SetupArchitecture=x64) so post-install
CreateProcess can launch Arm64/x64 app binaries without error 216.
Install Inno Setup 7, then re-run this script.
  winget install --id JRSoftware.InnoSetup -e
"@
}

$iss = Join-Path $Root "installer\dns-proxy.iss"
# SourceRoot for the .iss is relative to the script directory (installer\)
$issSourceRoot = "..\target\$Triple\release"

Write-Host "Compiling installer with $iscc ..." -ForegroundColor Cyan
Write-Host "  /DAppVersion=$Version /DTargetArch=$TargetArch /DSourceRoot=$issSourceRoot"

& $iscc `
    "/DAppVersion=$Version" `
    "/DTargetArch=$TargetArch" `
    "/DSourceRoot=$issSourceRoot" `
    $iss
if ($LASTEXITCODE -ne 0) { throw "ISCC failed with exit code $LASTEXITCODE" }

$expectedName = "DNSProxySetup-$Version-$TargetArch.exe"
$out = Join-Path $Root "dist\$expectedName"
if (-not (Test-Path $out)) {
    $fallback = Get-ChildItem (Join-Path $Root "dist\DNSProxySetup-*.exe") -ErrorAction SilentlyContinue |
        Sort-Object LastWriteTime -Descending |
        Select-Object -First 1
    if ($fallback) { $out = $fallback.FullName }
    else { throw "Installer EXE not found under dist\" }
}

# Confirm Setup EXE is x64 (not x86) — the CreateProcess 216 fix
$setupPe = [PeArch]::Machine($out)
if ($setupPe -ne "AMD64") {
    Write-Warning "Setup PE is $setupPe (expected AMD64). Arm64 [Run] may still hit CreateProcess 216."
} else {
    Write-Host "OK Setup PE AMD64 (64-bit Inno Setup)" -ForegroundColor DarkGray
}

Write-Host ""
Write-Host "Installer ready: $out" -ForegroundColor Green
Write-Host "Architecture:    $TargetArch ($Triple)" -ForegroundColor Green
