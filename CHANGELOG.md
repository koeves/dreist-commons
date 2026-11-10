# Changelog

## [0.2.1] - 2026-11-10

Shared work for Dreist's UI pass. Everything new is opt-in (an app marks a container
scrollable or sets a style / App flag), so Einst looks and behaves as before.

### Summary

- **dreist-gui**: scroll containers (wheel, shift + wheel, trackpad, skin scroll bars,
  thumb drag and paging) that keep their offset through resizes; the original's stock
  control input (relative knob drags, slider jumps, depth snapping, wheel steps, tempo box,
  menus picked on release, option-drags keeping the modulation end); transmod modulation
  layers, framed scroll thumbs, stepper and disclosure arrows, knob position marks,
  gain-reduction meters and EQ plot curves.
- **dreist-dsp**: response curves for the EQ and filter displays; the limiter and VCA
  compressor report gain reduction.

### Features

- **gui:** Opt-in scroll containers
- **gui:** Opt-in transmod layers, framed scroll thumbs, stepper and disclosure arrows
- **gui:** Opt-in second-generation stock control input
- **dsp:** Effect display curves and the limiter / VCA meters
- **gui:** Opt-in poly plot curves, grid and curve fills
- **gui:** Opt-in effect panel drawing from the original
- **gui:** Menu items picked on release, option-drags keep the modulation end

### Fixes

- **gui:** Transmod slider hit test defaults realModDim to 0.25
- **gui:** Scrolled content keeps its offset through resizes

## [0.2.0] - 2026-11-10

Shared work for Einst's v1 look and parity pass and Dreist's video parity pass. All new
renderer features are opt-in style attributes, so neither app changes look unless it asks.

### Summary

- **dreist-gui**: opt-in looks for v1's knobs, buttons, faders, scroll bars, plots and menu
  buttons; resizing rebuilt so any resize path ends like a fresh layout (anchors, mounted
  skins, splitters); host resizing with a minimum size (vendored nih-plug); much faster
  repaints for menu hover, clicks and panel updates (only what changed is redrawn).
- **dreist-dsp / dreist-engine**: analog table reads floor like the originals (a shared
  porting bug, audible in the voice filters, the vintage path and the analog filter
  effects); v1 rules for the sequencer window, the timestretch pool and graph record.

### Features

- **gui:** Popup arrow box width option
- **gui:** Button glyphs, meter base band and range markers
- **gui:** Opt-in scroll bar frame, key strip inset, icon panels on parent state
- **gui:** Opt-in search box without magnifier and tick outline
- **gui:** Opt-in transmod slider with a modulation strip
- **gui:** Opt-in outer mod track on transmod knobs
- **gui:** Opt-in plot background and range marker bar
- **gui:** Opt-in looks for knobs, sliders and buttons
- **gui:** Opt-in dial shadow under knobs
- **rules:** V1 sequencer window and timestretch pool size
- **rules:** V1 graph record rewrites the current step until the wrap
- **gui:** A recording painter that fingerprints drawing calls
- **vendor:** Nih-plug lets the host resize an editor that has a minimum size

### Fixes

- **gui:** Resized splitters keep the far panel and clamp
- **gui:** Clamp a splitter's bar to its panel limits on resize
- **gui:** Layers drawn across a pixmap's left or top edge stay in place
- **dsp:** Analog table reads floor the index like the original
- **engine:** V1 graph record writes reach that sub-block's note
- **gui:** Anchored controls lay out from their basis, whatever the resize path
- **gui:** A mounted skin keeps filling its host as the host resizes
- **gui:** A resized splitter moves from the position last chosen

### Performance

- **gui:** Brushed noise only for the texels a clip shows
- **gui:** Menu hover redraws only the menu
- **gui:** Faster copy of the frame into the window buffer
- **gui:** Clicks and menus repaint only what they change
- **gui:** Code-drawn panels repaint only when their drawing changes
- **gui:** Fingerprint drawing calls as words, not formatted text

### Documentation

- Rebuild both apps' VST3s after a change
- Keep work branches local until a release

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
