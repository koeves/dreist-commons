# Coordination log (dreist-commons)

Einst and Dreist both depend on this repository, and each project's agents may change it. This
file is how they keep each other informed. **Read it before touching commons; append an entry
after every commons push** (newest at the bottom): date, commit(s), what changed, which app it is
for, whether the other app's behaviour or goldens change, and any action needed on its side.

Submodule pointers when this log started (2026-11-05): Dreist `main` → 230f34e, Dreist `dev` →
5462f7c; Einst `dev` → 5462f7c.

## 2026-11-03 – 2026-11-04 (history before this log, all on `main`, picked up by both apps)

- 88468af: the split (shared crates of Dreist and Einst); 35cb4ca each app owns its editor.
- From the Einst side, all opt-in or hooks with v2 defaults unchanged, Dreist unaffected unless
  noted: installable 2^x for effects and voice filters (a0cb360, 77202ba), voice parameter model
  hook (29012c2), v1 rules for envelope ends / forward loop / step lanes / pan and sends / reverse
  and end fade / timestretch ratio / pattern MIDI export (fe5eb6f, a4e3f77, d49c00e, 9401a7c,
  2fd770b, 2d43602), layer-only trigger (e840de6), GUI drawing features (9d0a97f, 0a7cb89,
  4e2e504, d872395, 80c8e56).
- Porting fixes that changed Dreist too (goldens updated in Dreist at the time): Modulated Filter
  FM per sample from the left input (c4b2f78), tune-to-BPM fine tune (50de061), voice filter drive
  ramp across reset (fd5d072), end fade from the triggering sample, its frame check and the cut
  fade clamp (673bb4c, c4a67e6, 5559d30), vendored nih-plug with CLAP state fixes (04e0398,
  c6b7c5a, 230f34e).

## 2026-11-05 (Einst side)

- cdd0f5e: baseview background mode (`DREIST_BACKGROUND_GUI=1`, for tests/harnesses). Both apps;
  no behaviour change without the variable.
- 44d96fd, b5ba179, fb8a20b, 7b89d4f: second plugin editor in one host no longer aborts (unique
  ObjC class names for softbuffer's observer and the menu / drag-out classes; editor open/close
  catch panics). Both apps; Dreist must patch softbuffer to `commons/vendor/softbuffer` in its
  workspace Cargo.toml (done in Dreist be63075).
- fcf90a4: browser previews decode off the editor thread (`dreist_engine::preview`). Both apps
  (Dreist 8dbbf7b uses it); goldens unchanged.
- af84117: docs (worktree placement, cleanup). No code.
- 9c356c2: CI runs on main, tags and by hand only; work goes to `dev`. Both.
- 9584edc: `Dreist::sample_bytes` and `memory::bytes_label` (memory readout). Both apps (Dreist
  81b6766 shows it); goldens unchanged.
- 5462f7c: the original v1's four routing-depth step lanes (lanes 93–96, `LANE_COUNT` 97) and
  routing slots reach the voice model. For Einst; v2 documents have neither, so Dreist's renders
  and goldens are unchanged. **Dreist action:** lane menus must list `STEP_LANES.len()` (93), not
  `pattern::LANES` (done in Dreist 7ccd003).
- 217d77e: docs (shared-code coordination rule). No code.
- a09a64c: `SampleCache::forget` (Einst reloads samples changed on disk). Additive; Dreist
  unaffected, nothing to do.
