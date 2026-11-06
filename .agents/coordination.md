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
- Not yet picked up: Dreist `dev` is at 5462f7c (only docs commits after it); Dreist `main` is at
  230f34e (everything from cdd0f5e on reaches it at the next release merge).
