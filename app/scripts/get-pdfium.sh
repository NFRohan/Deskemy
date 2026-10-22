#!/bin/sh
# Fetch PDFium (the PDF viewer's renderer) into app/vendor/, where build.rs
# copies it beside the built binary. Linux's get-pdfium.ps1: the same pinned
# build (pdfium-render's `pdfium_7881` feature matches it), checked by its
# hash. The license texts are already in app/licenses/LICENSE-pdfium.txt.
#
#   sh app/scripts/get-pdfium.sh

set -eu
BUILD=7881
SHA256=1470e21b8b4a3b4ad7f85684e2da11d94f3b69a86d81dee11b9b6709d927ac1d
URL="https://github.com/bblanchon/pdfium-binaries/releases/download/chromium/$BUILD/pdfium-linux-x64.tgz"

vendor="$(cd "$(dirname "$0")/.." && pwd)/vendor"
mkdir -p "$vendor"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

curl -fsSL -o "$work/pdfium.tgz" "$URL"
echo "$SHA256  $work/pdfium.tgz" | sha256sum -c --quiet -
tar -xzf "$work/pdfium.tgz" -C "$work" lib/libpdfium.so
cp "$work/lib/libpdfium.so" "$vendor/libpdfium.so"
echo "libpdfium.so (chromium/$BUILD) is in $vendor"
