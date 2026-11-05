# Git conventions

## Commits

- Subjects follow [Conventional Commits](https://www.conventionalcommits.org/):
  `type(scope): summary`, imperative, lower-case summary, no trailing period.
  Types: `feat`, `fix`, `perf`, `refactor`, `docs`, `test`, `build`, `ci`, `chore`,
  `style`, `revert`; `!` after the type (or a `BREAKING CHANGE:` footer) for a breaking change.
- Scopes are optional and short: a crate or area (`formats`, `dsp`, `engine`, `gui`, `editor`, `testkit`, `ci`).
- A body only when the why is not obvious; wrap at 72 columns.
- CI checks every new commit subject (`.github/workflows/commits.yml`).
- No AI attribution of any kind: no `Co-Authored-By` for an assistant, no session trailers, no
  "Generated with ..." lines, in commits, PRs, tags or release notes. Author and committer are
  the repository's configured identity.

## Changelog and releases

- `CHANGELOG.md` is generated with [git-cliff](https://git-cliff.org) (`cliff.toml`) from the
  commit subjects: `git cliff --unreleased --tag vX.Y.Z --prepend CHANGELOG.md`, then edit the
  new section by hand where it helps.
- Release: bump `version` in `Cargo.toml`, commit `chore(release): vX.Y.Z`, tag `vX.Y.Z`
  (annotated) and push the tag. The release workflow publishes the tag's `CHANGELOG.md`
  section as the release notes (no bundles).

## Apps

Dreist and Einst include this repository as a submodule at `commons`. A change here reaches
them when they bump the submodule (`build: dreist-commons ...`); behaviour changes for one app
belong behind a hook, with the defaults unchanged.

## Worktrees and cleanup

- Worktrees go next to the repository or in a scratch folder, never inside a checkout: the
  bundler builds the outermost workspace it finds, so a nested worktree bundles the outer
  checkout's code.
- Remove a worktree and its local branch once the branch is merged.
- When a task ends, delete what it built and no longer needs: `target/` directories
  (`cargo clean`), scratch builds, test copies and downloads. Keep only what is still in use.

## Branches and CI

GitHub Actions minutes are limited, so:

- All work goes to the `dev` branch; `main` only moves when the owner says a release is ready
  (merge `dev` into `main`, then tag).
- CI runs on pushes to `main`, on tags and by hand (`workflow_dispatch`), never on `dev` and
  no pull requests (they would run it). So run the checks locally before every push: tests,
  clippy (with the standalone feature in the apps, with `dreist-gui/window` in commons) and
  `cargo fmt --check`.
- Until a release, the apps' `commons` submodule may point at dreist-commons `dev` commits. At
  release: merge dreist-commons `dev` into its `main` first, then point the apps at that commit
  and merge them.
