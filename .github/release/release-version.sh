#!/usr/bin/env bash
# The version a release pull request (dev -> main) releases: the one
# prepare.sh wrote into the workspace Cargo.toml, with its CHANGELOG.md
# section. Fails when that version is already tagged (nothing prepared), has
# no changelog section, or when releasable commits came into dev after the
# release was prepared (run prepare.sh again).
#
#   .github/release/release-version.sh            check (release pull request)
#   .github/release/release-version.sh --release  on main: no release when nothing is prepared
#
# Writes `tag=vX.Y.Z` and `version=X.Y.Z` to $GITHUB_OUTPUT (stdout outside
# Actions); the reason goes to stderr and to the job summary.
set -euo pipefail

mode=${1:-check}
out=${GITHUB_OUTPUT:-/dev/stdout}
summary=${GITHUB_STEP_SUMMARY:-/dev/stderr}
say() { printf '%s\n' "$*" >&2; printf '%s\n\n' "$*" >> "$summary"; }
# shellcheck source=.github/release/releasable.sh
. "$(dirname "$0")/releasable.sh"

version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
prev=$(git tag --list 'v[0-9]*' --sort=-v:refname | head -n 1)

if git rev-parse -q --verify "refs/tags/v$version" > /dev/null; then
  if [ "$mode" = --release ]; then
    say "Nothing to release: v$version is tagged already and no release was prepared."
    exit 0
  fi
  say "No release prepared: Cargo.toml has $version, released as v$version. Run .github/release/prepare.sh and merge its pull request into dev first."
  exit 1
fi
if ! grep -q "^## \[$version\]" CHANGELOG.md; then
  say "CHANGELOG.md has no section for $version: run .github/release/prepare.sh."
  exit 1
fi
prepared=$(git log -1 --format=%H --grep="^docs(release): changelog and version for v$version" HEAD || true)
if [ -z "$prepared" ]; then
  say "No commit \"docs(release): changelog and version for v$version\" on this branch: run .github/release/prepare.sh."
  exit 1
fi
late=$(git log --no-merges --format=%s "$prepared..HEAD" | grep -E "$RELEASABLE" || true)
if [ -n "$late" ]; then
  say "Releasable commits came after the release was prepared; run .github/release/prepare.sh again:"
  printf '%s\n' "$late" | sed 's/^/- /' | tee -a "$summary" >&2
  exit 1
fi
say "Releasing v$version (previous: ${prev:-none})."
printf 'tag=v%s\nversion=%s\n' "$version" "$version" >> "$out"
