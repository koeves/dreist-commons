#!/usr/bin/env bash
# The release notes of tag $1 (its changelog section without the heading),
# from the commits since the previous tag (git-cliff, cliff.toml). Works
# before the tag exists (the unreleased commits) and on its commit after.
set -euo pipefail
tag=$1
if [ "$(git rev-list -n 1 "refs/tags/$tag" 2>/dev/null)" = "$(git rev-parse HEAD)" ]; then
  git cliff --current --strip all
else
  git cliff --unreleased --tag "$tag" --strip all
fi | sed '1{/^## /d;}' | sed '/./,$!d'
