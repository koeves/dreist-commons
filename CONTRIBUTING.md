# Contributing

## Commits

- **Format**: [Conventional Commits](https://www.conventionalcommits.org/), `type(scope): summary`, with `!` after the type for a breaking change.
- **Types**: `feat`, `fix`, `perf`, `refactor`, `docs`, `test`, `build`, `ci`, `chore`, `style`, `revert`.
- **Checked**: by the `conventional` check; the changelog and release notes are generated from the commits.

## Branches and pull requests

- **Work**: on `feature/<name>`, one worktree per branch in `.claude/worktrees/<name>`.
- **Into `dev`**: a pull request, squash-merged; its title is the Conventional Commit.
- **Issues**: one per task; the pull request body says `Closes #N`.
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

- **CI**: `macos` and `linux` (both required).
- **CodeQL**: runs on every pull request into `dev` and `main`.

## Releases

- **Release**: a pull request from `dev` into `main`, merged with a merge commit.
- **Automatic**: the merge computes the version from the commits, tags `vX.Y.Z` and publishes the GitHub release.
- **Follow-up**: a changelog and version pull request into `dev` merges itself once green.
- **Version**: fix → patch, feat → minor, breaking → major (minor while on 0.x); `Release-As: X.Y.Z` in the release pull request overrides it.
- **Releasable**: feat, fix, perf, refactor, revert, breaking and dependency commits; without one nothing is released.
- **Order**: release dreist-commons before the apps that need the change.

## Apps

- **Submodule**: Dreist and Einst include this repository at `commons`.
- **Reaching an app**: a change arrives when the app bumps the submodule (`build: dreist-commons ...`).
- **One app's behaviour**: belongs behind a hook, with the defaults unchanged.

## Dependencies

- **Versions**: all third-party versions live here (`[workspace.dependencies]`); Dependabot's cargo updates run here.
- **System crates**: `dreist-host` owns cpal, midir, rfd, serde / serde_json, libc and clap-sys behind features, and re-exports what the apps use.
- **Reaching an app**: with the submodule bump and `cargo update -p <crate>` there.
