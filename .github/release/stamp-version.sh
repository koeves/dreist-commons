#!/usr/bin/env bash
# Write version $1 (X.Y.Z or vX.Y.Z) into the workspace Cargo.toml (its first
# `version = ` line, [workspace.package]; the crates inherit it, and the
# bundles' Info.plist and the About panel read it) and Cargo.lock.
set -euo pipefail
v=${1#v}
[[ $v =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "not a version: $1" >&2; exit 1; }
V=$v perl -pi -e 'if (!$done && s/^version = "[^"]*"/version = "$ENV{V}"/) { $done = 1 }' Cargo.toml
cargo update --workspace --quiet
