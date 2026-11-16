#!/bin/sh
# Add .debs to an apt repository checkout (served by GitHub Pages) and
# rebuild its signed index: pool/main/<p>/<package>/, dists/stable/ with
# Packages, Release, Release.gpg and InRelease, and key.asc at the top.
# The two newest versions of each package stay in the pool.
#
#   packaging/apt-publish.sh <repo-dir> <deb>...
#
# Environment: APT_KEY_ID, the signing key's ID or fingerprint, already in
# the gpg keyring. Needs apt-ftparchive (apt-utils), dpkg-deb and gpg.
set -eu

[ $# -ge 2 ] || { sed -n '2,10p' "$0" >&2; exit 2; }
: "${APT_KEY_ID:?APT_KEY_ID is required}"
repo=$(cd "$1" && pwd)
shift
keep=2

for deb in "$@"; do
    pkg=$(dpkg-deb --field "$deb" Package)
    dir=$repo/pool/main/$(printf '%s' "$pkg" | cut -c1)/$pkg
    mkdir -p "$dir"
    cp "$deb" "$dir/"
    # Oldest first; everything but the newest $keep goes.
    find "$dir" -name '*.deb' | sort -V |
        awk -v keep="$keep" '{ f[NR] = $0 } END { for (i = 1; i <= NR - keep; i++) print f[i] }' |
        while read -r old; do rm -f "$old"; done
done

cd "$repo"
bin=dists/stable/main/binary-amd64
mkdir -p "$bin"
apt-ftparchive packages pool >"$bin/Packages"
gzip -9nkf "$bin/Packages"
apt-ftparchive \
    -o APT::FTPArchive::Release::Origin=koeves \
    -o APT::FTPArchive::Release::Label=koeves \
    -o APT::FTPArchive::Release::Suite=stable \
    -o APT::FTPArchive::Release::Codename=stable \
    -o APT::FTPArchive::Release::Architectures=amd64 \
    -o APT::FTPArchive::Release::Components=main \
    release dists/stable >dists/stable/Release
gpg --batch --yes --local-user "$APT_KEY_ID" --armor --detach-sign --output dists/stable/Release.gpg dists/stable/Release
gpg --batch --yes --local-user "$APT_KEY_ID" --clearsign --output dists/stable/InRelease dists/stable/Release
gpg --batch --armor --export "$APT_KEY_ID" >key.asc
touch .nojekyll
