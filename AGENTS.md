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
- Git: Conventional Commits, changelog, releases and branches as in `CONTRIBUTING.md`; agent
  workflow notes are in the private `dreist-re` repository (`agents.md`).
- Branches: `feature/<name>` (one worktree each, outside the checkout) -> pull request into
  `dev` (squash only; the PR title is the Conventional Commit; checks green) -> release pull request `dev` -> `main` (merge commit). The
  merge releases: version from the commits (`Release-As: X.Y.Z` overrides), tag, GitHub release
  and a follow-up changelog / version pull request into `dev` happen on their own; never tag
  or bump by hand (CONTRIBUTING.md). Release commons before the apps.
- After any change here, rebuild and reinstall both apps' VST3s (Dreist and Einst) and say which
  commits are installed.
- Security alerts: check the open code scanning and Dependabot alerts at the start and end of any
  work here (`gh api repos/koeves/dreist-commons/code-scanning/alerts?state=open`, same for
  `dependabot/alerts`); fix real findings through an issue and a PR, dismiss false positives
  with the reason.

## Docs

- Human docs (README, CONTRIBUTING, other `.md` for readers) follow the fzf README style: a centred title with release, build, CodeQL, licence and platform badges under it.
- Short bullets of one sentence, two at most; no walls of text, and detail goes in its own section or file.
- Describe the project as it is: no status updates, no "now", "previously", "since" or "next release" wording, and nothing the reader doesn't need.
- This file is the agent instructions for every agent; `CLAUDE.md`, `GEMINI.md` and `.github/copilot-instructions.md` only point here, so edit this file.
- Agent notes stay in this file or in dreist-re; CHANGELOG.md and THIRD_PARTY_NOTICES.md are left to their generators.

## Build

    export PATH=$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH
    cargo test --workspace
    cargo clippy --workspace --all-targets -- -D warnings
    cargo clippy --workspace --all-targets --features dreist-gui/window -- -D warnings
    cargo clippy -p dreist-host --all-targets --all-features -- -D warnings
    cargo test -p dreist-host --all-features
    cargo fmt --all --check
