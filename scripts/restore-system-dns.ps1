#Requires -RunAsAdministrator
<#
.SYNOPSIS
  Emergency: restore adapter DNS away from 127.0.0.1 so the machine can resolve again.

.DESCRIPTION
  If "Use DNS Proxy" left adapters on 127.0.0.1 but the DnsProxy service is down,
  Windows cannot resolve names. This script:
    1. Prefers restoring from %ProgramData%\DNS Proxy\dns-backup.json when present
    2. Otherwise resets every adapter that points at 127.0.0.1 / ::1 to DHCP
    3. Flushes the DNS client cache
    4. Smoke-tests resolution of example.com
#>
$ErrorActionPreference = "Stop"

$backupPath = Join-Path $env:ProgramData "DNS Proxy\dns-backup.json"
Write-Host "Emergency DNS restore" -ForegroundColor Cyan

function Set-StaticOrDhcp {
    param(
        [string] $Alias,
        [bool] $Dhcp,
        [string[]] $Servers
    )
    if ($Dhcp -or -not $Servers -or $Servers.Count -eq 0) {
        Set-DnsClientServerAddress -InterfaceAlias $Alias -ResetServerAddresses
        Write-Host "  $Alias → DHCP / automatic"
    } else {
        Set-DnsClientServerAddress -InterfaceAlias $Alias -ServerAddresses $Servers
        Write-Host "  $Alias → $($Servers -join ', ')"
    }
}

$restoredFromBackup = $false
if (Test-Path $backupPath) {
    try {
        $backup = Get-Content $backupPath -Raw | ConvertFrom-Json
        Write-Host "Using backup: $backupPath"
        foreach ($a in $backup.adapters) {
            $alias = $a.friendly_name
            $adapter = Get-NetAdapter -Name $alias -ErrorAction SilentlyContinue
            if (-not $adapter) {
                Write-Host "  (skip missing adapter '$alias')" -ForegroundColor Yellow
                continue
            }
            Set-StaticOrDhcp -Alias $alias -Dhcp:([bool]$a.dhcp_dns) -Servers @($a.servers)
        }
        Remove-Item $backupPath -Force
        Write-Host "Removed backup file."
        $restoredFromBackup = $true
    } catch {
        Write-Host "Backup restore failed: $_ — falling back to DHCP reset." -ForegroundColor Yellow
    }
}

if (-not $restoredFromBackup) {
    Write-Host "Resetting adapters that list 127.0.0.1 / ::1 to DHCP..."
    $hits = Get-DnsClientServerAddress -AddressFamily IPv4, IPv6 |
        Where-Object {
            $_.ServerAddresses -contains '127.0.0.1' -or
            $_.ServerAddresses -contains '::1'
        }
    if (-not $hits) {
        Write-Host "No adapter currently lists loopback DNS."
    }
    foreach ($h in $hits) {
        Set-DnsClientServerAddress -InterfaceAlias $h.InterfaceAlias -ResetServerAddresses
        Write-Host "  $($h.InterfaceAlias) → DHCP (was $($h.ServerAddresses -join ', '))"
    }
}

ipconfig /flushdns | Out-Null
Write-Host ""
Write-Host "Current DNS:" -ForegroundColor Cyan
Get-DnsClientServerAddress -AddressFamily IPv4 |
    Where-Object { $_.ServerAddresses } |
    Format-Table InterfaceAlias, ServerAddresses -AutoSize

Write-Host "Resolve test:" -ForegroundColor Cyan
try {
    $r = Resolve-DnsName example.com -ErrorAction Stop | Select-Object -First 2
    $r | Format-Table Name, Type, IPAddress -AutoSize
    Write-Host "OK — name resolution works." -ForegroundColor Green
} catch {
    Write-Host "FAIL — Resolve-DnsName example.com: $_" -ForegroundColor Red
    Write-Host "Try: Settings → Network → your adapter → DNS → Automatic (DHCP)"
    exit 1
}
