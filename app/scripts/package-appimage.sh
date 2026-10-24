#!/bin/sh
# Build the Linux AppImage into app/target/packages/Deskemy_<version>_x86_64.AppImage:
# the release binary with libmpv (and so ffmpeg's codecs), the libraries
# they need and PDFium, so playback doesn't depend on what the distro ships
# (docs/linux-port.md, issue 9). Unsigned: package.ps1 signs it for a release.
#
#   sh app/scripts/get-pdfium.sh && sh app/scripts/package-appimage.sh
#
# Needs libmpv (the system's, bundled), patchelf, curl and file. A binary runs where
# glibc is at least as new as the build machine's: build on the oldest distro
# to support (CI: Ubuntu 24.04).

set -eu
app=$(cd "$(dirname "$0")/.." && pwd)
version=$(sed -n 's/^version = "\(.*\)"$/\1/p' "$app/Cargo.toml" | head -n 1)
out="$app/target/packages"
work="$app/target/appimage"
appdir="$work/Deskemy.AppDir"
appimage="$out/Deskemy_${version}_x86_64.AppImage"

# The host's own libraries, never bundled: the AppImage community's
# excludelist (github.com/AppImageCommunity/pkg2appimage, as of 15a64c2),
# what every desktop distro has and what has to be the host's own:
# - glibc and the C++ runtime.
# - The graphics stack (GL, EGL, gbm, drm): its drivers come from the host.
# - The display and sound servers' core client libraries (X11, xcb, Wayland,
#   ALSA, PipeWire, JACK): they speak the host server's protocol.
# - fontconfig, FreeType, HarfBuzz, FriBidi and a few base libraries.
# Plus VA-API: the host's GPU drivers only load into a libva at least as new
# as the one they were built for, so a bundled one would lose hardware decoding.
# Everything else is bundled, even what most desktops have: a fresh Fedora
# Workstation has no libXss or libXpresent, which libmpv needs.
HOST=" $(echo "
    ld-linux-x86-64.so.2 libanl.so.1 libBrokenLocale.so.1 libc.so.6 libdl.so.2 libm.so.6
    libmvec.so.1 libnss_compat.so.2 libnss_dns.so.2 libnss_files.so.2 libpthread.so.0
    libresolv.so.2 librt.so.1 libthread_db.so.1 libutil.so.1 libstdc++.so.6 libgcc_s.so.1
    libGL.so.1 libEGL.so.1 libGLdispatch.so.0 libGLX.so.0 libOpenGL.so.0 libglapi.so.0
    libdrm.so.2 libgbm.so.1 libxcb-dri2.so.0 libxcb-dri3.so.0
    libX11.so.6 libX11-xcb.so.1 libxcb.so.1 libwayland-client.so.0
    libasound.so.2 libjack.so.0 libpipewire-0.3.so.0
    libfontconfig.so.1 libfreetype.so.6 libharfbuzz.so.0 libfribidi.so.0
    libcom_err.so.2 libexpat.so.1 libgmp.so.10 libgpg-error.so.0 libICE.so.6 libSM.so.6
    libusb-1.0.so.0 libuuid.so.1 libz.so.1
    libva.so.2 libva-drm.so.2 libva-x11.so.2 libva-wayland.so.2
" | tr -s "[:space:]" " ") "

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
# (A library can be listed twice, under /lib and /usr/lib: the first is kept.)
for lib in $(ldd "$binary" "$libmpv" "$pdfium" | awk '$2 == "=>" && $3 ~ /^\// { print $3 }' | sort -u); do
    name=$(basename "$lib")
    case "$HOST" in
        *" $name "*) ;;
        *) [ -e "$appdir/usr/lib/$name" ] || cp -L "$lib" "$appdir/usr/lib/" ;;
    esac
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
# The software centre's page for it. A release lists itself first there; a
# test build (2.1.1-3) needn't.
metainfo="$app/linux/io.github.NFRohan.Deskemy.metainfo.xml"
listed=$(sed -n 's/.*<release version="\([^"]*\)".*/\1/p' "$metainfo" | head -n 1)
case "$version" in
    *-*) ;;
    "$listed") ;;
    *) echo "$version isn't the newest <release> in $metainfo" >&2; exit 1 ;;
esac
mkdir -p "$appdir/usr/share/metainfo"
cp "$metainfo" "$appdir/usr/share/metainfo/"
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
