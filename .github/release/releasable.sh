# shellcheck shell=bash
# Sourced by next-version.sh and release-version.sh: the commit subjects that
# make a release (feat, fix, perf, refactor, revert, breaking changes and
# dependency bumps).
# shellcheck disable=SC2034 # used by the scripts that source this file
RELEASABLE='^(feat|fix|perf|refactor|revert)(\([^)]*\))?!?: |^[a-z]+(\([^)]*\))?!: |^(build|chore)\(deps\): |^build(\([^)]*\))?: dreist-commons '
