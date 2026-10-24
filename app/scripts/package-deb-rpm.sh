#!/bin/sh
# Build the Linux .deb and .rpm into app/target/packages from the AppImage's
# AppDir, so they carry the same bundle: the binary, libmpv (and ffmpeg's
# full codecs) with its libraries, and PDFium, in /opt/deskemy; a deskemy
# command, the menu entry and icon in /usr. Nothing comes from the distro's
# mpv, so stock Fedora plays H.264 without RPM Fusion (docs/linux-port.md).
#
#   sh app/scripts/package-appimage.sh && sh app/scripts/package-deb-rpm.sh
#
# Run on the AppImage's build machine (Ubuntu 24.04): the .deb's dependencies
# are the packages there that hold the libraries left to the host. Needs nfpm
# (fetched, pinned), dpkg-dev, objdump and curl.

set -eu
app=$(cd "$(dirname "$0")/.." && pwd)
version=$(sed -n 's/^version = "\(.*\)"$/\1/p' "$app/Cargo.toml" | head -n 1)
out="$app/target/packages"
work="$app/target/deb-rpm"
appdir="$app/target/appimage/Deskemy.AppDir"
[ -x "$appdir/usr/bin/deskemy" ] || { echo "no AppDir: run package-appimage.sh first" >&2; exit 1; }
mkdir -p "$work" "$out"

NFPM_VERSION=2.47.0
NFPM_SHA256=0660ca602b2d2d2ae4781a06c692b3eeb9d437ffea05b831d76e41f4a3188783
nfpm="$work/nfpm-$NFPM_VERSION"
if [ ! -x "$nfpm" ]; then
    curl -fsSL -o "$work/nfpm.tgz" \
        "https://github.com/goreleaser/nfpm/releases/download/v$NFPM_VERSION/nfpm_${NFPM_VERSION}_Linux_x86_64.tar.gz"
    echo "$NFPM_SHA256  $work/nfpm.tgz" | sha256sum -c --quiet -
    tar -xzf "$work/nfpm.tgz" -C "$work" nfpm
    mv "$work/nfpm" "$nfpm"
fi

# What the bundle needs from the host: the libraries it links to and doesn't
# carry (package-appimage.sh's HOST list, as far as they're used), and the
# newest glibc symbol version anything in it asks for. Only its own links:
# what the host's libraries link to in turn is the host's business (Debian's
# FreeType needs libbz2.so.1.0, which Fedora calls libbz2.so.1).
bundle="$appdir/usr/bin/deskemy $appdir/usr/lib/*"
host=$(for name in $(objdump -p $bundle | awk '$1 == "NEEDED" { print $2 }' | sort -u); do
    case "$name" in libc.so.6 | ld-linux-*) ;; *) [ -e "$appdir/usr/lib/$name" ] || echo "$name" ;; esac
done)
glibc=$(objdump -T $bundle | grep -o 'GLIBC_[0-9][0-9.]*' | sed 's/GLIBC_//' | sort -uV | tail -n 1)
found=$(ldd $bundle)

# .deb: dpkg-shlibdeps' dependencies (the oldest versions the symbols used
# need, and alternatives: libjack.so.0 is "jackd2's or any JACK"), kept to
# the host's libraries. It lists the bundled ones' packages too, which we
# carry: each host library's is the first package of its package's shlibs
# entry, else that package. (dpkg knows a file by the path its package
# installed it at: under /lib or /usr/lib, which usrmerge made the same.)
wanted=" libc6 "
for name in $host; do
    lib=$(echo "$found" | awk -v n="$name" '$1 == n && $3 ~ /^\// { print $3; exit }')
    owner=$( (dpkg -S "$lib" || dpkg -S "/usr$lib" || dpkg -S "${lib#/usr}") 2>/dev/null | head -n 1 | cut -d: -f1,2)
    [ -n "$owner" ] || { echo "no package holds $name ($lib)" >&2; exit 1; }
    shlibs=$(dpkg-query --control-path "$owner" shlibs 2>/dev/null || true)
    first=$([ -n "$shlibs" ] && awk -v n="${name%.so.*}" -v v="${name##*.so.}" '$1 == n && $2 == v { print $3; exit }' "$shlibs")
    wanted="$wanted${first:-${owner%:*}} "
done
mkdir -p "$work/shlibdeps/debian"
printf 'Source: deskemy\n\nPackage: deskemy\nArchitecture: amd64\n' > "$work/shlibdeps/debian/control"
debs=$(cd "$work/shlibdeps" && dpkg-shlibdeps -O --ignore-missing-info -l"$appdir/usr/lib" $bundle 2>/dev/null \
    | sed -n 's/^shlibs:Depends=//p' | tr ',' '\n' | sed 's/^ *//' | while read -r dep; do
        case "$wanted" in *" ${dep%% *} "*) echo "$dep" ;; esac
    done)
for want in $wanted; do
    echo "$debs" | grep -q "^$want\( \|$\)" || { echo "no .deb dependency for $want" >&2; exit 1; }
done
# .rpm: each library by its soname, which Fedora's and openSUSE's packages
# both provide under their own package names.
rpms="libc.so.6(GLIBC_$glibc)(64bit)"
for name in $host; do
    rpms="$rpms
$name()(64bit)"
done
list() { echo "$1" | sort -u | sed 's/.*/    - "&"/'; }

cat > "$work/nfpm.yaml" <<EOF
name: deskemy
arch: amd64
platform: linux
version: "$version"
section: video
priority: optional
maintainer: Nayeem Fardin <nfrohan110@gmail.com>
vendor: Deskemy
homepage: https://github.com/NFRohan/Deskemy
license: MIT AND GPL-2.0-or-later
description: |-
  Offline player for your downloaded video courses.
  Deskemy keeps a library of course folders, resumes every lecture where you
  left it, and shows each lecture's resources beside it. libmpv, ffmpeg's
  codecs and PDFium are bundled in /opt/deskemy.
contents:
  - dst: /opt/deskemy
    type: dir
  - src: $appdir/usr/bin
    dst: /opt/deskemy/bin
    type: tree
  - src: $appdir/usr/lib
    dst: /opt/deskemy/lib
    type: tree
  - src: $appdir/usr/share/doc/deskemy
    dst: /usr/share/doc/deskemy
    type: tree
  - src: /opt/deskemy/bin/deskemy
    dst: /usr/bin/deskemy
    type: symlink
  - src: $appdir/usr/share/applications/deskemy.desktop
    dst: /usr/share/applications/deskemy.desktop
  - src: $appdir/usr/share/icons/hicolor/256x256/apps/deskemy.png
    dst: /usr/share/icons/hicolor/256x256/apps/deskemy.png
  - src: $appdir/usr/share/metainfo/io.github.NFRohan.Deskemy.metainfo.xml
    dst: /usr/share/metainfo/io.github.NFRohan.Deskemy.metainfo.xml
overrides:
  deb:
    depends:
$(list "$debs")
  rpm:
    depends:
$(list "$rpms")
EOF

deb="$out/deskemy_${version}_amd64.deb"
rpm="$out/deskemy_${version}_x86_64.rpm"
rm -f "$deb" "$rpm"
"$nfpm" package --config "$work/nfpm.yaml" --packager deb --target "$deb"
"$nfpm" package --config "$work/nfpm.yaml" --packager rpm --target "$rpm"
echo "glibc $glibc or newer; $(echo "$host" | wc -l) host libraries"
echo "$deb: $(du -h "$deb" | cut -f1)"
echo "$rpm: $(du -h "$rpm" | cut -f1)"
