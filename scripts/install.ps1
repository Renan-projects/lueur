<#
.SYNOPSIS
    Installe Lueur pour l'utilisateur courant.
.DESCRIPTION
    - installe PawnIO (pilote signé, nécessaire pour la RAM RGB) s'il est absent ;
    - copie Lueur dans %LOCALAPPDATA%\Programs\Lueur ;
    - crée un raccourci dans le menu Démarrer ;
    - active le lancement à l'ouverture de session (tâche planifiée, sans invite UAC) ;
    - lance Lueur.
    Le script se relance lui-même en administrateur si nécessaire.
.PARAMETER NoAutostart
    Ne pas lancer Lueur automatiquement à l'ouverture de session.
.PARAMETER NoPawnIO
    Ne pas installer PawnIO (seuls les périphériques USB seront pilotés).
#>
param([switch]$NoAutostart, [switch]$NoPawnIO)
$ErrorActionPreference = 'Stop'

$principal = [Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    $argList = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', "`"$PSCommandPath`"")
    if ($NoAutostart) { $argList += '-NoAutostart' }
    if ($NoPawnIO) { $argList += '-NoPawnIO' }
    Start-Process powershell.exe -Verb RunAs -ArgumentList $argList
    exit
}

$src = $PSScriptRoot
$dest = Join-Path $env:LOCALAPPDATA 'Programs\Lueur'
$files = 'lueur.exe', 'lueurctl.exe', 'SmbusPIIX4.bin', 'SmbusI801.bin', 'LICENSE.txt', 'PawnIO-Modules-LICENSE.txt', 'README.md', 'uninstall.ps1'

foreach ($f in 'lueur.exe', 'lueurctl.exe') {
    if (-not (Test-Path (Join-Path $src $f))) { throw "Fichier manquant à côté du script : $f" }
}

Write-Host '== Lueur : installation ==' -ForegroundColor Cyan

# 1. PawnIO
$pawnLib = Join-Path $env:ProgramFiles 'PawnIO\PawnIOLib.dll'
if ($NoPawnIO) {
    Write-Host 'PawnIO : ignoré (-NoPawnIO).'
} elseif (Test-Path $pawnLib) {
    Write-Host 'PawnIO : déjà installé.'
} else {
    Write-Host 'PawnIO : téléchargement…'
    $setup = Join-Path $env:TEMP 'PawnIO_setup.exe'
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
    Invoke-WebRequest 'https://github.com/namazso/PawnIO.Setup/releases/latest/download/PawnIO_setup.exe' -OutFile $setup -UseBasicParsing
    $sig = Get-AuthenticodeSignature $setup
    if ($sig.Status -ne 'Valid') { throw "Signature de PawnIO_setup.exe invalide ($($sig.Status)), installation annulée." }
    Write-Host "PawnIO : installation (signé par $($sig.SignerCertificate.GetNameInfo('SimpleName', $false)))…"
    $p = Start-Process $setup -ArgumentList '-install', '-silent' -Wait -PassThru
    Remove-Item $setup -ErrorAction SilentlyContinue
    if ($p.ExitCode -ne 0 -or -not (Test-Path $pawnLib)) { throw "L'installation de PawnIO a échoué (code $($p.ExitCode))." }
}

# 2. Fichiers
Get-Process lueur -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 300
New-Item -ItemType Directory -Force $dest | Out-Null
foreach ($f in $files) {
    $from = Join-Path $src $f
    if (Test-Path $from) { Copy-Item $from $dest -Force }
}
Write-Host "Fichiers copiés dans $dest"

# 3. Raccourci du menu Démarrer
$lnk = Join-Path ([Environment]::GetFolderPath('Programs')) 'Lueur.lnk'
$shell = New-Object -ComObject WScript.Shell
$sc = $shell.CreateShortcut($lnk)
$sc.TargetPath = Join-Path $dest 'lueur.exe'
$sc.WorkingDirectory = $dest
$sc.Description = 'Lueur — contrôleur RGB ultra-léger'
$sc.Save()

# 4. Lancement automatique + démarrage
$ctl = Join-Path $dest 'lueurctl.exe'
if ($NoAutostart) {
    & $ctl autostart off 2>$null | Out-Null
    Start-Process (Join-Path $dest 'lueur.exe')
} else {
    & $ctl autostart on
    if ($LASTEXITCODE -ne 0) { throw 'Création de la tâche planifiée impossible.' }
    schtasks.exe /Run /TN Lueur | Out-Null
}

Write-Host ''
Write-Host 'Lueur est installé et lancé : icône arc-en-ciel dans la zone de notification.' -ForegroundColor Green
Write-Host 'Clic sur l''icône = menu des effets. Configuration avancée : lueurctl --help'
if ($Host.Name -eq 'ConsoleHost') { Start-Sleep -Seconds 4 }
