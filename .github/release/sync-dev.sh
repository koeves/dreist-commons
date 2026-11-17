#!/usr/bin/env bash
# Bring dev up to main after a push to main (the checkout's origin uses the
# DEV_SYNC_KEY deploy key, which may bypass dev's rules):
#
# - dev is behind main: fast-forward dev to main's commit.
# - dev has commits of its own: merge main into dev ("chore: sync main into
#   dev") and push that, so dev's history holds main's commits and tags.
# - The merge conflicts: open a sync pull request (sync/main-<sha>) to
#   resolve by hand, and run its checks (pull requests opened with
#   GITHUB_TOKEN start no workflows).
#
# `GH` replaces gh (tests).
set -euo pipefail

gh=${GH:-gh}
main=$(git rev-parse HEAD)
bot=(-c user.name="github-actions[bot]" -c user.email="41898282+github-actions[bot]@users.noreply.github.com")

for _ in 1 2 3; do
  git fetch -q origin dev
  if git merge-base --is-ancestor "$main" origin/dev; then
    echo "dev already contains main ($main)."
    exit 0
  fi
  if git merge-base --is-ancestor origin/dev "$main"; then
    if git push -q origin "$main:refs/heads/dev"; then
      echo "dev fast-forwarded to main ($main)."
      exit 0
    fi
    continue
  fi
  git checkout -q --detach origin/dev
  if git "${bot[@]}" merge -q --no-ff --no-edit -m "chore: sync main into dev" "$main"; then
    if git push -q origin "HEAD:refs/heads/dev"; then
      echo "dev had moved on: merged main ($main) into dev."
      git checkout -q --detach "$main"
      exit 0
    fi
    git checkout -q --detach "$main"
    continue
  fi
  git merge --abort
  git checkout -q --detach "$main"
  branch=sync/main-$(git rev-parse --short "$main")
  git push -q -f origin "$main:refs/heads/$branch"
  if [ -z "$($gh pr list --head "$branch" --base dev --state open --json number --jq '.[0].number // empty')" ]; then
    $gh pr create --base dev --head "$branch" --title "chore: sync main into dev" \
      --body "Merging main ($main) into dev conflicts. Resolve it by merging main into dev locally, so dev keeps main's commits and tags in its history."
  fi
  for wf in ci.yml commits.yml codeql.yml; do $gh workflow run "$wf" --ref "$branch"; done
  echo "::warning::Merging main into dev conflicts: opened a sync pull request ($branch)."
  exit 0
done
echo "::error::dev kept moving while it was being brought up to main; run the release-on-main dev job again."
exit 1
