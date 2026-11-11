#!/usr/bin/env bash
# CHANGELOG.md with tag $1's section (git-cliff, cliff.toml) above the
# previous ones.
set -euo pipefail
tag=$1
dir=$(dirname "$0")
section=$(mktemp)
{
  printf '## [%s] - %s\n\n' "${tag#v}" "$(git log -1 --format=%cs "refs/tags/$tag" 2>/dev/null || date -u +%F)"
  "$dir/notes.sh" "$tag"
  printf '\n'
} > "$section"
awk -v section="$section" '!done && /^## / { while ((getline l < section) > 0) print l; done = 1 } { print }' CHANGELOG.md > CHANGELOG.md.new
mv CHANGELOG.md.new CHANGELOG.md
rm -f "$section"
