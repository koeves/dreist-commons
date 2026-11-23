#!/bin/sh
# Write the Homebrew cask for an app's macOS release archives, one per kind
# of Mac (<App>-v<version>-macOS-AppleSilicon.zip and -macOS-Intel.zip:
# install.sh, <App>.app, <App>.vst3, <App>.clap) to standard output; Homebrew
# picks the one for the Mac. The install locations match packaging/README.md.
#
#   packaging/cask.sh <App> <version> <arm64 sha256> <Intel sha256> <owner/repo> [description]
set -eu

[ $# -ge 5 ] || { sed -n '2,7p' "$0" >&2; exit 2; }
app=$1
version=${2#v}
sha_arm=$3
sha_intel=$4
repo=$5
desc=${6:-"$app instrument: standalone app, VST3 and CLAP"}
token=$(printf '%s' "$app" | tr '[:upper:]' '[:lower:]')
dir="$app-v#{version}-macOS-#{arch}"

cat <<CASK
cask "$token" do
  arch arm: "AppleSilicon", intel: "Intel"

  version "$version"
  sha256 arm:   "$sha_arm",
         intel: "$sha_intel"

  url "https://github.com/$repo/releases/download/v#{version}/$dir.zip"
  name "$app"
  desc "$desc"
  homepage "https://github.com/$repo"

  livecheck do
    url :url
    strategy :github_latest
  end

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
