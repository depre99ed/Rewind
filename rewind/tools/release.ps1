# Build signe + manifeste GitHub + copie Bureau. -Publish publie code et version.
# Exemple : npm run release -- -Version 0.2.2 -Publish -Notes "Corrections"
# ASCII volontaire pour Windows PowerShell 5.1.
param(
    [string]$Version = "",
    [string]$Repo = "depre99ed/Rewind",
    [string]$Notes = "",
    [switch]$Publish
)
$ErrorActionPreference = "Stop"
$root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$workspace = (Resolve-Path (Join-Path $root "..")).Path
$utf8 = New-Object System.Text.UTF8Encoding $false
function Assert-Exit([string]$Action) {
    if ($LASTEXITCODE -ne 0) { throw "$Action (code $LASTEXITCODE)" }
}
Push-Location $root
try {
    if ($Repo -notmatch '^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$') { throw "Depot GitHub invalide" }
    $endpoint = "https://github.com/$Repo/releases/latest/download/latest.json"
    $source = [IO.File]::ReadAllText((Join-Path $root 'src-tauri\src\update.rs'))
    if (-not $source.Contains('pub const ENDPOINT: &str = "' + $endpoint + '";')) {
        throw "Le canal compile dans update.rs ne correspond pas au depot $Repo"
    }
    $confPath = Join-Path $root 'src-tauri\tauri.conf.json'
    $conf = [IO.File]::ReadAllText($confPath) | ConvertFrom-Json
    if ($Version) {
        if ($Version -notmatch '^\d+\.\d+\.\d+$') { throw 'Version attendue : X.Y.Z (version stable)' }
        if ([version]$Version -lt [version]$conf.version) { throw 'Impossible de publier une version inferieure' }
    } else { $Version = $conf.version }
    $key = Join-Path $env:USERPROFILE '.tauri\rewind-updater.key'
    if (-not (Test-Path -LiteralPath $key)) { throw "Cle de signature absente : $key" }
    if ($Publish) {
        $account = & gh api user --jq .login
        Assert-Exit 'Connexion GitHub requise : gh auth login'
        if ($Repo.Split('/')[0] -ne $account.Trim()) { throw "Le depot ne correspond pas au compte connecte" }
        $ErrorActionPreference = 'Continue'
        $remoteRepo = & gh repo view $Repo --json visibility 2>$null
        $repoExists = $LASTEXITCODE -eq 0
        $ErrorActionPreference = 'Stop'
        if ($repoExists) {
            if (($remoteRepo | ConvertFrom-Json).visibility -ne 'PUBLIC') { throw 'Les mises a jour necessitent un depot public' }
            $releaseList = & gh release list --repo $Repo --limit 100 --json tagName,isDraft,isPrerelease
            Assert-Exit 'Lecture des versions GitHub'
            foreach ($release in ($releaseList | ConvertFrom-Json)) {
                if ($release.tagName -eq "v$Version") { throw "La version v$Version existe deja. Choisir un nouveau numero." }
                if (-not $release.isDraft -and -not $release.isPrerelease -and $release.tagName -match '^v?(\d+\.\d+\.\d+)$') {
                    if ([version]$Matches[1] -ge [version]$Version) { throw 'La nouvelle version doit etre superieure a la version publiee' }
                }
            }
        }
    }
    # Synchronise les numeros de version avant de compiler, sans reformater les fichiers.
    foreach ($file in @($confPath, (Join-Path $root 'package.json'))) {
        $jsonText = [IO.File]::ReadAllText($file)
        $jsonText = [regex]::Replace($jsonText, '(?m)^(  "version": )"[^"]+"', '${1}"' + $Version + '"')
        [IO.File]::WriteAllText($file, $jsonText, $utf8)
    }
    $lockPath = Join-Path $root 'package-lock.json'
    # Seules les deux premieres versions sont celles du paquet racine, pas les dependances.
    $originalLock = [IO.File]::ReadAllText($lockPath)
    $rx = New-Object System.Text.RegularExpressions.Regex '("version":\s*)"[^"]+"'
    $lockText = $rx.Replace($originalLock, '${1}"' + $Version + '"', 2)
    [IO.File]::WriteAllText($lockPath, $lockText, $utf8)
    $cargoPath = Join-Path $root 'src-tauri\Cargo.toml'
    $cargoText = [IO.File]::ReadAllText($cargoPath)
    $cargoText = [regex]::Replace($cargoText, '(?m)^version = "[^"]+"', 'version = "' + $Version + '"')
    [IO.File]::WriteAllText($cargoPath, $cargoText, $utf8)
    & npm.cmd run check
    Assert-Exit 'Verification Svelte'
    & node tools/check-i18n.mjs
    Assert-Exit 'Verification des traductions'
    $oldDataDir = $env:REWIND_DATA_DIR
    try {
        $env:REWIND_DATA_DIR = Join-Path $workspace '.release\test-data'
        & cargo test --manifest-path src-tauri/Cargo.toml --lib
        Assert-Exit 'Tests Rust'
    } finally { $env:REWIND_DATA_DIR = $oldDataDir }
    # Le CLI accepte le chemin : la cle ne passe jamais dans les logs ni dans le depot.
    $oldKey = $env:TAURI_SIGNING_PRIVATE_KEY
    $oldPassword = $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD
    try {
        $env:TAURI_SIGNING_PRIVATE_KEY = $key
        $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = ''
        & npx.cmd tauri build
        Assert-Exit 'Compilation Tauri'
    } finally {
        $env:TAURI_SIGNING_PRIVATE_KEY = $oldKey
        $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = $oldPassword
    }
    $nsis = Join-Path $root 'src-tauri\target\release\bundle\nsis'
    $setup = Join-Path $nsis "Rewind_${Version}_x64-setup.exe"
    if (-not (Test-Path -LiteralPath "$setup.sig")) { throw 'Signature absente' }
    $exe = Join-Path $root 'src-tauri\target\release\rewind.exe'
    $verify = Start-Process -FilePath $exe -ArgumentList @('--verify-update', ('"' + $setup + '"'), ('"' + $setup + '.sig"')) -WindowStyle Hidden -Wait -PassThru
    if ($verify.ExitCode -ne 0) { throw 'Verification signature / fichier modifie echouee' }
    $latest = [ordered]@{
        version = $Version
        notes = $Notes
        pub_date = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ')
        platforms = [ordered]@{ 'windows-x86_64' = [ordered]@{
            signature = [IO.File]::ReadAllText("$setup.sig").Trim()
            url = "https://github.com/$Repo/releases/download/v$Version/Rewind_${Version}_x64-setup.exe"
        } }
    }
    $manifest = Join-Path $nsis 'latest.json'
    [IO.File]::WriteAllText($manifest, ($latest | ConvertTo-Json -Depth 5), $utf8)
    & (Join-Path $PSScriptRoot 'copier-setup.ps1')
    if (-not $Publish) { Write-Host "Build signe pret : $setup"; return }
    if (-not $repoExists) {
        & gh repo create $Repo --public --description 'Rewind - clips instantanes pour Windows, Tauri et Svelte'
        Assert-Exit 'Creation du depot'
    }
    & gh auth setup-git
    Assert-Exit 'Configuration GitHub CLI pour Git'
    $publicDir = & (Join-Path $PSScriptRoot 'export-public.ps1') -Repo $Repo -Version $Version
    $publicDir = $publicDir | Select-Object -Last 1
    $head = & git -C $publicDir rev-parse HEAD
    Assert-Exit 'Revision publique'
    $notesPath = Join-Path $nsis 'release-notes.md'
    if (-not $Notes) { $Notes = "Rewind $Version - installation Windows et mises a jour signees via GitHub." }
    [IO.File]::WriteAllText($notesPath, $Notes, $utf8)
    # Brouillon tant que les trois pieces ne sont pas toutes chargees.
    & gh release create "v$Version" $setup "$setup.sig" $manifest --repo $Repo --target $head --title "Rewind $Version" --notes-file $notesPath --draft
    Assert-Exit 'Envoi des fichiers dans la version brouillon'
    & gh release edit "v$Version" --repo $Repo --draft=false --latest
    Assert-Exit 'Publication de la version'
    $report = Join-Path $nsis 'update-verification.json'
    $probe = Start-Process -FilePath $exe -ArgumentList @('--check-update', ('"' + $report + '"'), '--download') -WindowStyle Hidden -Wait -PassThru
    if ($probe.ExitCode -ne 0) { throw "Version publiee, mais verification du telechargement echouee : $report" }
    Write-Host "Publie et signature distante verifiee : https://github.com/$Repo/releases/tag/v$Version"
} finally { Pop-Location }
