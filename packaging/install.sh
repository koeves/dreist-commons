#!/bin/sh
# Install the app next to this script (standalone app, VST3, CLAP, Audio
# Unit on macOS) on Linux
# or macOS, or remove it. packaging/install.ps1 does the same on Windows.
#
#   ./install.sh                         install for this user
#   sudo ./install.sh --system           install for all users
#   ./install.sh --uninstall             remove an install for this user
#   ./install.sh --uninstall --purge     also remove this user's diagnostics logs
#   sudo ./install.sh --uninstall --system   remove a system-wide install
#   ./install.sh --app <Name> --uninstall    remove without the files at hand
#   (--user, the default here, is accepted as on Windows)
#
# Next to this script, each part installed when present:
#   Linux: <App>.vst3, <App>-CLAP/ (<App>.clap + Resources/),
#          <App>-App/ (<App>, Resources/, <app>.desktop, icons/)
#   macOS: <App>.vst3, <App>.clap, <App>.component, <App>.app (Apple
#          Silicon, Intel or universal; a build for the other kind of Mac
#          is refused)
#
# The install locations are listed in packaging/README.md; settings and
# content are left alone. DESTDIR=<dir> stages the same layout under <dir>
# (for packages and tests): no root needed, links and launchers point to the
# final locations.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
scope=user
action=install
purge=0
app=
args="$*"

usage() {
    awk 'NR > 1 && /^#/ { sub(/^# ?/, ""); print; next } NR > 1 { exit }' "$0"
}

while [ $# -gt 0 ]; do
    case "$1" in
        --uninstall) action=uninstall ;;
        --purge) purge=1 ;;
        --system) scope=system ;;
        --user) scope=user ;;
        --app)
            [ $# -gt 1 ] || { usage >&2; exit 2; }
            app=$2
            shift
            ;;
        -h | --help) usage; exit 0 ;;
        *) usage >&2; exit 2 ;;
    esac
    shift
done

