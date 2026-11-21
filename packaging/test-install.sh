#!/bin/sh
# Check packaging/install.sh on this system (Linux or macOS) with a fake app,
# in a scratch home folder and DESTDIR, so nothing real is touched:
# install, reinstall over an older version, a refused incomplete build, a
# refused build for the other kind of Mac, --system staging, uninstall and
# --purge.
#
#   packaging/test-install.sh
# shellcheck disable=SC2015 # ok and bad always succeed
set -eu

script=$(cd "$(dirname "$0")" && pwd)/install.sh
work=$(mktemp -d "${TMPDIR:-/tmp}/install-test.XXXXXX")
trap 'rm -rf "$work"' EXIT
os=$(uname -s)
app=Fake
fails=0

ok() { echo "ok   $1"; }
bad() { echo "FAIL $1"; fails=$((fails + 1)); }
has() { if [ -e "$2" ] || [ -L "$2" ]; then ok "$1"; else bad "$1 ($2 missing)"; fi; }
gone() { if [ -e "$2" ] || [ -L "$2" ]; then bad "$1 ($2 left)"; else ok "$1"; fi; }

# A binary with the system's executable header.
binary() {
    mkdir -p "$(dirname "$1")"
    if [ "$os" = Linux ]; then printf '\177ELF rest' >"$1"; else printf '\317\372\355\376 rest' >"$1"; fi
}

# The files a release archive holds, next to a copy of install.sh.
dist=$work/dist
mkdir -p "$dist"
cp "$script" "$dist/install.sh"
if [ "$os" = Linux ]; then
    binary "$dist/$app.vst3/Contents/x86_64-linux/$app.so"
    binary "$dist/$app-CLAP/$app.clap"
    mkdir -p "$dist/$app-CLAP/Resources"
    binary "$dist/$app-App/$app"
    mkdir -p "$dist/$app-App/Resources" "$dist/$app-App/icons/48"
    printf 'png' >"$dist/$app-App/icons/48/fake.png"
    printf '[Desktop Entry]\nName=Fake\nExec=fake\nIcon=fake\n' >"$dist/$app-App/fake.desktop"
else
    binary "$dist/$app.vst3/Contents/MacOS/$app"
    binary "$dist/$app.clap/Contents/MacOS/$app"
    binary "$dist/$app.app/Contents/MacOS/$app"
fi

run() { HOME=$work/home XDG_DATA_HOME='' XDG_CONFIG_HOME='' DESTDIR=$work/root sh "$dist/install.sh" "$@"; }
mkdir -p "$work/home" "$work/root/Applications"
H=$work/home
R=$work/root

if [ "$os" = Linux ]; then
    vst3=$R$H/.vst3/$app.vst3
    clap=$R$H/.clap/$app/$app.clap
    appd=$R$H/.local/share/$app
    link=$R$H/.local/bin/fake
    desk=$R$H/.local/share/applications/fake.desktop
else
    vst3=$R$H/Library/Audio/Plug-Ins/VST3/$app.vst3
    clap=$R$H/Library/Audio/Plug-Ins/CLAP/$app.clap
    appd=$R/Applications/$app.app
fi

echo "== install"
run >"$work/out" 2>&1 || { cat "$work/out"; bad "install exits 0"; }
has "VST3" "$vst3"
has "CLAP" "$clap"
has "app" "$appd"
if [ "$os" = Linux ]; then
    has "command link" "$link"
    [ "$(readlink "$link")" = "$H/.local/share/$app/$app" ] && ok "link points to the final place" || bad "link target $(readlink "$link")"
    grep -q "^Exec=$H/.local/share/$app/$app\$" "$desk" && ok "launcher has the absolute path" || bad "launcher Exec"
    has "icon" "$R$H/.local/share/icons/hicolor/48x48/apps/fake.png"
fi
grep -q "^VST3: " "$work/out" && grep -q "installed" "$work/out" && ok "messages" || bad "messages: $(cat "$work/out")"

echo "== reinstall drops files of an older version"
stale=$vst3/Contents/old-file
touch "$stale"
run >/dev/null 2>&1 || bad "reinstall exits 0"
gone "old file" "$stale"
gone "no .new left" "$vst3.new"

echo "== an incomplete build changes nothing"
cp -R "$dist" "$work/broken"
if [ "$os" = Linux ]; then : >"$work/broken/$app-App/$app"; else : >"$work/broken/$app.app/Contents/MacOS/$app"; fi
touch "$vst3/marker"
if HOME=$H XDG_DATA_HOME='' XDG_CONFIG_HOME='' DESTDIR=$R sh "$work/broken/install.sh" >/dev/null 2>"$work/err"; then bad "refuses an empty binary"; else ok "refuses an empty binary"; fi
grep -q "Not a complete build" "$work/err" && ok "says why" || bad "error: $(cat "$work/err")"
has "install untouched" "$vst3/marker"

