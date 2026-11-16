#!/bin/sh
# Check packaging/deb.sh and packaging/apt-publish.sh on Debian / Ubuntu with
# a fake app and a throwaway signing key, all in a scratch folder: the .deb's
# layout and fields, the signed index, pool pruning, and apt reading the repo.
#
#   packaging/test-packages.sh
set -eu

here=$(cd "$(dirname "$0")" && pwd)
work=$(mktemp -d "${TMPDIR:-/tmp}/packages-test.XXXXXX")
trap 'gpgconf --kill all 2>/dev/null || true; rm -rf "$work"' EXIT
app=Fake
fails=0
ok() { echo "ok   $1"; }
bad() { echo "FAIL $1"; fails=$((fails + 1)); }
check() { if eval "$2"; then ok "$1"; else bad "$1"; fi; }

# A Linux release folder (packaging/README.md: archive layout).
dist=$work/$app-v1.0.0-Linux
mkdir -p "$dist/$app.vst3/Contents/x86_64-linux" "$dist/$app-CLAP/Resources" "$dist/$app-App/Resources" "$dist/$app-App/icons/48"
cp "$here/install.sh" "$dist/"
for f in "$app.vst3/Contents/x86_64-linux/$app.so" "$app-CLAP/$app.clap" "$app-App/$app"; do printf '\177ELF rest' >"$dist/$f"; done
printf 'png' >"$dist/$app-App/icons/48/fake.png"
printf '[Desktop Entry]\nName=Fake\nExec=fake\nIcon=fake\n' >"$dist/$app-App/fake.desktop"

echo "== deb"
export DEB_MAINTAINER="Test <test@example.invalid>"
deb=$(sh "$here/deb.sh" "$dist" v1.0.0 "$work/out")
check "named <app>_<version>_amd64.deb" '[ "$(basename "$deb")" = fake_1.0.0_amd64.deb ]'
check "package field" '[ "$(dpkg-deb --field "$deb" Package)" = fake ]'
check "version field" '[ "$(dpkg-deb --field "$deb" Version)" = 1.0.0 ]'
list=$(dpkg-deb --contents "$deb")
for p in ./usr/lib/vst3/Fake.vst3/ ./usr/lib/clap/Fake/Fake.clap ./opt/Fake/Fake ./usr/bin/fake ./usr/share/applications/fake.desktop ./usr/share/icons/hicolor/48x48/apps/fake.png; do
    check "has $p" 'printf "%s\n" "$list" | grep -q " $p"'
done
check "files owned by root" '! printf "%s\n" "$list" | grep -v "root/root" | grep -q .'
check "command links to /opt" 'printf "%s\n" "$list" | grep -q "./usr/bin/fake -> /opt/Fake/Fake"'
dpkg-deb --extract "$deb" "$work/x"
check "launcher runs /opt/Fake/Fake" 'grep -q "^Exec=/opt/Fake/Fake$" "$work/x/usr/share/applications/fake.desktop"'

echo "== apt repository"
export GNUPGHOME=$work/gnupg
mkdir -m 700 "$GNUPGHOME"
gpg --batch --quiet --passphrase '' --quick-generate-key "packaging test" ed25519 sign 1d
APT_KEY_ID=$(gpg --batch --list-keys --with-colons | awk -F: '/^fpr/ { print $10; exit }')
export APT_KEY_ID
repo=$work/repo
mkdir "$repo"
sh "$here/apt-publish.sh" "$repo" "$deb"
for v in 1.0.1 1.0.2; do sh "$here/deb.sh" "$dist" "$v" "$work/out" >/dev/null; done
sh "$here/apt-publish.sh" "$repo" "$work/out/fake_1.0.1_amd64.deb" "$work/out/fake_1.0.2_amd64.deb"
check "keeps the two newest" '[ "$(ls "$repo/pool/main/f/fake")" = "$(printf "fake_1.0.1_amd64.deb\nfake_1.0.2_amd64.deb")" ]'
check "Packages lists 1.0.2" 'grep -q "^Version: 1.0.2$" "$repo/dists/stable/main/binary-amd64/Packages"'
check "Packages.gz" '[ -s "$repo/dists/stable/main/binary-amd64/Packages.gz" ]'
gpg --batch --export "$APT_KEY_ID" >"$work/key.gpg"
check "InRelease signature" 'gpgv --keyring "$work/key.gpg" "$repo/dists/stable/InRelease" 2>/dev/null'
check "Release.gpg signature" 'gpgv --keyring "$work/key.gpg" "$repo/dists/stable/Release.gpg" "$repo/dists/stable/Release" 2>/dev/null'
check "key.asc is the public key" 'grep -q "BEGIN PGP PUBLIC KEY BLOCK" "$repo/key.asc" && ! grep -q PRIVATE "$repo/key.asc"'

# apt itself, as a user would configure it, in its own scratch state.
state=$work/apt
mkdir -p "$state/lists/partial" "$state/cache/archives/partial" "$state/etc/sources.list.d" "$state/etc/preferences.d" "$state/etc/trusted.gpg.d"
cp "$repo/key.asc" "$state/key.asc"
echo "deb [signed-by=$state/key.asc] file:$repo stable main" >"$state/etc/sources.list"
aptopts="-o Dir::State=$state -o Dir::State::Lists=$state/lists -o Dir::Cache=$state/cache -o Dir::Etc=$state/etc -o Dir::Etc::SourceList=$state/etc/sources.list -o Dir::Etc::SourceParts=$state/etc/sources.list.d -o Dir::State::status=/var/lib/dpkg/status -o Debug::NoLocking=1"
# shellcheck disable=SC2086 # aptopts holds several options
check "apt update accepts the signed repo" 'apt-get $aptopts update >"$work/apt.log" 2>&1 && ! grep -q "^W:\|^E:" "$work/apt.log"'
# shellcheck disable=SC2086
check "apt sees fake 1.0.2" 'apt-cache $aptopts policy fake | grep -q "Candidate: 1.0.2"'

if [ "$fails" = 0 ]; then echo "all passed"; else cat "$work/apt.log" 2>/dev/null || true; echo "$fails failed"; exit 1; fi
