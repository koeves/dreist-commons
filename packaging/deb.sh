#!/bin/sh
# Build a .deb from an app's Linux release folder (install.sh next to the
# parts, packaging/README.md: archive layout), with the --system layout under
# /usr (PREFIX=/usr) and the app in /opt/<App>.
#
#   packaging/deb.sh <release-folder> <version> <out-dir>
#
# Environment: DEB_MAINTAINER ("Name <address>", required), DEB_DESCRIPTION
# (one line), DEB_HOMEPAGE. Writes <out-dir>/<app>_<version>_amd64.deb and
# prints its path. Needs dpkg-deb.
set -eu

[ $# = 3 ] || { sed -n '2,11p' "$0" >&2; exit 2; }
src=$(cd "$1" && pwd)
version=${2#v}
out=$3
: "${DEB_MAINTAINER:?DEB_MAINTAINER is required (Name <address>)}"

app=
for p in "$src"/*.vst3; do
    [ -e "$p" ] && app=$(basename "$p" .vst3)
done
[ -n "$app" ] || { echo "No <App>.vst3 in $src" >&2; exit 2; }
pkg=$(printf '%s' "$app" | tr '[:upper:]' '[:lower:]')

root=$(mktemp -d "${TMPDIR:-/tmp}/deb.XXXXXX")
trap 'rm -rf "$root"' EXIT
DESTDIR=$root PREFIX=/usr sh "$src/install.sh" --system >/dev/null

size=$(du -sk "$root" | cut -f1)
mkdir -p "$root/DEBIAN"
cat >"$root/DEBIAN/control" <<CONTROL
Package: $pkg
Version: $version
Architecture: amd64
Maintainer: $DEB_MAINTAINER
Installed-Size: $size
Depends: libc6 (>= 2.39), libasound2t64 | libasound2, libx11-6, libx11-xcb1, libxcb1, libxcursor1, libgl1
Recommends: xdg-desktop-portal | zenity
Section: sound
Priority: optional
Homepage: ${DEB_HOMEPAGE:-https://github.com/koeves/$pkg}
Description: ${DEB_DESCRIPTION:-$app instrument: standalone app, VST3 and CLAP}
CONTROL

mkdir -p "$out"
deb=$out/${pkg}_${version}_amd64.deb
dpkg-deb --root-owner-group -Zxz --build "$root" "$deb" >/dev/null
echo "$deb"