if [ "$os" = Darwin ]; then
    echo "== a build for the other kind of Mac changes nothing"
    machine=$(uname -m)
    [ "$(sysctl -n sysctl.proc_translated 2>/dev/null || true)" = 1 ] && machine=arm64
    other=x86_64
    [ "$machine" = arm64 ] || other=arm64
    # A thin Mach-O header: 64-bit magic, then the CPU type (arm64 0c, x86_64 07).
    macho() {
        if [ "$1" = arm64 ]; then printf '\317\372\355\376\014\000\000\001 rest' >"$2"; else printf '\317\372\355\376\007\000\000\001 rest' >"$2"; fi
    }
    cp -R "$dist" "$work/other"
    macho "$other" "$work/other/$app.clap/Contents/MacOS/$app"
    if HOME=$H DESTDIR=$R sh "$work/other/install.sh" >/dev/null 2>"$work/err"; then bad "refuses the other architecture"; else ok "refuses the other architecture"; fi
    grep -q "use the $app-<version>-macOS-" "$work/err" && ok "names the right archive" || bad "error: $(cat "$work/err")"
    has "install untouched" "$vst3/marker"
    cp -R "$dist" "$work/own"
    macho "$machine" "$work/own/$app.clap/Contents/MacOS/$app"
    printf '\312\376\272\276 rest' >"$work/own/$app.app/Contents/MacOS/$app"
    HOME=$H DESTDIR=$R sh "$work/own/install.sh" >/dev/null 2>&1 && ok "installs this Mac's and universal builds" || bad "this Mac's build refused"
fi

echo "== wrong options"
if run --bogus >/dev/null 2>&1; then bad "unknown option fails"; else ok "unknown option fails"; fi
run --help | grep -q -- "--uninstall" && ok "help lists the options" || bad "help"

echo "== uninstall"
mkdir -p "$R$H/.config/$app" "$R$H/Library/Application Support/$app"
run --uninstall >"$work/out" 2>&1 || bad "uninstall exits 0"
gone "VST3" "$vst3"
gone "CLAP" "$clap"
gone "app" "$appd"
[ "$os" = Linux ] && gone "command link" "$link" && gone "launcher" "$desk"
grep -q "removed" "$work/out" && ok "message" || bad "message: $(cat "$work/out")"

echo "== uninstall --purge also removes the logs"
run >/dev/null 2>&1
if [ "$os" = Linux ]; then logs=$R$H/.local/state/$app; else logs=$R$H/Library/Logs/$app; fi
mkdir -p "$logs"
: >"$logs/fake.log"
run --uninstall >"$work/out" 2>&1
has "logs kept without --purge" "$logs/fake.log"
grep -q "logs in" "$work/out" && ok "says where the logs stay" || bad "message: $(cat "$work/out")"
run >/dev/null 2>&1
run --uninstall --purge >"$work/out" 2>&1 || bad "--purge exits 0"
gone "logs" "$logs"
gone "VST3" "$vst3"
grep -q "with its logs" "$work/out" && ok "message" || bad "message: $(cat "$work/out")"

echo "== uninstall by name, with nothing next to the script"
run >/dev/null 2>&1
mkdir -p "$work/bare"
cp "$script" "$work/bare/install.sh"
HOME=$H XDG_DATA_HOME='' XDG_CONFIG_HOME='' DESTDIR=$R sh "$work/bare/install.sh" --app "$app" --uninstall >/dev/null 2>&1 || bad "--app uninstall exits 0"
gone "VST3" "$vst3"

echo "== system-wide staging"
run --system >/dev/null 2>&1 || bad "--system exits 0"
if [ "$os" = Linux ]; then
    has "VST3" "$R/usr/local/lib/vst3/$app.vst3"
    has "CLAP" "$R/usr/lib/clap/$app/$app.clap"
    has "app" "$R/opt/$app/$app"
    [ "$(readlink "$R/usr/local/bin/fake")" = "/opt/$app/$app" ] && ok "link points to /opt" || bad "system link"
else
    has "VST3" "$R/Library/Audio/Plug-Ins/VST3/$app.vst3"
    has "CLAP" "$R/Library/Audio/Plug-Ins/CLAP/$app.clap"
    has "app" "$R/Applications/$app.app"
fi
run --system --uninstall >/dev/null 2>&1 || bad "--system --uninstall exits 0"
if [ "$os" = Linux ]; then gone "VST3" "$R/usr/local/lib/vst3/$app.vst3"; else gone "VST3" "$R/Library/Audio/Plug-Ins/VST3/$app.vst3"; fi

if [ "$fails" = 0 ]; then echo "all passed"; else echo "$fails failed"; exit 1; fi
