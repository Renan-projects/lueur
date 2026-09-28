<#
.SYNOPSIS
    Désinstalle Lueur.
.PARAMETER Purge
    Supprime aussi la configuration et le journal (%APPDATA%\Lueur).
.NOTES
    PawnIO n'est pas désinstallé : d'autres logiciels (OpenRGB, FanControl,
    LibreHardwareMonitor…) peuvent s'en servir. Pour le retirer :
    Paramètres > Applications > PawnIO.
#>
param([switch]$Purge)
$ErrorActionPreference = 'Stop'

$principal = [Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    $argList = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', "`"$PSCommandPath`"")
    if ($Purge) { $argList += '-Purge' }
    Start-Process powershell.exe -Verb RunAs -ArgumentList $argList
    exit
}

$dest = Join-Path $env:LOCALAPPDATA 'Programs\Lueur'
Get-Process lueur -ErrorAction SilentlyContinue | Stop-Process -Force
schtasks.exe /Delete /TN Lueur /F 2>$null | Out-Null
Remove-Item (Join-Path ([Environment]::GetFolderPath('Programs')) 'Lueur.lnk') -ErrorAction SilentlyContinue
Start-Sleep -Milliseconds 300
# The script may live inside $dest: remove it last, from a detached process.
Start-Process cmd.exe -WindowStyle Hidden -ArgumentList '/c', "timeout /t 2 >nul & rmdir /s /q `"$dest`""
if ($Purge) { Remove-Item (Join-Path $env:APPDATA 'Lueur') -Recurse -Force -ErrorAction SilentlyContinue }

Write-Host 'Lueur est désinstallé.' -ForegroundColor Green
if ($Host.Name -eq 'ConsoleHost') { Start-Sleep -Seconds 3 }
