# dreist-commons

Shared crates of Dreist and Einst (see README.md). Apps plug in through
`dreist_formats::profile` and `dreist_dsp::fx::install_factory`; keep app specifics (names,
resources, IDs, menus, readers) in the apps, not here.

- No original product, vendor or class names in code, comments or docs (only in README and in
  `dreist_formats::legacy`, which holds the legacy on-disk names). Cite reverse-engineering
  evidence with tags `[re:xxxxxx]`; the tag map and the ledgers are in the private `dreist-re`
  repository (`dreist/evidence-tags.tsv`, `einst/evidence-tags.tsv`).
- Goldens (`crates/*/tests/golden`) must stay identical unless a change is meant to alter them;
  explain every golden change in its commit. Porting bugs get their own commit with an evidence
  reference.
- Tests that need UI graphics or factory data use a Dreist checkout at `../dreist` and skip
  without one.
- Git: Conventional Commits, releases and branches as in `CONTRIBUTING.md`; agent
  workflow notes are in the private `dreist-re` repository (`agents.md`).
- Branches: `feature/<name>` (one worktree each, in `.claude/worktrees/<name>`) -> pull request into
  `dev` (squash only; the PR title is the Conventional Commit; checks green) -> release pull request `dev` -> `main` (merge commit). The
  merge releases: version from the commits (`Release-As: X.Y.Z` overrides), tag, GitHub release happen on their own, and `dev` then follows `main`; never tag
  or bump by hand (CONTRIBUTING.md). Release commons before the apps.
- Pull requests: a draft while longer work is in progress, marked ready once its testing is done; short
  work goes from a local branch straight to a ready pull request.
- After any change here, rebuild and reinstall both apps' VST3s (Dreist and Einst) and say which
  commits are installed.
- Security alerts: check the open code scanning and Dependabot alerts at the start and end of any
  work here (`gh api repos/koeves/dreist-commons/code-scanning/alerts?state=open`, same for
  `dependabot/alerts`); fix real findings through an issue and a PR, dismiss false positives
  with the reason.

## Agent rules

- No Claude or AI attribution: no `Co-Authored-By` or `Claude-Session` trailers, "Generated with Claude Code" footers or claude.ai links in commits, pull requests, issues, comments or release notes.
- Re-read a pull request, issue or comment body after writing it, because some tools append a footer.
- Stage only the paths of the change (`git add <paths>`), never `git add -A`, `git add .` or `git commit -a`; use the global git identity.
- The owner merges feature pull requests; agents open them.
- `.claude/settings.json` and `.claude/hooks/guard.py` enforce these rules, no pushes to `dev` or `main`, and no edits to generated files; the `attribution` job in `commits.yml` checks pull requests. Keep both files identical in dreist-commons, dreist and einst.

## Docs

- Human docs (README, CONTRIBUTING, other `.md` for readers) follow the fzf README style: a centred title with release, build, CodeQL, licence and platform badges under it.
- Short bullets of one sentence, two at most; no walls of text, and detail goes in its own section or file.
- Describe the project as it is: no status updates, no "now", "previously", "since" or "next release" wording, and nothing the reader doesn't need.
- This file is the agent instructions for every agent; `CLAUDE.md`, `GEMINI.md` and `.github/copilot-instructions.md` only point here, so edit this file.
- Agent notes stay in this file or in dreist-re; THIRD_PARTY_NOTICES.md is left to its generator; there is no CHANGELOG.md (the notes are on GitHub Releases).

## Build

    cargo test --workspace
    cargo clippy --workspace --all-targets -- -D warnings
    cargo clippy --workspace --all-targets --features dreist-gui/window -- -D warnings
    cargo clippy -p dreist-host --all-targets --all-features -- -D warnings
    cargo test -p dreist-host --all-features
    cargo fmt --all --check
