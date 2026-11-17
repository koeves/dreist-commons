#!/usr/bin/env bash
# Bring dev up to main after a push to main: fast-forward dev to main's
# commit (the checkout's origin uses the DEV_SYNC_KEY deploy key, which may
# bypass dev's rules), so both point at the same commit. When dev has commits
# main lacks, a fast-forward is not possible: open a sync pull request
# (sync/main-<sha>) into dev instead and run its checks (pull requests opened
# with GITHUB_TOKEN start no workflows). `GH` replaces gh (tests).
set -euo pipefail

gh=${GH:-gh}
main=$(git rev-parse HEAD)
git fetch -q origin dev

if git merge-base --is-ancestor "$main" origin/dev; then
  echo "dev already contains main ($main)."
  exit 0
fi
if git merge-base --is-ancestor origin/dev "$main"; then
  git push -q origin "$main:refs/heads/dev"
  echo "dev fast-forwarded to main ($main)."
  exit 0
fi

branch=sync/main-$(git rev-parse --short "$main")
git push -q -f origin "$main:refs/heads/$branch"
if [ -z "$($gh pr list --head "$branch" --base dev --state open --json number --jq '.[0].number // empty')" ]; then
  $gh pr create --base dev --head "$branch" --title "chore: sync main into dev" \
    --body "dev has commits that main lacks, so it could not be fast-forwarded to main ($main). Merge this to bring main's commits into dev."
fi
for wf in ci.yml commits.yml codeql.yml; do $gh workflow run "$wf" --ref "$branch"; done
echo "::warning::dev has moved on since the release: opened a sync pull request ($branch) instead of a fast-forward."
