# Changelog

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
