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
- Branches: work on local feature branches or local `dev` (parallel threads: one worktree each,
  outside the checkout); push nothing while working. At a release, clean up `dev`'s commits, push `dev`,
  merge into `main` (`--no-ff`) and release.
- After any change here, rebuild and reinstall both apps' VST3s (Dreist and Einst) and say which
  commits are installed.

## Build

    export PATH=$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH
    cargo test --workspace
    cargo clippy --workspace --all-targets -- -D warnings
    cargo clippy --workspace --all-targets --features dreist-gui/window -- -D warnings
    cargo clippy -p dreist-host --all-targets --all-features -- -D warnings
    cargo test -p dreist-host --all-features
    cargo fmt --all --check
