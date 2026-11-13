# Copie le dernier installeur compile sur le Bureau (Rewind-setup.exe), en remplacant l'ancien.
# (Sans accents : Windows PowerShell 5.1 lit ce fichier en ANSI.)
$ErrorActionPreference = "Stop"
$nsis = Join-Path $PSScriptRoot "..\src-tauri\target\release\bundle\nsis"
$setup = Get-ChildItem $nsis -Filter "*-setup.exe" | Sort-Object LastWriteTime -Descending | Select-Object -First 1
if (-not $setup) { throw "Aucun installeur dans $nsis" }
$desk = [Environment]::GetFolderPath("Desktop")
$dest = Join-Path $desk "Rewind-setup.exe"
Copy-Item $setup.FullName $dest -Force
Write-Host "Installeur copie sur le Bureau : $dest ($([math]::Round($setup.Length / 1MB, 1)) Mo)"
