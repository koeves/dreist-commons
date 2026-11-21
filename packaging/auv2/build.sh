#!/usr/bin/env bash
# Build an Audio Unit (AUv2, .component) from an app's CLAP on macOS, with
# clap-wrapper and Apple's AudioUnitSDK at the pinned versions below. The
# .component carries the .clap in Contents/PlugIns and loads it from there.
#
#   packaging/auv2/build.sh --clap <App>.clap --subtype <code> --bundle-id <id> \
#       [--manufacturer Drst] [--manufacturer-name Dreist] [--version 1.2.3] \
#       [--arch arm64|x86_64|universal] [--out <dir>] [--work <dir>] [--sign <identity>]
#
#   --clap               the built CLAP bundle to wrap
#   --subtype            the AU subtype, four characters (one per app)
#   --bundle-id          the .component's CFBundleIdentifier
#   --manufacturer       the AU manufacturer code (default Drst)
#   --manufacturer-name  the name Logic groups the AU under (default Dreist)
#   --version            the bundle version (default the CLAP's)
#   --arch               default the CLAP's architectures
#   --out                where <App>.component goes (default next to the CLAP)
#   --work               sources and build files (default target/auv2)
#   --sign               a "Developer ID Application: ..." identity: hardened
#                        runtime and a timestamp (default ad-hoc)
#   --fetch              only fetch the sources into the --work folder
#
# The AU type is aumu (instrument). Needs Xcode's command line tools, CMake
# and git. Sources come from GitHub on the first run and are checked against
# the pinned commits.
set -euo pipefail

# Pinned sources: name, repository, tag, commit.
CLAP_WRAPPER=(clap-wrapper https://github.com/free-audio/clap-wrapper.git v0.18.0 c3a2531a5e66572b4246fc64271e2103d0de37bb)
AUDIOUNIT_SDK=(AudioUnitSDK https://github.com/apple/AudioUnitSDK.git AudioUnitSDK-1.1.0 53a9a2008aae7fb1b0a9f093dd523b9b12f6c0d9)
CLAP_SDK=(clap https://github.com/free-audio/clap.git 1.2.10 195b42a004144fab0b3cf95e9c067187d15365b7)

usage() { sed -n '2,24p' "$0" | sed 's/^# \{0,1\}//'; }
here=$(cd "$(dirname "$0")" && pwd)
clap=
subtype=
bundle_id=
manufacturer=Drst
manufacturer_name=Dreist
version=
arch=
out=
work=target/auv2
sign=
fetch_only=0
while [ $# -gt 0 ]; do
    case "$1" in
        -h | --help) usage; exit 0 ;;
        --fetch) fetch_only=1; shift; continue ;;
    esac
    [ $# -gt 1 ] || { usage >&2; exit 2; }
    case "$1" in
        --clap) clap=$2 ;;
        --subtype) subtype=$2 ;;
        --bundle-id) bundle_id=$2 ;;
        --manufacturer) manufacturer=$2 ;;
        --manufacturer-name) manufacturer_name=$2 ;;
        --version) version=$2 ;;
        --arch) arch=$2 ;;
        --out) out=$2 ;;
        --work) work=$2 ;;
        --sign) sign=$2 ;;
        *) usage >&2; exit 2 ;;
    esac
    shift 2
done
[ "$(uname -s)" = Darwin ] || { echo "Audio Units build on macOS only" >&2; exit 2; }
mkdir -p "$work"
work=$(cd "$work" && pwd)

# A source at its pinned commit: a shallow clone of the tag, checked.
fetch() {
    local dir=$work/src/$1 got
    if [ "$(git -C "$dir" rev-parse HEAD 2>/dev/null || true)" != "$4" ]; then
        rm -rf "$dir"
        git -c advice.detachedHead=false clone -q --depth 1 --branch "$3" "$2" "$dir"
        got=$(git -C "$dir" rev-parse HEAD)
        if [ "$got" != "$4" ]; then
            echo "$1 $3 is $got, pinned $4: nothing built" >&2
            exit 1
        fi
    fi
}
fetch "${CLAP_WRAPPER[@]}"
fetch "${AUDIOUNIT_SDK[@]}"
fetch "${CLAP_SDK[@]}"
[ "$fetch_only" = 0 ] || exit 0

if [ -z "$clap" ] || [ -z "$subtype" ] || [ -z "$bundle_id" ]; then
    usage >&2
    exit 2
fi
[ -d "$clap" ] || { echo "No CLAP bundle at $clap" >&2; exit 1; }
for c in "$subtype" "$manufacturer"; do
    [ "${#c}" = 4 ] || { echo "AU codes are four characters: $c" >&2; exit 2; }
done
# Apple reserves the all-lower-case manufacturer codes.
if [ "$manufacturer" = "$(printf '%s' "$manufacturer" | tr '[:upper:]' '[:lower:]')" ]; then
    echo "The manufacturer code needs an upper-case letter: $manufacturer" >&2
    exit 2
fi

clap=$(cd "$(dirname "$clap")" && pwd)/$(basename "$clap")
name=$(basename "$clap" .clap)
out=${out:-$(dirname "$clap")}
mkdir -p "$out"
out=$(cd "$out" && pwd)
[ -n "$version" ] || version=$(plutil -extract CFBundleShortVersionString raw "$clap/Contents/Info.plist")
if [ -z "$arch" ]; then
    archs=$(lipo -archs "$clap/Contents/MacOS/$name")
else
    case "$arch" in
        arm64 | x86_64) archs=$arch ;;
        universal) archs="arm64 x86_64" ;;
        *) echo "unknown --arch: $arch (arm64, x86_64 or universal)" >&2; exit 2 ;;
    esac
fi


echo "==> Audio Unit $name ($manufacturer/$subtype, $version, ${archs// /+})"
build=$work/build-$name
log=$work/build-$name.log
if ! cmake -S "$here" -B "$build" -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_OSX_ARCHITECTURES="${archs// /;}" \
    -DCMAKE_OSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-11.0}" \
    -DCLAP_WRAPPER_ROOT="$work/src/clap-wrapper" \
    -DAUDIOUNIT_SDK_ROOT="$work/src/AudioUnitSDK" \
    -DCLAP_SDK_ROOT="$work/src/clap" \
    -DAU_NAME="$name" -DAU_CLAP="$clap" -DAU_VERSION="$version" -DAU_BUNDLE_ID="$bundle_id" \
    -DAU_MANUFACTURER="$manufacturer" -DAU_MANUFACTURER_NAME="$manufacturer_name" \
    -DAU_SUBTYPE="$subtype" >"$log" 2>&1; then
    cat "$log" >&2
    exit 1
fi
# The wrapper copies the CLAP into the bundle before each build: start clean.
rm -rf "${build:?}/$name.component"
if ! cmake --build "$build" --config Release -j "$(sysctl -n hw.ncpu)" >>"$log" 2>&1; then
    tail -50 "$log" >&2
    exit 1
fi

component=$out/$name.component
rm -rf "$component"
ditto "$build/$name.component" "$component"
# Licence and notices as in the CLAP.
for f in "$clap"/Contents/Resources/LICENSE* "$clap"/Contents/Resources/THIRD_PARTY_NOTICES.md; do
    [ -f "$f" ] || continue
    mkdir -p "$component/Contents/Resources"
    cp "$f" "$component/Contents/Resources/"
done
if [ -n "$sign" ]; then
    echo "==> Signing $component"
    codesign --force --deep --options runtime --timestamp --sign "$sign" "$component"
else
    codesign --force --deep --sign - "$component" >/dev/null 2>&1 || echo "warning: ad-hoc codesign failed for $component" >&2
fi
