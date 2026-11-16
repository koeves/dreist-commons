#!/usr/bin/env bash
# Prepare a release: cut release/vX.Y.Z from origin/dev, write the version
# (stamp-version.sh) and its CHANGELOG.md section (changelog.sh) in one
# commit by you, push it and open its pull request into dev. Once that is
# merged, `--open` opens the release pull request (dev -> main); merging that
# releases (release-on-main.yml).
#
#   .github/release/prepare.sh                 the version from the commits since the last tag
#   .github/release/prepare.sh --as X.Y.Z      this version instead (Release-As)
#   .github/release/prepare.sh --dry-run       commit locally only, print the result
#   .github/release/prepare.sh --open          open the release pull request
#
# Works in a temporary worktree, so the current checkout is left alone.
# Needs git, git-cliff, cargo and gh.
set -euo pipefail

as=""
dry=0
open=0
while [ $# -gt 0 ]; do
  case "$1" in
    --as) as=${2:?--as needs a version}; shift ;;
    --dry-run) dry=1 ;;
    --open) open=1 ;;
    -h | --help) sed -n '2,15p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
  shift
done

root=$(git rev-parse --show-toplevel)
cd "$root"
git fetch -q --tags origin dev main

if [ "$open" = 1 ]; then
  version=$(git show origin/dev:Cargo.toml | sed -n 's/^version = "\(.*\)"/\1/p' | head -n 1)
  if git rev-parse -q --verify "refs/tags/v$version" > /dev/null; then
    echo "dev has $version, released already: prepare a release first" >&2
    exit 1
  fi
  gh pr create --base main --head dev --title "chore(release): v$version" \
    --body "Release v$version: \`dev\` into \`main\` (merge commit). The changelog and version came in with the \`docs(release)\` commit (prepare.sh)."
  exit 0
fi

tmp=$(mktemp -d "${TMPDIR:-/tmp}/prepare.XXXXXX")
cleanup() { git -C "$root" worktree remove --force "$tmp" 2>/dev/null || true; rm -rf "$tmp"; }
trap cleanup EXIT
git worktree add -q --detach "$tmp" origin/dev
cd "$tmp"
if [ -f .gitmodules ]; then git submodule update -q --init; fi

if [ -n "$as" ]; then
  version=${as#v}
  [[ $version =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "not a version: $as" >&2; exit 2; }
else
  version=$(GITHUB_OUTPUT=/dev/stdout GITHUB_STEP_SUMMARY=/dev/null .github/release/next-version.sh | sed -n 's/^version=//p')
  [ -n "$version" ] || { echo "Nothing to release (see above); --as X.Y.Z releases anyway" >&2; exit 1; }
fi
tag=v$version
if git rev-parse -q --verify "refs/tags/$tag" > /dev/null; then
  echo "$tag exists already" >&2
  exit 1
fi

branch=release/$tag
git switch -q -c "$branch"
.github/release/changelog.sh "$tag"
.github/release/stamp-version.sh "$version"
title="docs(release): changelog and version for $tag"
git commit -q -am "$title"
echo "== $title"
git show --stat --format='%an <%ae>%n%s' HEAD

if [ "$dry" = 1 ]; then
  git --no-pager diff HEAD~1 -- CHANGELOG.md | head -n 60
  git switch -q --detach
  git -C "$root" branch -q -D "$branch"
  echo "(dry run: nothing pushed, no branch kept)"
  exit 0
fi
git push -q -u origin "$branch"
gh pr create --base dev --head "$branch" --title "$title" \
  --body "CHANGELOG.md and the workspace version for $tag (prepare.sh). After it is merged: \`.github/release/prepare.sh --open\` opens the release pull request."
