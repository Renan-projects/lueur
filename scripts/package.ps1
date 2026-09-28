<#
.SYNOPSIS
    Assemble l'archive de distribution à partir de target\release.
    Utilisé par la CI ; lancer « cargo build --release » avant.
#>
param(
    [string]$PawnIOModulesVersion = '0.2.11',
    [string]$OutDir = 'dist'
)
$ErrorActionPreference = 'Stop'

$root = Split-Path $PSScriptRoot
$version = (Select-String -Path (Join-Path $root 'Cargo.toml') -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
$name = "Lueur-$version"
$out = Join-Path $root $OutDir
$stage = Join-Path $out $name
Remove-Item $stage -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $stage | Out-Null

foreach ($exe in 'lueur.exe', 'lueurctl.exe') {
    Copy-Item (Join-Path $root "target\release\$exe") $stage
}
Copy-Item (Join-Path $root 'LICENSE') (Join-Path $stage 'LICENSE.txt')
Copy-Item (Join-Path $root 'README.md') $stage
foreach ($s in 'install.ps1', 'uninstall.ps1', 'Installer.cmd') {
    Copy-Item (Join-Path $PSScriptRoot $s) $stage
}

# Signed SMBus modules from the official PawnIO.Modules release (LGPL-2.1).
$tag = $PawnIOModulesVersion
$zip = Join-Path $env:TEMP "pawnio-modules-$tag.zip"
$tmp = Join-Path $env:TEMP "pawnio-modules-$tag"
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
Invoke-WebRequest "https://github.com/namazso/PawnIO.Modules/releases/download/$tag/release_$($tag -replace '\.', '_').zip" -OutFile $zip -UseBasicParsing
Expand-Archive $zip $tmp -Force
Copy-Item (Join-Path $tmp 'SmbusPIIX4.bin'), (Join-Path $tmp 'SmbusI801.bin') $stage
Copy-Item (Join-Path $tmp 'COPYING') (Join-Path $stage 'PawnIO-Modules-LICENSE.txt')
Remove-Item $zip, $tmp -Recurse -Force

$archive = Join-Path $out "$name-windows-x64.zip"
Remove-Item $archive -ErrorAction SilentlyContinue
Compress-Archive -Path $stage -DestinationPath $archive
Write-Host "Archive : $archive"
