# Changelog

## [0.1.1] - 2026-11-06

Second release, for Einst's first release (Dreist picks it up later). Main's history was cleaned up:
`main` now holds the initial commit and one merge per release (`git log --first-parent main`);
every change stays on `dev`, unchanged. The v0.1.0 tag and release are kept as they were.
GitHub Actions now run only on release tags (and by hand), not on pushes to `main`.

### What was ported

- The hooks Einst needs to rebuild the original v1 on these crates, all opt-in so Dreist is
  unchanged: v1 rules for step lanes, pan and sends, reverse and end fade, timestretch, scene
  snapshots, sampler modes, the default MIDI map and slicer pad count; slice types; effect
  control outputs on the meters; GUI drawing for v1's skin (gradients, bevels, radial knobs,
  level strips, meters, plots, tables, block menu icons, draggable value boxes).
- Shared fixes for both apps: end fades, CLAP state load in the vendored nih-plug, several
  copies of the plugin in one host, and browser previews decoded off the editor thread.

### Breaking

- `dreist-editor` moved out: each app owns its editor (Dreist's `dreist-editor`, Einst's
  `einst-editor`), so their screens can differ. The profile loses the editor-only fields
  (`about`, `settings_dir`, `legacy_settings_dirs`, `settings_env`, `open_also`,
  `effect_menu`, `read_document`, `read_project`) and `open_extensions`, `plain_document`,
  `plain_project`.

### Features

- **gui:** The header logo's mark comes from the app profile
- **gui:** Draw level strips, pan sliders, needle meters, plots and tables
- **gui:** Gradients, bevels, knob pointers, dashes, arrow buttons, strips, range sliders and text effects
- **gui:** Slider grooves and caps, shaded gradients, knob face rings, popup arrows
- **gui:** Radial gradients, text glow, brushed noise, grip shapes
- **engine:** Play one layer of a pad from the editor
- **engine:** Opt-in v1 step lanes at note start
- **engine:** Opt-in v1 pan stage and send summing
- **dsp:** Opt-in v1 reverse and end fade
- **dsp:** Opt-in v1 timestretch ratio
- **engine:** Opt-in v1 pattern MIDI export
- **vendor:** Baseview background mode for tests and harnesses
- **engine:** Sample memory and its readout text
- **engine:** The original v1's routing-depth lanes and routing slots reach the voice
- **dreist-engine:** `SampleCache::forget`
- **dreist-engine:** Rule for the original v1's default MIDI map
- **dreist-dsp:** Rule for the slicer's pad count
- **dreist-gui:** Optional fine knob drag
- **dsp:** Slice types and v1's slice type pad allocation
- **engine:** Save and load slice types in analysis files
- **engine:** Effect control outputs reach the meters
- **engine:** Export `FxOut`
- **gui:** Plots draw several coloured curves
- **dsp:** The original v1's timestretch amount curve
- **engine:** v1's scene snapshot of empty patterns
- **gui:** Optional ctrl-click as right click
- **dreist-gui:** Dragable value boxes
- **dreist-gui:** Block icon menu items
- **engine:** `Rules::sampler_modes_v1` keeps the sampler modes in the project

### Fixes

- **dsp:** End fade starts at the sample that reaches it
- **dsp:** End fade checks the frame read before the advance
- **dsp:** Cut fades clamp to the region length
- **vendor:** nih-plug CLAP state load checks the claimed length
- **vendor:** nih-plug CLAP state load asks the host to rescan values
- **vendor:** Softbuffer observer class unique to each library copy
- **gui:** Menu target and drag source classes unique to each copy
- **vendor:** nih-plug editor open and close catch panics
- **engine:** Merged meters take the later effect outputs

### Performance

- **engine:** Browser previews decode off the editor thread

### Refactoring

- Move the editor out; each app owns its editor (breaking)

### Documentation

- Contributing section, git conventions for agents
- **readme:** Drop the note on reverse-engineering notes
- **formats:** Note the shipped Analog Ring Mod parameter order
- Worktree placement and cleanup after a task
- Coordinate shared-code changes between Einst and Dreist
- Coordination log for changes from either app
- Coordination log for `SampleCache::forget`
- Coordination log for the v1 MIDI map rule
- Coordination log for slice_pads
- Coordination log for fine_drag
- Coordination log for slice types
- Coordination log for slice type files
- Coordination log for effect outputs
- Coordination log for FxOut export
- Coordination log for meter merge
- Coordination log for plot curves
- Coordination log for v1 stretch curve
- Coordination log for scene snapshot rule
- Coordination log for ctrl-click
- Coordination log for dragable value boxes
- Coordination log for block icon menu items
- Coordination log for sampler_modes_v1
- Coordination log for the v0.1.1 release and tag-only CI

### Build and CI

- Vendor nih-plug
- Vendor softbuffer
- Run on main, tags and by hand only; work goes to dev
- Changelog skips coordination log commits
- Run workflows only on release tags
- Build on the macos-15 runner


## [0.1.0] - 2026-11-03

First release: the crates Dreist and Einst share, moved out of Dreist beta 1 and renamed
`dreist-*`.

### Ported

- **dreist-formats**: the document tree and its XML reader/writer; project, kit, pad, engine,
  pattern, bank and preset readers; parameter and effect tables; resource paths.
- **dreist-dsp**: the sampler voice (playback, loops, envelopes, voice filters, modulation) and
  the 47 effect nodes, with goldens of every effect.
- **dreist-engine**: step and song sequencer, scenes, mixing and sends, offline render and
  export, MIDI in and MIDI files, host and MIDI bindings, REX import (macOS).
- **dreist-gui** and **dreist-editor**: the skin engine, controls and rendering, and the full
  editor (pads, layers, patterns, song, browser, inspector, effects, drag and drop).
- **dreist-testkit**: golden files and allocation checks.

### Hooks for apps

- App profile (`dreist_formats::profile`): names, resources, settings folder, the file family an
  app saves (`.geist*`, `.dreist*` and `.einst*` all open), effect menu and document readers.
- Effect-node factory (`fx::install_factory`), the parameter 2^x (`fx::install_exp2`),
  behaviour rules (`rules`: sampler auto-stop, envelope ends, forward loop) and the voice
  parameter model (`model`). Defaults reproduce Dreist bit for bit.
- Golden harnesses for apps: `engine::harness::check_scenarios`, `editor::screens::check_screens`.

### Fixes found while porting

- Modulated Filter: one FM coefficient per sample, from the left input.
- Tune to BPM: fine tune from the unclamped coarse value, in double precision.
- Voice filter: the drive ramp survives a voice reset.
