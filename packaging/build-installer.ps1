<#
.SYNOPSIS
    Glassbar's installer build. Uses deploy (the sibling installer project)
    as a dependency: points it at packaging\app-pack.json, builds it, and
    packages the result as the Glassbar exe installer.

    The deploy source must be a sibling checkout of this repo (or set
    DEPLOY_DIR). Run deploy\scripts\setup-signing.ps1 once for signing
    materials; signing and the updater manifest are optional steps.

.PARAMETER Sign
    Signs the installer exe with the code-signing certificate. Requires
    DEPLOY_CERT_PASSWORD.

.PARAMETER Manifest
    Generates latest.json next to the installer for the updater (requires a
    prior setup-signing run and -Sign).

.PARAMETER Publish
    Creates the GitHub release (needs gh; implies -Sign -Manifest).

.EXAMPLE
    npm run installer
    npm run installer -- -Sign -Manifest
#>
param(
    [switch]$Sign,
    [switch]$Manifest,
    [switch]$Publish,
    [string]$Repo = "alphamerics/liquidbar"
)

$ErrorActionPreference = "Stop"
if ($Publish) { $Sign = $true; $Manifest = $true }

$root = Split-Path -Parent $PSScriptRoot
$deployDir = if ($env:DEPLOY_DIR) { $env:DEPLOY_DIR } else { Join-Path (Split-Path -Parent $root) "deploy" }
$config = Join-Path $PSScriptRoot "app-pack.json"
if (-not (Test-Path (Join-Path $deployDir "package.json"))) {
    throw "deploy not found at $deployDir (set DEPLOY_DIR or clone it as a sibling)."
}

$cfg = Get-Content $config -Raw | ConvertFrom-Json
$version = "$((Get-Content (Join-Path $PSScriptRoot $cfg.versionSource) -Raw | ConvertFrom-Json).version)"
$outDir = Join-Path $PSScriptRoot "dist"
New-Item -ItemType Directory -Force -Path $outDir | Out-Null
$setupExe = Join-Path $outDir "$($cfg.appName)_$version`_x64-setup.exe"

Write-Host "==> 1/4 Building Glassbar $version" -ForegroundColor Cyan
Push-Location $root
try {
    cmd /c "npm run tauri build -- --no-bundle"
    if ($LASTEXITCODE -ne 0) { throw "glassbar build failed" }
} finally { Pop-Location }

Write-Host "==> 2/4 Building identity package" -ForegroundColor Cyan
& (Join-Path $deployDir "scripts\make-identity-package.ps1") -Config $config

Write-Host "==> 3/4 Building the deploy installer" -ForegroundColor Cyan
$env:DEPLOY_APP_CONFIG = $config
Push-Location $deployDir
try {
    cmd /c "npm run tauri build -- --no-bundle"
    if ($LASTEXITCODE -ne 0) { throw "deploy build failed" }
} finally { Pop-Location }
Copy-Item (Join-Path $deployDir "src-tauri\target\release\deploy.exe") $setupExe -Force

Write-Host "==> 4/4 Packaging" -ForegroundColor Cyan
if ($Sign) {
    $kits = "C:\Program Files (x86)\Windows Kits\10\bin"
    $signtool = Get-ChildItem $kits -Filter "signtool.exe" -Recurse |
        Where-Object { $_.FullName -match "\\x64\\" } |
        Sort-Object FullName -Descending | Select-Object -First 1
    $certName = $cfg.identityPublisher -replace "^CN=", ""
    & $signtool.FullName sign /fd SHA256 /n $certName /s My $setupExe
    if ($LASTEXITCODE -ne 0) { throw "signing the installer failed" }
}

if ($Manifest) {
    & (Join-Path $deployDir "scripts\make-release-manifest.ps1") `
        -Config $config `
        -Artifact $setupExe `
        -Signature "$setupExe.sig" `
        -Version $version `
        -Repo $Repo
}

Write-Host ""
Write-Host "==> Installer ready: $setupExe" -ForegroundColor Green
if ($Publish) {
    gh release create "v$version" $setupExe "$setupExe.sig" (Join-Path $outDir "latest.json") --title "Glassbar v$version" --generate-notes
    if ($LASTEXITCODE -ne 0) { throw "gh release create failed" }
    Write-Host "==> Published v$version" -ForegroundColor Green
}
