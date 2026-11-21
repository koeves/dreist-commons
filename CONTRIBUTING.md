# Contributing

## Commits

- **Format**: [Conventional Commits](https://www.conventionalcommits.org/), `type(scope): summary`, with `!` after the type for a breaking change.
- **Types**: `feat`, `fix`, `perf`, `refactor`, `docs`, `test`, `build`, `ci`, `chore`, `style`, `revert`.
- **Checked**: by the `conventional` check. Versions and release notes are generated from the commits.

## Branches and pull requests

- **Work**: on `feature/<name>`, one worktree per branch in `.claude/worktrees/<name>`.
- **Into `dev`**: a pull request, squash-merged. Its title is the Conventional Commit.
- **Drafts**: longer work opens as a draft pull request, marked ready once its testing is done. Short work goes from a local branch straight to a ready pull request.
- **Issues**: for features, bug reports and work spanning several pull requests or repos. The pull request links it with `Closes #N`.
- **No issue**: small fixes, docs, chores and dependency bumps go straight to a pull request with a conventional title.
- **Larger work**: one tracking issue with sub-issues or a checklist, in dreist-commons when it spans repos. No project board.
- **Rules**: `dev` and `main` take changes only through pull requests with green checks.
- **Dependabot**: targets `dev`. Patch, minor and Actions updates merge on their own when green.

## Checks

Run them before pushing:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --workspace --all-targets --features dreist-gui/window -- -D warnings
cargo clippy -p dreist-host --all-features --all-targets -- -D warnings
cargo test --workspace
cargo test -p dreist-gui --features window
cargo test -p dreist-host --all-features
```

- **CI**: pull requests into `dev` run `linux` and the Linux `packaging` test. The release pull request and the release also run `macos` and the macOS (with the Audio Unit check) and Windows `packaging` tests.
- **Docs only**: a pull request that changes only Markdown skips the build and test jobs. A skipped job counts as passed.
- **CodeQL**: runs on every pull request into `dev` and `main`.

## Releases

- **Release**: when `dev` is ready, a pull request from `dev` into `main`, merged with a merge commit. Its `release-version` check shows the version it releases and sets the title to `chore(release): vX.Y.Z` (or `chore: sync dev into main (no release)`).
- **Version**: from the commits since the last tag: fix → patch, feat → minor, breaking → major (minor while on 0.x). `Release-As: X.Y.Z` in the release pull request or a commit sets it.
- **Releasable**: feat, fix, perf, refactor, revert, breaking and dependency commits. `ci`, `docs`, `test`, `style` and other commits alone release nothing.
- **Automatic**: the merge tags `vX.Y.Z` and publishes the GitHub release, with the changes since the last tag as its notes.
- **Changes**: on the [Releases](https://github.com/koeves/dreist-commons/releases) page. There is no changelog file.
- **`dev` follows `main`**: after the merge, `dev` is fast-forwarded to `main` (the `DEV_SYNC_KEY` deploy key), or `main` is merged into `dev` when `dev` has moved on. A sync pull request opens only when that merge conflicts.
- **Versions in the repository**: `Cargo.toml` keeps `0.0.0`. The release is the tag.
- **Order**: release dreist-commons before the apps that need the change.

## Apps

- **Submodule**: Dreist and Einst include this repository at `commons`.
- **Reaching an app**: a change arrives when the app bumps the submodule (`build: dreist-commons ...`).
- **One app's behaviour**: belongs behind a hook, with the defaults unchanged.

## Dependencies

- **Versions**: all third-party versions live here (`[workspace.dependencies]`). Dependabot's cargo updates run here.
- **System crates**: `dreist-host` owns cpal, midir, rfd, serde / serde_json, libc and clap-sys behind features, and re-exports what the apps use.
- **Reaching an app**: with the submodule bump and `cargo update -p <crate>` there.
