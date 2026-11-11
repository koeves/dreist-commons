# Contributing

Commit subjects follow [Conventional Commits](https://www.conventionalcommits.org/):
`type(scope): summary`, with type one of `feat`, `fix`, `perf`, `refactor`, `docs`, `test`,
`build`, `ci`, `chore`, `style` or `revert`, and `!` after the type for a breaking change. CI
checks every new commit (`.github/workflows/commits.yml`); the changelog is generated from them.

Releases: merging the release pull request (`dev` -> `main`, merge commit) releases; there is
no manual tagging, bumping or changelog commit. `.github/workflows/release-on-main.yml` computes
the version from the commits since the last `v*` tag (`git cliff --bumped-version`, rules in
`cliff.toml`: fix and the rest -> patch, feat -> minor, breaking -> major, and breaking -> minor
while on 0.x), tags the merge commit `vX.Y.Z`, runs `release.yml` (the commit check, CI, the
GitHub release with notes generated from the commits) and opens the follow-up pull request
`docs(release): changelog and version for vX.Y.Z` into `dev` (CHANGELOG.md, the `Cargo.toml` /
`Cargo.lock` version), which merges itself once its checks pass. Only feat, fix, perf, refactor,
revert, breaking and dependency (`build(deps)`, `chore(deps)`) commits are releasable: without
one nothing is released and the run says so. A line `Release-As: X.Y.Z` in the release pull
request's body (or in a commit message since the last tag) sets the version instead, and
releases even without a releasable commit. Release dreist-commons before the apps that need
the change. `release.yml` also runs on a tag pushed by hand, and by hand to republish the notes:
`gh workflow run release.yml --ref <tag> -f tag=<tag> -f upload=true`.

Branches and CI: work happens on `feature/<name>` branches (parallel threads: one worktree each,
outside the checkout), pushed and merged into `dev` through a pull request by squash only: the pull request title is
a Conventional Commit subject and becomes the commit (checked by `conventional`). Merge once its
checks pass (CI, `conventional`, CodeQL). Dependabot targets `dev`: patch
and minor updates, GitHub Actions and submodule bumps merge on their own when the checks pass,
major updates wait for a review. A release is a pull request from `dev` into `main`, merged with
a merge commit (see Releases). GitHub Actions run on pull requests into `dev` and `main`, on
pushes to `main` (the release), on release tags and by hand (`workflow_dispatch`, also how the
follow-up pull request gets its checks), never on other branch pushes. Run the checks locally before pushing: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings` (also
with `--features dreist-gui/window`, and `-p dreist-host --all-features`) and `cargo test --workspace`
(and `-p dreist-host --all-features`).

Apps: Dreist and Einst include this repository as a submodule at `commons`. A change reaches
them when they bump the submodule (`build: dreist-commons ...`); behaviour that one app needs
belongs behind a hook, with the defaults unchanged.

Dependencies: third-party versions live here only (`[workspace.dependencies]`, Dependabot's cargo
updates run here). The apps take them through the commons crates: `dreist-host` owns the
standalone's and editor's system-side crates (cpal, midir, rfd, serde / serde_json, libc,
clap-sys) behind features and re-exports those the apps use directly. A bump reaches an app with
the submodule bump and `cargo update -p <crate>` there.