- 522fc8c: `Rules::midi_map_v1` (v1's default MIDI map). Off by default; Dreist unchanged.
  **Dreist action** only if Dreist builds a `Rules` literal: add `midi_map_v1: false`.
- 63dafdb: `Rules::slice_pads` (64 by default; Einst 16). Dreist unchanged.
- 6a909e4: `App::fine_drag` (off by default; Einst on). Dreist unchanged.
- 957bd35: `SlicePoint::kind` (slice type) and `slice::extract_pattern_full` (v1's
  slice type allocation). Existing extract functions unchanged; Dreist unchanged. **Dreist
  action** only if Dreist builds a `SlicePoint` literal: add `kind: 0` (none found).
- 70c996b: analysis files read / write a slice's `Type` (only when typed). Dreist unchanged.
- 7004d94: `Node::output` / `Effect::output` (effect control outputs) and
  `Meters::fx_out` / `Meters::fx_output` (gain-reduction meters per chain slot). Audio and goldens
  unchanged; Dreist can ignore them. **Dreist action** only if Dreist builds a `Meters` literal
  without `..Default::default()` (none found).
- 0a772ea: `dreist_engine::FxOut` exported. No behaviour change.
- e717364: `Meters::merge` takes the merged-in block's effect outputs. No change for Dreist.
- e2482ba: `CtrlPlot` `curves` attribute (several coloured curves). Opt-in; Dreist never sets it, goldens unchanged.
- 0e592d8: v1 timestretch amount curve under `Rules::timestretch_v1`, plus
  `params::set_stretch_v1` for the knob units (off by default). Dreist unchanged.
- 86445a9: `Rules::scene_snapshot_v1` (off by default; Einst on). Dreist unchanged. **Dreist
  action** only if Dreist builds a `Rules` literal: add `scene_snapshot_v1: false`.
- 7db4810: `App::ctrl_click_right` (off by default; Einst on). Dreist unchanged.
- fb3406d: dreist-gui value boxes drag when the control or its style sets `dragable` (v1's flag; up = more, 200 px per range, shift 2000 px). Opt-in; Dreist's skins never set it, so Dreist is unchanged.
- ccf9715: dreist-gui `menu::BLOCK` item prefix (v1's block icon in the icon column). Opt-in; Dreist never uses it, so Dreist is unchanged.
- 8000b85: `Rules::sampler_modes_v1` (off by default; Einst on): the project `Sampler` node keeps `startmode` / `timedmode` by v1 name and the engine reads them; `sampler::V1_START_MODES` / `V1_TIMED_MODES`. Dreist unchanged. **Dreist action** only if Dreist builds a `Rules` literal: add `sampler_modes_v1: false`.
- Not yet picked up: Dreist `dev` is at 5462f7c (only docs commits after it); Dreist `main` is at
  230f34e (everything from cdd0f5e on reaches it at the next release merge).

## 2026-11-06 (commons and Einst release v0.1.1; Dreist not part of it)

- commons v0.1.1: `main` now holds the initial commit (88468af) plus one `--no-ff` merge of
  `dev` per release (`git log --first-parent main`). commons `dev` was not rewritten: every
  commit stays where it was, so Dreist's pointers (`dev` 5462f7c, `main` fcf90a4) stay reachable.
- GitHub Actions now run only on release tags (`vX.Y.Z`: commit-subject check, CI, release) and
  by hand (`workflow_dispatch`); no workflow runs on a push to `main` or any branch.
- Einst v0.1.1 (its first release) points at the commons v0.1.1 merge. Einst's `dev` was
  reworded to Conventional Commits and force-pushed.
- **Dreist action, whenever Dreist is ready (left out of this release at the owner's request):**
  bump the submodule to commons v0.1.1 (`build: dreist-commons v0.1.1`); apply the same CI rule
  (drop `push: branches` from ci.yml and commits.yml, call commits.yml from release.yml; see
  commons `.agents/git.md`, "Branches and CI"); `cliff.toml` may skip `^docs: coordination log`
  as commons does now.

## 2026-11-06 (Dreist release v0.1.1)

- Dreist v0.1.1 is released. Its submodule points at the commons v0.1.1 merge on `main`
  (76cc631, `build: dreist-commons v0.1.1`). No commons code changed; commons goldens are
  unchanged. Dreist's only golden change is its own `modal-about` (the About dialog shows the
  version).
- Dreist applies the same CI rule: workflows run only on `vX.Y.Z` tags and by hand. The release
  workflow calls commits.yml and ci.yml, and macOS jobs use `macos-15`.
- Dreist `main` was cleaned up the same way as commons: the beta 1 commit (c39869f) plus one
  `--no-ff` merge of `dev` per release (6d67210 for v0.1.1, tag v0.1.1). The v0.1.0 tag and
  release are kept.
- Dreist `dev` was force-pushed once, at the owner's request, to fix one commit author: bcc192b
  and the 29 commits after it were re-authored to koeves; trees, messages and dates are
  unchanged (dev e0ac1c7, was f63b59f). Commits up to 8815db6 keep their SHAs. Any Dreist
  checkout or worktree based on the old `dev` should reset to `origin/dev`.
- No action needed in Einst or commons. Dreist's next submodule bumps go to commons `dev`
  commits again until the next release.