# The app's name comes from the parts next to this script.
if [ -z "$app" ]; then
    for p in "$here"/*.vst3 "$here"/*.clap "$here"/*.component "$here"/*.app "$here"/*-CLAP "$here"/*-App; do
        [ -e "$p" ] || continue
        n=$(basename "$p")
        n=${n%.vst3}; n=${n%.clap}; n=${n%.component}; n=${n%.app}; n=${n%-CLAP}; n=${n%-App}
        if [ -z "$app" ]; then
            app=$n
        elif [ "$app" != "$n" ]; then
            echo "More than one app next to this script ($app, $n), nothing changed" >&2
            exit 2
        fi
    done
fi
if [ -z "$app" ]; then
    echo "Nothing to install next to this script ($here); --app <Name> names the app to remove" >&2
    exit 2
fi
bin=$(printf '%s' "$app" | tr '[:upper:]' '[:lower:]')

destdir=${DESTDIR:-}
if [ -z "$destdir" ] && [ "$(id -u)" = 0 ] && [ "$scope" = user ]; then
    echo "Run without sudo to install for your user, or add --system for all users" >&2
    exit 2
fi
if [ -z "$destdir" ] && [ "$(id -u)" != 0 ] && [ "$scope" = system ]; then
    echo "--system needs root: sudo $0 $args" >&2
    exit 2
fi

# Install locations (packaging/README.md lists the same).
os=$(uname -s)
case "$os:$scope" in
    Linux:user)
        share=${XDG_DATA_HOME:-$HOME/.local/share}
        vst3_dir=$HOME/.vst3
        clap_dir=$HOME/.clap/$app
        app_dir=$share/$app
        bin_dir=$HOME/.local/bin
        desktop_dir=$share/applications
        icon_dir=$share/icons/hicolor
        settings=${XDG_CONFIG_HOME:-$HOME/.config}/$app
        ;;
    Linux:system)
        vst3_dir=/usr/local/lib/vst3
        clap_dir=/usr/lib/clap/$app
        app_dir=/opt/$app
        bin_dir=/usr/local/bin
        desktop_dir=/usr/local/share/applications
        icon_dir=/usr/local/share/icons/hicolor
        # shellcheck disable=SC2088 # shown as is: every user's own folder
        settings="~/.config/$app"
        ;;
    Darwin:user)
        vst3_dir=$HOME/Library/Audio/Plug-Ins/VST3
        clap_dir=$HOME/Library/Audio/Plug-Ins/CLAP
        au_dir=$HOME/Library/Audio/Plug-Ins/Components
        # /Applications when this user may write there (admins), else ~/Applications.
        if [ -w "$destdir/Applications" ]; then app_dir=/Applications; else app_dir=$HOME/Applications; fi
        settings="$HOME/Library/Application Support/$app"
        ;;
    Darwin:system)
        vst3_dir=/Library/Audio/Plug-Ins/VST3
        clap_dir=/Library/Audio/Plug-Ins/CLAP
        au_dir=/Library/Audio/Plug-Ins/Components
        app_dir=/Applications
        # shellcheck disable=SC2088 # shown as is: every user's own folder
        settings="~/Library/Application Support/$app"
        ;;
    *)
        echo "This script installs on Linux and macOS; on Windows use install.ps1" >&2
        exit 2
        ;;
esac
sizes="48 64 128 256 512"

# The diagnostics logs (dreist_engine::diag), per user: under sudo, the
# user's who ran it.
user_home=$HOME
if [ -n "${SUDO_USER:-}" ] && [ "$(id -u)" = 0 ]; then
    user_home=$(eval echo "~$SUDO_USER")
fi
if [ "$os" = Linux ]; then
    state=${XDG_STATE_HOME:-$user_home/.local/state}
    case "$state" in /*) ;; *) state=$user_home/.local/state ;; esac
    logs=$state/$app
else
    logs=$user_home/Library/Logs/$app
fi

# Where the app runs from (links and launchers), and where files are written.
app_final=${app_dir:-}
bin_final=${bin_dir:-}
if [ -n "$destdir" ]; then
    vst3_dir=$destdir$vst3_dir
    clap_dir=$destdir$clap_dir
    [ "$os" = Darwin ] && au_dir=$destdir$au_dir
    app_dir=$destdir$app_dir
    [ "$os" = Linux ] && {
        bin_dir=$destdir$bin_dir
        desktop_dir=$destdir$desktop_dir
        icon_dir=$destdir$icon_dir
    }
fi

# Paths in messages, with the home folder as ~.
show() {
    case "$1" in
        "$HOME"/*) printf '~%s' "${1#"$HOME"}" ;;
        *) printf '%s' "$1" ;;
    esac
}

refresh() {
    [ -z "$destdir" ] || return 0
    # macOS: hosts see a changed Audio Unit once the component registry
    # restarts (it does so on its own when next needed).
    if [ "$os" = Darwin ]; then
        killall -9 AudioComponentRegistrar >/dev/null 2>&1 || true
        return 0
    fi
    if command -v update-desktop-database >/dev/null 2>&1; then
        update-desktop-database -q "$desktop_dir" 2>/dev/null || true
    fi
    if command -v gtk-update-icon-cache >/dev/null 2>&1; then
        gtk-update-icon-cache -q -f -t "$icon_dir" 2>/dev/null || true
    fi
}

remove() {
    if [ "$os" = Linux ]; then
        rm -rf "$vst3_dir/$app.vst3" "$clap_dir" "$app_dir"
        rm -f "$bin_dir/$bin" "$desktop_dir/$bin.desktop"
        for s in $sizes; do
            rm -f "$icon_dir/${s}x$s/apps/$bin.png"
        done
    else
        rm -rf "$vst3_dir/$app.vst3" "$clap_dir/$app.clap" "$au_dir/$app.component" "$app_dir/$app.app"
    fi
}

if [ "$action" = uninstall ]; then
    remove
    refresh
    if [ "$purge" = 1 ]; then
        rm -rf "$destdir$logs"
        echo "$app removed with its logs (settings in $(show "$settings") and content kept)"
    else
        echo "$app removed (settings in $(show "$settings"), logs in $(show "$logs") and content kept)"
    fi
    exit 0
fi

# The parts for this system, each checked to be a complete build before
# anything installed is touched.
if [ "$os" = Linux ]; then
    vst3=$here/$app.vst3
    clap=$here/$app-CLAP
    appsrc=$here/$app-App
    magic=7f454c46
    vst3_bin=
    for f in "$vst3"/Contents/*-linux/"$app.so"; do
        [ -e "$f" ] && vst3_bin=$f && break
    done
    clap_bin=$clap/$app.clap
    app_bin=$appsrc/$app
else
    vst3=$here/$app.vst3
    clap=$here/$app.clap
    appsrc=$here/$app.app
    magic="cffaedfe cafebabe"
    vst3_bin=$vst3/Contents/MacOS/$app
    clap_bin=$clap/Contents/MacOS/$app
    au=$here/$app.component
    au_bin=$au/Contents/MacOS/$app
    app_bin=$appsrc/Contents/MacOS/$app
fi
check_build() {
    head=$(od -An -tx1 -N4 "$1" 2>/dev/null | tr -d ' \n' || true)
    for m in $magic; do
        [ "$head" = "$m" ] && return 0
    done
    echo "Not a complete build, nothing changed: $1" >&2
    exit 1
}
# macOS: a build for one architecture (the Apple Silicon or Intel archive)
# must be this Mac's; a universal build fits both. Under Rosetta the Mac is
# still Apple Silicon.
if [ "$os" = Darwin ]; then
    machine=$(uname -m)
    [ "$(sysctl -n sysctl.proc_translated 2>/dev/null || true)" = 1 ] && machine=arm64
fi
mac_name() { if [ "$1" = arm64 ]; then echo "Apple Silicon"; else echo Intel; fi; }
check_arch() {
    [ "$os" = Darwin ] || return 0
    case $(od -An -tx1 -N8 "$1" 2>/dev/null | tr -d ' \n' || true) in
        cffaedfe0c000001) arch=arm64 ;;
        cffaedfe07000001) arch=x86_64 ;;
        *) return 0 ;;
    esac
    [ "$arch" = "$machine" ] && return 0
    want=$(mac_name "$machine" | tr -d ' ')
    echo "This build is for $(mac_name "$arch") Macs, this Mac is $(mac_name "$machine"): use the $app-<version>-macOS-$want archive; nothing changed" >&2
    exit 1
}
found=0
[ -d "$vst3" ] && { check_build "${vst3_bin:-$vst3/Contents}"; found=1; }
[ -d "$clap" ] && { check_build "$clap_bin"; found=1; }
[ -n "${au:-}" ] && [ -d "$au" ] && { check_build "$au_bin"; found=1; }
[ -d "$appsrc" ] && { check_build "$app_bin"; found=1; }
for f in "$vst3_bin" "$clap_bin" "${au_bin:-}" "$app_bin"; do
    [ -f "$f" ] && check_arch "$f"
done
if [ "$found" = 0 ]; then
    echo "Nothing to install for $os next to this script ($here)" >&2
    exit 2
fi

# Copy next to the destination, then swap it in, so an interrupted copy
# leaves no half install.
place() {
    mkdir -p "$(dirname "$2")"
    rm -rf "$2.new"
    cp -R "$1" "$2.new"
    rm -rf "$2"
    mv "$2.new" "$2"
}

# A reinstall replaces everything, so files of an older version do not stay.
remove
if [ -d "$vst3" ]; then
    place "$vst3" "$vst3_dir/$app.vst3"
    echo "VST3: $(show "$vst3_dir/$app.vst3")"
fi
if [ -d "$clap" ]; then
    if [ "$os" = Linux ]; then dest=$clap_dir; else dest=$clap_dir/$app.clap; fi
    place "$clap" "$dest"
    echo "CLAP: $(show "$dest")"
fi
if [ -n "${au:-}" ] && [ -d "$au" ]; then
    place "$au" "$au_dir/$app.component"
    echo "Audio Unit: $(show "$au_dir/$app.component")"
fi
if [ -d "$appsrc" ]; then
    if [ "$os" = Linux ]; then
        place "$appsrc" "$app_dir"
        mkdir -p "$bin_dir" "$desktop_dir"
        ln -sf "$app_final/$app" "$bin_dir/$bin"
        # Absolute paths: ~/.local/bin is only on PATH if it existed at login,
        # and launchers hide entries whose program they cannot find.
        awk -v exe="$app_final/$app" -v icon="$bin" '
            /^Exec=/ { print "TryExec=" exe; print "Exec=" exe; next }
            /^TryExec=/ { next }
            /^Icon=/ { print "Icon=" icon; next }
            { print }' "$app_dir/$bin.desktop" >"$desktop_dir/$bin.desktop"
        for s in $sizes; do
            if [ -f "$app_dir/icons/$s/$bin.png" ]; then
                mkdir -p "$icon_dir/${s}x$s/apps"
                cp "$app_dir/icons/$s/$bin.png" "$icon_dir/${s}x$s/apps/$bin.png"
            fi
        done
        echo "App: $(show "$app_dir") (launcher \"$app\", command $(show "$bin_final/$bin"))"
    else
        place "$appsrc" "$app_dir/$app.app"
        echo "App: $(show "$app_dir/$app.app")"
    fi
fi
refresh
echo "$app installed (settings in $(show "$settings"))"
