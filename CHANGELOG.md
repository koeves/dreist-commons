# Changelog

## [0.1.0] - 2026-11-06

First release: the crates Dreist and Einst share, moved out of Dreist beta 1 and renamed
`dreist-*`.

### What was ported

- **dreist-formats**: the document tree and its XML reader/writer; project, kit, pad, engine,
  pattern, bank and preset readers; parameter and effect tables; resource paths.
- **dreist-dsp**: the sampler voice (playback, loops, envelopes, voice filters, modulation,
  timestretch) and the 47 effect nodes, with goldens of every effect.
- **dreist-engine**: step and song sequencer, scenes, mixing and sends, offline render and
  export, MIDI in and MIDI files, host and MIDI bindings, slicing, REX import (macOS).
- **dreist-gui**: the skin engine, controls and rendering each app's editor is built on.
- **dreist-testkit**: golden files and allocation checks.
- Hooks for apps, all with Dreist's behaviour as the default: the app profile, the effect-node
  factory, behaviour rules (the original v1's envelope ends, loops, step lanes, pan and sends,
  reverse and end fade, timestretch, scene snapshots, sampler modes, MIDI map, slicer pads),
  the voice parameter model and golden harnesses.
- The vendored nih-plug: CLAP state load checks and host rescans, editor panics caught, and
  inactive auxiliary outputs no longer silence the plugin in hosts such as Live.

### Features

- The shared crates of Dreist and Einst
- **dsp:** App-installable 2^x for effect parameter conversions
- **dsp:** Voice filter cores take the app's 2^x; behaviour rules (sampler auto-stop)
- **dsp:** Rules for v1's envelope end and forward loop
- **dsp:** Installable voice parameter model for pitch and filter
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
- **dreist-engine:** SampleCache::forget
- **dreist-engine:** Rule for the original v1's default MIDI map
- **dreist-dsp:** Rule for the slicer's pad count
- **dreist-gui:** Optional fine knob drag
- **dsp:** Slice types and v1's slice type pad allocation
- **engine:** Save and load slice types in analysis files
- **engine:** Effect control outputs reach the meters
- **engine:** Export FxOut
- **gui:** Plots draw several coloured curves
- **dsp:** The original v1's timestretch amount curve
- **engine:** v1's scene snapshot of empty patterns
- **gui:** Optional ctrl-click as right click
- **dreist-gui:** Dragable value boxes
- **dreist-gui:** Block icon menu items
- **engine:** Rules::sampler_modes_v1 keeps the sampler modes in the project

### Fixes

- **dsp:** Modulated Filter takes one FM coefficient per sample from the left input
- **engine:** Tune to BPM takes fine tune from the unclamped coarse value, in double
- **dsp:** Voice filter keeps the drive ramp across a voice reset
- **dsp:** End fade starts at the sample that reaches it
- **dsp:** End fade checks the frame read before the advance
- **dsp:** Cut fades clamp to the region length
- **vendor:** nih-plug CLAP state load checks the claimed length
- **vendor:** nih-plug CLAP state load asks the host to rescan values
- **vendor:** Softbuffer observer class unique to each library copy
- **gui:** Menu target and drag source classes unique to each copy
- **vendor:** nih-plug editor open and close catch panics
- **engine:** Merged meters take the later effect outputs
- **vendor:** nih-plug gives inactive aux outputs scratch buffers

### Performance

- **engine:** Browser previews decode off the editor thread

### Refactoring

- Move the editor out; each app owns its editor (breaking)

### Documentation

- Add README, CLAUDE.md and PROGRESS.md
- Plain words that are not references stay words in comments
- **readme:** How apps pin and override this repository
- **readme:** How the three projects relate
- Describe behaviour instead of the original's internals in comments
- Contributing section, git conventions for agents
- **readme:** Drop the note on reverse-engineering notes
- **formats:** Note the shipped Analog Ring Mod parameter order
- Worktree placement and cleanup after a task
- Coordinate shared-code changes between Einst and Dreist
- Coordination log for changes from either app
- Coordination log for SampleCache::forget
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
- **changelog:** V0.1.1 ships with Einst; Dreist picks it up later
- Coordination log for the Dreist v0.1.1 release

### Tests

- Skip the UI-asset tests without an app's UI folder

### Build and CI

- Test on Linux and macOS against a Dreist checkout
- Run without an app checkout; README: public library, submodule use
- Conventional-commit check, git-cliff changelog, tag-triggered release
- Vendor nih-plug
- Vendor softbuffer
- Run on main, tags and by hand only; work goes to dev
- Changelog skips coordination log commits
- Run workflows only on release tags
- Build on the macos-15 runner

### Other

- Remove agent notes and unused files
