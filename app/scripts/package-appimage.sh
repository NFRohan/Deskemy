#!/bin/sh
# Build the Linux AppImage into app/target/packages/Deskemy_<version>_x86_64.AppImage:
# the release binary with libmpv (and so ffmpeg's codecs), the libraries
# they need and PDFium, so playback doesn't depend on what the distro ships
# (docs/linux-port.md, issue 9). Unsigned: package.ps1 signs it for a release.
#
#   sh app/scripts/get-pdfium.sh && sh app/scripts/package-appimage.sh
#
# Needs libmpv (the system's, bundled), patchelf and curl. A binary runs where
# glibc is at least as new as the build machine's: build on the oldest distro
# to support (CI: Ubuntu 24.04).

set -eu
app=$(cd "$(dirname "$0")/.." && pwd)
version=$(sed -n 's/^version = "\(.*\)"$/\1/p' "$app/Cargo.toml" | head -n 1)
out="$app/target/packages"
work="$app/target/appimage"
appdir="$work/Deskemy.AppDir"
appimage="$out/Deskemy_${version}_x86_64.AppImage"

# The host's own libraries, never bundled:
# - glibc and the C++ runtime: they must be the host's, and every distro has them.
# - The graphics stack (GL, EGL, gbm, drm) and VA-API / VDPAU: their drivers
#   come from the host and must match its versions.
# - The display and sound servers' client libraries (X11, xcb, Wayland,
#   ALSA, PulseAudio, PipeWire, JACK): they speak the host server's protocol
#   and read its configuration.
# - fontconfig and FreeType: they read the host's font configuration.
# glibc's short names match whole (libm isn't libmpv); the rest by prefix.
GLIBC='(libc|libm|libdl|libpthread|librt|libresolv|libutil|libstdc\+\+|libgcc_s)\.so'
HOST="^($GLIBC|ld-linux|libGL|libEGL|libOpenGL|libgbm|libdrm|libglapi|libva|libvdpau|libX|libxcb|libxkbcommon|libwayland|libasound|libpulse|libpipewire|libjack|libfontconfig|libfreetype)"

(cd "$app" && cargo build --release)
binary="$app/target/release/deskemy"
pdfium="$app/target/release/libpdfium.so"
[ -f "$pdfium" ] || { echo "no libpdfium.so: run get-pdfium.sh first" >&2; exit 1; }
libmpv=$(ldconfig -p | awk '/libmpv\.so\.2 .*x86-64/ { print $NF; exit }')
[ -n "$libmpv" ] || { echo "no libmpv.so.2 on this system" >&2; exit 1; }

rm -rf "$appdir"
mkdir -p "$appdir/usr/bin" "$appdir/usr/lib" "$appdir/usr/share/applications" \
    "$appdir/usr/share/icons/hicolor/256x256/apps" "$appdir/usr/share/doc/deskemy" "$out"
cp "$binary" "$appdir/usr/bin/deskemy"
cp -r "$app/target/release/licenses/." "$appdir/usr/share/doc/deskemy/"

# libmpv is loaded at run time, so it isn't among the binary's own libraries:
# bundle it and PDFium, then everything the three of them link to.
cp -L "$libmpv" "$appdir/usr/lib/libmpv.so.2"
cp "$pdfium" "$appdir/usr/lib/libpdfium.so"
for lib in $(ldd "$binary" "$libmpv" "$pdfium" | awk '$2 == "=>" && $3 ~ /^\// { print $3 }' | sort -u); do
    basename "$lib" | grep -Eq "$HOST" || cp -Ln "$lib" "$appdir/usr/lib/"
done

# They find each other through their run paths, not LD_LIBRARY_PATH: every
# program Deskemy opens (a PDF app, the browser) would inherit that, and load
# our libraries instead of its own.
patchelf --set-rpath '$ORIGIN/../lib' "$appdir/usr/bin/deskemy"
for lib in "$appdir"/usr/lib/*; do
    patchelf --set-rpath '$ORIGIN' "$lib"
done

cat > "$appdir/usr/share/applications/deskemy.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Deskemy
GenericName=Course player
Comment=Offline player for your downloaded video courses
Exec=deskemy
Icon=deskemy
Terminal=false
Categories=Education;AudioVideo;Video;Player;
EOF
cp "$app/ui/logo.png" "$appdir/usr/share/icons/hicolor/256x256/apps/deskemy.png"
ln -s usr/share/applications/deskemy.desktop "$appdir/deskemy.desktop"
ln -s usr/share/icons/hicolor/256x256/apps/deskemy.png "$appdir/deskemy.png"
ln -s deskemy.png "$appdir/.DirIcon"
ln -s usr/bin/deskemy "$appdir/AppRun"

# appimagetool packs it with the static AppImage runtime, which runs without
# libfuse2 (gone from many distros). It's an AppImage itself; extracting it
# instead of mounting needs no FUSE in the build machine either.
tool="$work/appimagetool"
if [ ! -x "$tool" ]; then
    curl -fsSL -o "$tool" https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage
    chmod +x "$tool"
fi
rm -f "$appimage"
ARCH=x86_64 APPIMAGE_EXTRACT_AND_RUN=1 "$tool" --no-appstream "$appdir" "$appimage"
echo "$appimage: $(du -h "$appimage" | cut -f1), $(ls "$appdir/usr/lib" | wc -l) libraries bundled"
