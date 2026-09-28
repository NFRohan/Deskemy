# Build the Windows release artifacts into app/target/packages:
#
#   deskemy_<version>_x64-setup.exe       the per-user installer (cargo-packager, NSIS)
#   deskemy_<version>_x64_en-US.msi       the per-machine installer (WiX, installer/main.wxs)
#   *.sig                                 their signatures, when a signing key is set
#   Deskemy_<version>_x64-portable.zip    the portable build (with its .portable marker)
#   latest.json                           the update manifest, when signed
#   SHA256SUMS.txt                        checksums of the installers and zip
#
# Signing: set CARGO_PACKAGER_SIGN_PRIVATE_KEY (the key, or a path to it) and
# CARGO_PACKAGER_SIGN_PRIVATE_KEY_PASSWORD — the same minisign key the Tauri
# releases were signed with (TAURI_SIGNING_PRIVATE_KEY). See docs/releasing.md.
#
# Usage (from anywhere):  powershell -File app/scripts/package.ps1 [-Notes "..."]

param([string]$Notes = "")

$ErrorActionPreference = "Stop"
Set-Location (Split-Path $PSScriptRoot -Parent)   # app/

$version = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
$out = "target/packages"
Write-Host "Packaging Deskemy $version"

# 0. Clear the last run's outputs, so a stale signature or manifest can't
#    ride along with a new build.
if (Test-Path $out) {
    Get-ChildItem $out -File | Remove-Item -Force
}

# 1. Release build + installers (+ .sig when a key is set).
cargo packager --release
if ($LASTEXITCODE) { throw "cargo packager failed" }
$setupName = "deskemy_${version}_x64-setup.exe"
$msiName = "deskemy_${version}_x64_en-US.msi"
$setup = Join-Path $out $setupName
$msi = Join-Path $out $msiName
foreach ($installer in $setup, $msi) {
    if (-not (Test-Path $installer)) { throw "no installer at $installer" }
}

# 2. Portable zip: the app, libmpv, licenses and the .portable marker, which
#    keeps its data in a data/ folder beside it.
$stage = Join-Path $out "portable"
Remove-Item -Recurse -Force $stage -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $stage | Out-Null
Copy-Item target/release/deskemy.exe, target/release/libmpv-2.dll $stage
Copy-Item -Recurse target/release/licenses (Join-Path $stage "licenses")
New-Item -ItemType File (Join-Path $stage ".portable") | Out-Null
$zip = Join-Path $out "Deskemy_${version}_x64-portable.zip"
Remove-Item -Force $zip -ErrorAction SilentlyContinue
Compress-Archive -Path (Join-Path $stage "*") -DestinationPath $zip
Remove-Item -Recurse -Force $stage

# 3. latest.json: read by the Tauri 1.x updater and this app's, from
#    releases/latest/download/latest.json. MSI installs read
#    windows-x86_64-msi (both updaters), the rest windows-x86_64 — each
#    updates its own kind of install in place. `format` is for cargo-packager's
#    updater; the Tauri one ignores it.
$download = "https://github.com/NFRohan/Deskemy/releases/download/v$version"
if ((Test-Path "$setup.sig") -and (Test-Path "$msi.sig")) {
    $manifest = [ordered]@{
        version   = $version
        notes     = $Notes
        pub_date  = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
        platforms = [ordered]@{
            "windows-x86_64"     = [ordered]@{
                signature = (Get-Content -Raw "$setup.sig").Trim()
                url       = "$download/$setupName"
                format    = "nsis"
            }
            "windows-x86_64-msi" = [ordered]@{
                signature = (Get-Content -Raw "$msi.sig").Trim()
                url       = "$download/$msiName"
                format    = "wix"
            }
        }
    }
    # UTF-8 without a BOM (Windows PowerShell 5.1's Set-Content can't).
    $json = $manifest | ConvertTo-Json -Depth 5
    [System.IO.File]::WriteAllText((Join-Path (Resolve-Path $out).Path "latest.json"), $json, (New-Object System.Text.UTF8Encoding $false))
} else {
    Write-Warning "No signing key set: the installers are unsigned and there's no latest.json (the updaters need both)."
}

# 4. SHA256SUMS.txt for the downloads, as earlier releases published.
$sums = Get-ChildItem $out -File | Where-Object { $_.Name -match '\.(exe|msi|zip)$' } | Sort-Object Name | ForEach-Object {
    "{0}  {1}" -f (Get-FileHash -Algorithm SHA256 $_.FullName).Hash.ToLower(), $_.Name
}
[System.IO.File]::WriteAllText((Join-Path (Resolve-Path $out).Path "SHA256SUMS.txt"), (($sums -join "`n") + "`n"), (New-Object System.Text.UTF8Encoding $false))

Write-Host ""
Get-ChildItem $out -File | Format-Table Name, @{ n = "MB"; e = { [math]::Round($_.Length / 1MB, 1) } } -AutoSize
