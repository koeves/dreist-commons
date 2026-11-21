#!/usr/bin/env bash
# Close the issues a release fixes. Pull requests merge into dev, where
# GitHub ignores closing keywords, so after a release this closes every open
# issue of this repository that a pull request in the release names with
# "Closes", "Fixes" or "Resolves" (`#N` or `owner/repo#N`), with a comment
# naming the release.
#
# Usage: close-issues.sh vX.Y.Z (run in a full checkout at the release tag).
# `GH` replaces gh (tests).
set -euo pipefail

gh=${GH:-gh}
tag=$1
repo=${GITHUB_REPOSITORY:?}
prev=$(git tag --list 'v[0-9]*' --sort=-v:refname | grep -vxF "$tag" | head -n 1 || true)
range=${prev:+$prev..}$tag

# The pull requests whose commits are in the release.
prs=$(git rev-list --no-merges "$range" | while read -r sha; do
  "$gh" api "repos/$repo/commits/$sha/pulls" --jq '.[].number'
done | sort -un)

issues=$(for pr in $prs; do
  "$gh" api "repos/$repo/pulls/$pr" --jq '.body // ""' |
    { grep -oiE '\b(close[sd]?|fix(e[sd])?|resolve[sd]?):?[[:space:]]+([A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+)?#[0-9]+' || true; } |
    sed -E 's/^[^[:space:]]+[[:space:]]+//' |
    while read -r ref; do
      case "$ref" in
        "#"*) echo "${ref#\#}" ;;
        "$repo#"*) echo "${ref#*#}" ;;
      esac
    done
done | sort -un)

for n in $issues; do
  state=$("$gh" api "repos/$repo/issues/$n" --jq 'if .pull_request then "pr" else .state end' 2>/dev/null || echo missing)
  if [ "$state" = open ]; then
    "$gh" api -X POST "repos/$repo/issues/$n/comments" -f body="Released in $tag." >/dev/null
    "$gh" api -X PATCH "repos/$repo/issues/$n" -f state=closed -f state_reason=completed >/dev/null
    echo "Closed #$n (released in $tag)."
  fi
done
