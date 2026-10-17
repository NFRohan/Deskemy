# Fetch PDFium (the PDF viewer's renderer) into app/vendor/, where build.rs
# copies it beside the built exe, as it does libmpv. The build and its hash
# are pinned: pdfium-render's `pdfium_7881` feature matches this build. The
# license texts are already in app/licenses/LICENSE-pdfium.txt.
#
#   powershell -File app/scripts/get-pdfium.ps1

$ErrorActionPreference = "Stop"
$Build = "7881"
$Sha256 = "73cc0de638ac2095e7445bf56a38200a5b7c7ca0e9f4ba144598f2457377ac08"
$Url = "https://github.com/bblanchon/pdfium-binaries/releases/download/chromium/$Build/pdfium-win-x64.tgz"

$vendor = Join-Path $PSScriptRoot "..\vendor"
New-Item -ItemType Directory -Force $vendor | Out-Null
$work = Join-Path ([System.IO.Path]::GetTempPath()) "deskemy-pdfium-$Build"
Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $work | Out-Null

$archive = Join-Path $work "pdfium-win-x64.tgz"
Invoke-WebRequest -Uri $Url -OutFile $archive -UseBasicParsing
$hash = (Get-FileHash $archive -Algorithm SHA256).Hash.ToLower()
if ($hash -ne $Sha256) { throw "pdfium-win-x64.tgz: SHA-256 $hash, expected $Sha256" }

tar -xzf $archive -C $work
if ($LASTEXITCODE -ne 0) { throw "couldn't unpack $archive" }
Copy-Item (Join-Path $work "bin\pdfium.dll") (Join-Path $vendor "pdfium.dll") -Force
Remove-Item -Recurse -Force $work
"pdfium.dll (chromium/$Build) is in $((Resolve-Path $vendor).Path)"
