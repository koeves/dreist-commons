#!/usr/bin/env bash
# The next release version, from the commits since the last `v*` tag:
# git-cliff's `--bumped-version` with cliff.toml's [bump] rules (on 0.x:
# fix -> patch, feat or breaking -> minor). A `Release-As: X.Y.Z` line in the
# merged pull request's body (the file $PR_BODY_FILE) or, failing that, in the
# newest commit message since the tag that has one, sets the version instead.
#
# Releasable commits: feat, fix, perf, refactor, revert, breaking changes and
# dependency bumps (`build(deps)`, `chore(deps)`, `build: dreist-commons ...`).
# Without one (and without Release-As) nothing is released.
#
# Writes `tag=vX.Y.Z` and `version=X.Y.Z` to $GITHUB_OUTPUT (stdout outside
# Actions), nothing when there is no release; the reason goes to stderr and
# to the job summary.
set -euo pipefail

out=${GITHUB_OUTPUT:-/dev/stdout}
summary=${GITHUB_STEP_SUMMARY:-/dev/stderr}
say() { printf '%s\n' "$*" >&2; printf '%s\n\n' "$*" >> "$summary"; }

prev=$(git describe --tags --abbrev=0 --match 'v[0-9]*' HEAD 2>/dev/null || true)
range=${prev:+$prev..}HEAD

release_as() { tr -d '\r' | sed -n -E 's/^[[:space:]]*Release-As:[[:space:]]*v?([0-9]+\.[0-9]+\.[0-9]+)[[:space:]]*$/\1/Ip'; }
override=""
if [ -n "${PR_BODY_FILE:-}" ] && [ -f "$PR_BODY_FILE" ]; then
  override=$(release_as < "$PR_BODY_FILE" | tail -n 1)
fi
if [ -z "$override" ]; then
  while IFS= read -r sha; do
    override=$(git log -1 --format=%B "$sha" | release_as | tail -n 1)
    [ -n "$override" ] && break
  done < <(git rev-list --no-merges "$range")
fi

if [ -n "$override" ]; then
  version=$override
  say "Release-As: $version (since ${prev:-the first commit})."
else
  releasable='^(feat|fix|perf|refactor|revert)(\([^)]*\))?!?: |^[a-z]+(\([^)]*\))?!: |^(build|chore)\(deps\): |^build(\([^)]*\))?: dreist-commons '
  hits=$(git log --no-merges --format=%s "$range" | grep -E "$releasable" || true)
  breaking=$(git log --no-merges --format=%B "$range" | grep -E '^BREAKING[ -]CHANGE: ' || true)
  if [ -z "$hits" ] && [ -z "$breaking" ]; then
    say "Nothing to release: no feat, fix, perf, refactor, revert, breaking or dependency commit since ${prev:-the first commit} (add \`Release-As: X.Y.Z\` to release anyway)."
    git log --no-merges --format='- %s' "$range" >> "$summary"
    exit 0
  fi
  version=$(git cliff --bumped-version 2>/dev/null)
  version=${version#v}
fi

if ! [[ $version =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  say "Not a version: \"$version\"."
  exit 1
fi
if git rev-parse -q --verify "refs/tags/v$version" > /dev/null; then
  say "Tag v$version exists already (last release: ${prev:-none})."
  exit 1
fi
say "Releasing v$version (previous: ${prev:-none})."
printf 'tag=v%s\nversion=%s\n' "$version" "$version" >> "$out"
