#!/usr/bin/env bash
# Keep the release pull requests in step with dev (release-pr.yml runs it
# after every push to dev):
#
# - A prepared release on dev (release-version.sh passes with a version):
#   open the release pull request dev -> main, "chore(release): vX.Y.Z".
# - Releasable commits and no prepared release: write release/next from dev
#   with the version and its CHANGELOG.md section (what prepare.sh writes)
#   and open or update its pull request into dev, "chore(release): vX.Y.Z".
#   Approving and merging it is the decision to release.
# - Nothing to release: close a leftover release/next pull request.
#
# Pull requests and pushes made with GITHUB_TOKEN start no workflows, so the
# checks run by workflow_dispatch on the branch; their check runs land on its
# head commit and count as its required checks. `GH` replaces gh (tests).
# Needs git with a writable origin, git-cliff, cargo and gh.
set -euo pipefail

gh=${GH:-gh}
dir=$(cd "$(dirname "$0")" && pwd)
branch=release/next

open_pr() { $gh pr list --head "$1" --base "$2" --state open --json number --jq '.[0].number // empty'; }
checks() { for wf in ci.yml commits.yml codeql.yml; do $gh workflow run "$wf" --ref "$1"; done; }
quiet() { GITHUB_OUTPUT=/dev/stdout GITHUB_STEP_SUMMARY=/dev/null "$@" 2> /dev/null; }

git fetch -q --tags origin dev
git checkout -q --detach origin/dev

# A prepared release on dev: the release pull request into main.
prepared=$(quiet "$dir/release-version.sh" | sed -n 's/^version=//p' || true)
if [ -n "$prepared" ]; then
  title="chore(release): v$prepared"
  n=$(open_pr dev main)
  if [ -z "$n" ]; then
    $gh pr create --base main --head dev --title "$title" \
      --body "Release v$prepared: merging (merge commit) tags it and publishes the release. The changelog and version came into dev with the release pull request."
  else
    $gh pr edit "$n" --title "$title"
  fi
  checks dev
  echo "Release pull request into main: v$prepared"
  exit 0
fi

# The next version from the commits since the last tag.
version=$(quiet "$dir/next-version.sh" | sed -n 's/^version=//p' || true)
if [ -z "$version" ]; then
  n=$(open_pr "$branch" dev)
  if [ -n "$n" ]; then
    $gh pr close "$n" --delete-branch --comment "Nothing to release on dev any more."
  fi
  echo "Nothing to release"
  exit 0
fi
tag=v$version

git switch -q -C "$branch"
"$dir/changelog.sh" "$tag"
"$dir/stamp-version.sh" "$version"
if git diff --quiet; then
  echo "release/next has nothing new for $tag"
  exit 0
fi
git -c user.name="github-actions[bot]" -c user.email="41898282+github-actions[bot]@users.noreply.github.com" \
  commit -q -am "chore(release): $tag"
git push -q -f origin "$branch"

body=$(mktemp)
{
  printf 'Merging this pull request decides the release %s: it brings the version and its CHANGELOG.md section into dev, and the release pull request into main opens. It follows dev until then.\n\n' "$tag"
  awk -v head="## [$version]" 'index($0, head) == 1 { on = 1 } on && /^## / && index($0, head) != 1 { exit } on { print }' CHANGELOG.md
} > "$body"
n=$(open_pr "$branch" dev)
if [ -z "$n" ]; then
  $gh pr create --base dev --head "$branch" --title "chore(release): $tag" --body-file "$body"
else
  $gh pr edit "$n" --title "chore(release): $tag" --body-file "$body"
fi
rm -f "$body"
checks "$branch"
echo "Release pull request into dev: $tag"
