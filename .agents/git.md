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
