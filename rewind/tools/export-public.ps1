# Checkout de publication sans historique ni donnees personnelles locales.
param([Parameter(Mandatory=$true)][string]$Repo, [Parameter(Mandatory=$true)][string]$Version)
$ErrorActionPreference = 'Stop'
$workspace = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$stageRoot = Join-Path $workspace '.release'
$stage = Join-Path $stageRoot ("source-$Version-" + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $stageRoot -Force | Out-Null
& git clone "https://github.com/$Repo.git" $stage | Out-Host
if ($LASTEXITCODE -ne 0) { throw 'Clone du depot public impossible' }
& git -C $stage checkout -B main | Out-Host
if ($LASTEXITCODE -ne 0) { throw 'Branche de publication impossible' }
$files = @(& git -C $workspace ls-files --cached --others --exclude-standard -- rewind docs)
if ($LASTEXITCODE -ne 0) { throw 'Inventaire des sources impossible' }
$files = @($files | Where-Object { $_ -notmatch '(^|/)(\.claude|\.vscode|\.wrangler|node_modules|target|build|\.svelte-kit|gen)/' -and $_ -notmatch '(\.key|\.pem|\.log|\.env)$' })
$files += @('README.md', 'README.en.md', '.gitignore', 'PUBLICATION.md')
$files = @($files | Sort-Object -Unique | Where-Object { Test-Path -LiteralPath (Join-Path $workspace $_) -PathType Leaf })
# Retirer uniquement les fichiers precedemment publies qui n'existent plus.
$prefix = [IO.Path]::GetFullPath($stage).TrimEnd('\') + '\'
foreach ($old in @(& git -C $stage ls-files)) {
    if ($files -notcontains $old) {
        $target = [IO.Path]::GetFullPath((Join-Path $stage $old))
        if (-not $target.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) { throw 'Chemin hors du checkout' }
        if (Test-Path -LiteralPath $target -PathType Leaf) { Remove-Item -LiteralPath $target }
    }
}
foreach ($file in $files) {
    $dest = Join-Path $stage $file
    New-Item -ItemType Directory -Path (Split-Path $dest) -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $workspace $file) -Destination $dest -Force
}
& git -C $stage config user.name 'depre99ed'
& git -C $stage config user.email '331618125+depre99ed@users.noreply.github.com'
& git -C $stage add --all
if ($LASTEXITCODE -ne 0) { throw 'Preparation des sources impossible' }
& git -C $stage diff --cached --quiet
if ($LASTEXITCODE -eq 1) {
    & git -C $stage commit -m "Rewind $Version : sources de la version publique" | Out-Host
    if ($LASTEXITCODE -ne 0) { throw 'Commit public impossible' }
} elseif ($LASTEXITCODE -ne 0) { throw 'Verification du diff impossible' }
& git -C $stage push origin main | Out-Host
if ($LASTEXITCODE -ne 0) { throw 'Publication des sources impossible' }
Write-Output $stage
