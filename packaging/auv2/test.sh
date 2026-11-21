#!/usr/bin/env bash
# Check the Audio Unit build on macOS: wrap a silent test CLAP with the apps'
# bus layout (packaging/auv2/test/test-clap.c) as a universal .component,
# install it for this user with install.sh, run auval on it (and on its
# Intel half under Rosetta) and remove it.
#
#   packaging/auv2/test.sh [--work <dir>]   (default target/auv2)
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
work=target/auv2
[ "${1:-}" = --work ] && work=$2
mkdir -p "$work"
work=$(cd "$work" && pwd)
dist=$work/test-dist
rm -rf "$dist"
mkdir -p "$dist/AuTest.clap/Contents/MacOS"

# The CLAP headers at build.sh's pinned commit.
"$here/build.sh" --fetch --work "$work"
clap_sdk=$work/src/clap

echo "==> Test CLAP"
clang -bundle -O2 -std=c11 -Wall -Werror -arch arm64 -arch x86_64 \
    -mmacosx-version-min="${MACOSX_DEPLOYMENT_TARGET:-11.0}" \
    -I "$clap_sdk/include" "$here/test/test-clap.c" -o "$dist/AuTest.clap/Contents/MacOS/AuTest"
cat >"$dist/AuTest.clap/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key><string>AuTest</string>
    <key>CFBundleIdentifier</key><string>rs.dreist.auv2-test</string>
    <key>CFBundleName</key><string>AuTest</string>
    <key>CFBundlePackageType</key><string>BNDL</string>
    <key>CFBundleShortVersionString</key><string>1.0.0</string>
    <key>CFBundleVersion</key><string>1.0.0</string>
</dict>
</plist>
PLIST
codesign --force --sign - "$dist/AuTest.clap" >/dev/null 2>&1

"$here/build.sh" --clap "$dist/AuTest.clap" --subtype Atst --bundle-id rs.dreist.auv2-test.component --work "$work"
archs=$(lipo -archs "$dist/AuTest.component/Contents/MacOS/AuTest")
[ "$archs" = "x86_64 arm64" ] || [ "$archs" = "arm64 x86_64" ] || { echo "Not universal: $archs" >&2; exit 1; }
# Only the component next to install.sh: the CLAP stays out of the plug-in folders.
rm -rf "$dist/AuTest.clap"
cp "$here/../install.sh" "$dist/install.sh"

echo "==> auval"
trap 'sh "$dist/install.sh" --app AuTest --uninstall >/dev/null' EXIT
sh "$dist/install.sh"
if ! auval -v aumu Atst Drst >"$work/auval-test.log" 2>&1; then
    tail -40 "$work/auval-test.log" >&2
    echo "auval failed (log: $work/auval-test.log)" >&2
    exit 1
fi
grep -q "Default Bus Count:17" "$work/auval-test.log" || { echo "auval does not see 17 output buses" >&2; exit 1; }
echo "auval passed (17 output buses)"
# The Intel half too, where Rosetta runs it.
if [ "$(uname -m)" = arm64 ] && arch -x86_64 /usr/bin/true 2>/dev/null; then
    if ! arch -x86_64 auval -v aumu Atst Drst >"$work/auval-test-x86_64.log" 2>&1; then
        tail -40 "$work/auval-test-x86_64.log" >&2
        echo "auval (x86_64) failed (log: $work/auval-test-x86_64.log)" >&2
        exit 1
    fi
    echo "auval (x86_64) passed"
fi
