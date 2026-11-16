#!/bin/sh
# Write the Homebrew cask for an app's macOS release archive
# (<App>-v<version>-macOS.zip: install.sh, <App>.app, <App>.vst3, <App>.clap)
# to standard output. The install locations match packaging/README.md.
#
#   packaging/cask.sh <App> <version> <sha256> <owner/repo> [description]
set -eu

[ $# -ge 4 ] || { sed -n '2,6p' "$0" >&2; exit 2; }
app=$1
version=${2#v}
sha=$3
repo=$4
desc=${5:-"$app instrument: standalone app, VST3 and CLAP"}
token=$(printf '%s' "$app" | tr '[:upper:]' '[:lower:]')
dir="$app-v#{version}-macOS"

cat <<CASK
cask "$token" do
  version "$version"
  sha256 "$sha"

  url "https://github.com/$repo/releases/download/v#{version}/$dir.zip"
  name "$app"
  desc "$desc"
  homepage "https://github.com/$repo"

  livecheck do
    url :url
    strategy :github_latest
  end

  depends_on arch: :arm64
  depends_on macos: ">= :big_sur"

  app "$dir/$app.app"
  vst3_plugin "$dir/$app.vst3"
  artifact "$dir/$app.clap", target: "~/Library/Audio/Plug-Ins/CLAP/$app.clap"

  zap trash: "~/Library/Application Support/$app"

  caveats <<~EOS
    $app is not signed by Apple: open it once with right-click > Open, and
    clear the quarantine flag if a host blocks the plugins:
      xattr -dr com.apple.quarantine ~/Library/Audio/Plug-Ins/VST3/$app.vst3 ~/Library/Audio/Plug-Ins/CLAP/$app.clap
  EOS
end
CASK
