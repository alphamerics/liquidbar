<#
.SYNOPSIS
    Builds Glassbar as a signed MSIX installer with the
    graphicsCaptureProgrammatic capability (removes the yellow WGC border).

.DESCRIPTION
    1. Builds the release exe via "npm run tauri build --no-bundle".
    2. Stages the app + an AppxManifest declaring the capture capability.
    3. Creates (or reuses) a self-signed code-signing certificate whose
       subject matches the manifest publisher.
    4. Packs everything with MakeAppx and signs it with SignTool.
    5. Optionally installs: -Install trusts the cert (needs admin) and
       registers the package for the current user.

.PARAMETER SkipBuild
    Reuses the last release build instead of rebuilding.

.PARAMETER Install
    Trusts the generated certificate and installs the MSIX for the current
    user. Trusting the certificate requires an elevated prompt.

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File packaging\build-installer.ps1 -Install
#>
param(
    [switch]$SkipBuild,
    [switch]$Install
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot

# ---------- Build ----------
$exe = Join-Path $root "src-tauri\target\release\glassbar.exe"
if (-not $SkipBuild) {
    Write-Host "==> Building release binary (tauri build --no-bundle)" -ForegroundColor Cyan
    Push-Location $root
    try {
        npm run tauri build -- --no-bundle
        if ($LASTEXITCODE -ne 0) { throw "tauri build failed" }
    } finally {
        Pop-Location
    }
}
if (-not (Test-Path $exe)) {
    throw "Release exe not found at $exe. Build first."
}

# ---------- Version ----------
$config = Get-Content (Join-Path $root "src-tauri\tauri.conf.json") -Raw | ConvertFrom-Json
$version = "$($config.version).0"

# ---------- Stage ----------
$stage = Join-Path $env:TEMP "glassbar-msix"
$outDir = Join-Path $PSScriptRoot "dist"
Remove-Item $stage -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Path (Join-Path $stage "app") -Force | Out-Null
New-Item -ItemType Directory -Path (Join-Path $stage "Assets") -Force | Out-Null
New-Item -ItemType Directory -Path $outDir -Force | Out-Null

Copy-Item $exe (Join-Path $stage "app\glassbar.exe")
$broker = Join-Path $root "src-tauri\target\release\glass-broker.exe"
if (-not (Test-Path $broker)) {
    throw "Glass broker not found at $broker. Build first."
}
Copy-Item $broker (Join-Path $stage "app\glass-broker.exe")
foreach ($name in @("Square44x44Logo.png", "Square150x150Logo.png", "StoreLogo.png")) {
    Copy-Item (Join-Path $root "src-tauri\icons\$name") (Join-Path $stage "Assets\$name")
}

$manifest = @"
<?xml version="1.0" encoding="utf-8"?>
<Package
  xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10"
  xmlns:uap="http://schemas.microsoft.com/appx/manifest/uap/windows10"
  xmlns:rescap="http://schemas.microsoft.com/appx/manifest/foundation/windows10/restrictedcapabilities">
  <Identity Name="NewMoonWorks.Glassbar" Publisher="CN=Glassbar" Version="$version" />
  <Properties>
    <DisplayName>Glassbar</DisplayName>
    <PublisherDisplayName>New Moon Works</PublisherDisplayName>
    <Logo>Assets\StoreLogo.png</Logo>
  </Properties>
  <Dependencies>
    <TargetDeviceFamily Name="Windows.Desktop" MinVersion="10.0.22000.0" MaxVersionTested="10.0.26100.0" />
  </Dependencies>
  <Resources>
    <Resource Language="en-us" />
  </Resources>
  <Applications>
    <Application Id="Glassbar" Executable="app\glassbar.exe" EntryPoint="Windows.FullTrustApplication">
      <uap:VisualElements
        DisplayName="Glassbar"
        Description="A GPU-rendered Windows 11 taskbar replacement"
        Square150x150Logo="Assets\Square150x150Logo.png"
        Square44x44Logo="Assets\Square44x44Logo.png"
        BackgroundColor="transparent" />
    </Application>
  </Applications>
  <Capabilities>
    <rescap:Capability Name="runFullTrust" />
    <rescap:Capability Name="graphicsCaptureProgrammatic" />
  </Capabilities>
</Package>
"@
Set-Content -Path (Join-Path $stage "AppxManifest.xml") -Value $manifest -Encoding UTF8

# ---------- Tooling (MakeAppx / SignTool from the Windows SDK) ----------
$kits = "C:\Program Files (x86)\Windows Kits\10\bin"
if (-not (Test-Path $kits)) {
    throw "Windows SDK not found at $kits. Install the Windows SDK (the MSVC toolchain installs one)."
}
$sdkBin = Get-ChildItem $kits -Directory |
    Where-Object { Test-Path (Join-Path $_.FullName "x64\MakeAppx.exe") } |
    Sort-Object Name -Descending |
    Select-Object -First 1
if (-not $sdkBin) { throw "MakeAppx.exe not found in $kits" }
$makeappx = Join-Path $sdkBin.FullName "x64\MakeAppx.exe"
$signtool = Get-ChildItem $kits -Filter "signtool.exe" -Recurse |
    Where-Object { $_.FullName -match "\\x64\\" } |
    Sort-Object FullName -Descending |
    Select-Object -First 1
if (-not $signtool) { throw "signtool.exe not found in $kits" }
Write-Host "==> Using MakeAppx: $makeappx" -ForegroundColor DarkGray
Write-Host "==> Using SignTool: $($signtool.FullName)" -ForegroundColor DarkGray

# ---------- Certificate ----------
$subject = "CN=Glassbar"
$cert = Get-ChildItem Cert:\CurrentUser\My |
    Where-Object { $_.Subject -eq $subject } |
    Sort-Object NotAfter -Descending |
    Select-Object -First 1
if (-not $cert) {
    Write-Host "==> Creating self-signed code-signing certificate ($subject)" -ForegroundColor Cyan
    $cert = New-SelfSignedCertificate `
        -Type Custom `
        -Subject $subject `
        -KeyUsage DigitalSignature `
        -FriendlyName "Glassbar MSIX signing" `
        -CertStoreLocation "Cert:\CurrentUser\My"
}
$pfxPath = Join-Path $outDir "Glassbar.pfx"
$cerPath = Join-Path $outDir "Glassbar.cer"
$certPassword = ConvertTo-SecureString -String "glassbar" -AsPlainText -Force
Export-PfxCertificate -Cert $cert -FilePath $pfxPath -Password $certPassword | Out-Null
Export-Certificate -Cert $cert -FilePath $cerPath | Out-Null

# ---------- Pack and sign ----------
$msix = Join-Path $outDir "Glassbar_$($config.version)_x64.msix"
Write-Host "==> Packing $msix" -ForegroundColor Cyan
& $makeappx pack /o /d $stage /p $msix
if ($LASTEXITCODE -ne 0) { throw "MakeAppx failed" }
Write-Host "==> Signing $msix" -ForegroundColor Cyan
& ($signtool.FullName) sign /fd SHA256 /f $pfxPath /p "glassbar" $msix
if ($LASTEXITCODE -ne 0) { throw "SignTool failed" }

Write-Host ""
Write-Host "==> Installer ready: $msix" -ForegroundColor Green
if ($Install) {
    Write-Host "==> Trusting the signing certificate (LocalMachine\TrustedPeople)" -ForegroundColor Cyan
    $trusted = Get-ChildItem Cert:\LocalMachine\TrustedPeople |
        Where-Object { $_.Thumbprint -eq $cert.Thumbprint }
    if (-not $trusted) {
        $process = Start-Process certutil -ArgumentList "-addstore -f TrustedPeople `"$cerPath`"" -Verb RunAs -Wait -PassThru
        if ($process.ExitCode -ne 0) { throw "Could not trust the certificate (needs admin)" }
    }
    Write-Host "==> Installing the package" -ForegroundColor Cyan
    Add-AppxPackage -Path $msix
    Write-Host "==> Installed. Launch Glassbar from the Start menu." -ForegroundColor Green
} else {
    Write-Host ""
    Write-Host "To install:" -ForegroundColor Yellow
    Write-Host "  1. Trust the cert (once, needs admin):"
    Write-Host "     certutil -addstore -f TrustedPeople `"$cerPath`""
    Write-Host "  2. Add-AppxPackage `"$msix`""
    Write-Host "  ...or rerun this script with -Install."
}