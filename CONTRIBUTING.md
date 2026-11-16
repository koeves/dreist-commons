# Contributing

## Commits

- **Format**: [Conventional Commits](https://www.conventionalcommits.org/), `type(scope): summary`, with `!` after the type for a breaking change.
- **Types**: `feat`, `fix`, `perf`, `refactor`, `docs`, `test`, `build`, `ci`, `chore`, `style`, `revert`.
- **Checked**: by the `conventional` check; the changelog and release notes are generated from the commits.

## Branches and pull requests

- **Work**: on `feature/<name>`, one worktree per branch in `.claude/worktrees/<name>`.
- **Into `dev`**: a pull request, squash-merged; its title is the Conventional Commit.
- **Drafts**: longer work opens as a draft pull request, marked ready once its testing is done; short work goes from a local branch straight to a ready pull request.
- **Issues**: for features, bug reports and work spanning several pull requests or repos; the pull request links it with `Closes #N`.
- **No issue**: small fixes, docs, chores and dependency bumps go straight to a pull request with a conventional title.
- **Board**: features and larger work for all three repos are tracked on [Dreist & Einst](https://github.com/users/koeves/projects/1).
- **Rules**: `dev` and `main` take changes only through pull requests with green checks.
- **Dependabot**: targets `dev`; patch, minor and Actions updates merge on their own when green.

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

- **CI**: pull requests into `dev` run `linux` and the Linux `packaging` test; the release pull request and the release also run `macos` and the macOS and Windows `packaging` tests.
- **Docs only**: a pull request that changes only Markdown skips the build and test jobs; a skipped job counts as passed.
- **CodeQL**: runs on every pull request into `dev` and `main`.

## Releases

- **Release pull request into `dev`**: after every push to `dev` with releasable commits, `release-pr.yml` opens or updates `chore(release): vX.Y.Z` with the version and its CHANGELOG section; approving and merging it decides the release.
- **By hand**: `.github/release/prepare.sh` writes the same pull request (`--as X.Y.Z` sets the version, `--dry-run` tries it locally).
- **Version**: from the commits since the last tag: fix → patch, feat → minor, breaking → major (minor while on 0.x); `--as X.Y.Z` or a `Release-As: X.Y.Z` line in a commit sets it.
- **Releasable**: feat, fix, perf, refactor, revert, breaking and dependency commits; `ci`, `docs`, `test`, `style` and other commits alone release nothing.
- **Release**: once that is merged, `release-pr.yml` opens the pull request from `dev` into `main`, merged with a merge commit; its `release-version` check needs the prepared version and changelog and no releasable commit after them.
- **Automatic**: the merge tags `vX.Y.Z` and publishes the GitHub release; the changelog and version are already in `dev`.
- **Order**: release dreist-commons before the apps that need the change.

## Apps

- **Submodule**: Dreist and Einst include this repository at `commons`.
- **Reaching an app**: a change arrives when the app bumps the submodule (`build: dreist-commons ...`).
- **One app's behaviour**: belongs behind a hook, with the defaults unchanged.

## Dependencies

- **Versions**: all third-party versions live here (`[workspace.dependencies]`); Dependabot's cargo updates run here.
- **System crates**: `dreist-host` owns cpal, midir, rfd, serde / serde_json, libc and clap-sys behind features, and re-exports what the apps use.
- **Reaching an app**: with the submodule bump and `cargo update -p <crate>` there.
