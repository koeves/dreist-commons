<div align="center">

# dreist-commons

[![Release](https://img.shields.io/github/v/release/koeves/dreist-commons)](https://github.com/koeves/dreist-commons/releases/latest)
[![CI](https://github.com/koeves/dreist-commons/actions/workflows/ci.yml/badge.svg)](https://github.com/koeves/dreist-commons/actions/workflows/ci.yml)
[![CodeQL](https://github.com/koeves/dreist-commons/actions/workflows/codeql.yml/badge.svg)](https://github.com/koeves/dreist-commons/actions/workflows/codeql.yml)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue)](LICENSE)
[![Platforms](https://img.shields.io/badge/platforms-macOS%20%7C%20Windows%20%7C%20Linux-lightgrey)](#platforms)

</div>

The shared crates of [Dreist](https://github.com/koeves/dreist) (a Rust rebuild of FXpansion Geist2)
and [Einst](https://github.com/koeves/einst) (a rebuild of Geist v1).

## Highlights

- **One engine, two apps**: each app plugs in its names, resources, file family and effect nodes through an app profile.
- **Own editor per app**: panels, dialogs, settings and the effect menu live in each app.
- **Opens every family**: `.geist*`, `.dreist*` and `.einst*` files load in both apps.

## Contents

- [Crates](#crates)
- [App profile](#app-profile)
- [Platforms](#platforms)
- [Development](#development)
- [Contributing](#contributing)
- [Related projects](#related-projects)
- [Licence](#licence)

## Crates

| Crate | Role |
|---|---|
| `dreist-formats` | Document tree, project / kit / preset readers, effect tables, resource paths, the app profile and the legacy on-disk names |
| `dreist-dsp` | Sampler voice DSP and the effect nodes; `fx::install_factory` lets an app supply its own nodes |
| `dreist-engine` | Step and song sequencer, mixing, offline render, MIDI; `harness` runs the engine goldens |
| `dreist-gui` | Skin-driven GUI toolkit (tiny-skia, baseview) |
| `dreist-testkit` | Golden fingerprints and allocation counting for tests |
| `dreist-host` | Audio devices (cpal), MIDI ports (midir), file dialogs (rfd), the standalone's session; features `audio`, `midi`, `dialog`, `session`, `clap-sys` |

`vendor/` holds patched dependencies, added by the apps as `[patch]` entries:

- **`baseview`**: re-entrancy guard for modal dialogs, background mode for tests on macOS and X11.
- **`softbuffer`**: a per-library macOS observer class name.
- **`nih-plug`**: the plugin framework, with fixes for the standalone's audio setup.
- **`signalsmith-stretch`**: a fixed phase-randomisation seed, the same on every platform.

## App profile

```rust
dreist_formats::profile::install(&PROFILE);            // names, resources, files, logo
dreist_dsp::fx::install_factory(NodeFactory { .. });   // optional: the app's own effect nodes
```

- **`Profile`**: product name, default theme, UI and icon fonts, header logo, resource locations and the saved file family.
- **File families**: `LEGACY` (`.geist*`), `DREIST` (`.dreist*`), `EINST` (`.einst*`).
- **Default**: without an installed profile, `BASE` (Dreist's) applies.

## Platforms

- **macOS**: 11 or newer, Apple Silicon.
- **Windows**: 10 or newer, x64.
- **Linux**: x86_64, glibc 2.39 or newer (Ubuntu 24.04, Fedora 40); X11, Wayland through XWayland.
- **Linux backends**: X11 windows (baseview, softbuffer), ALSA audio and MIDI (cpal, midir), XDG portal file dialogs (rfd).
- **macOS only**: native drags out of the window, the app-wide menu bar and REX files.

Linux build packages for `dreist-gui/window` and the `dreist-host` features (Debian / Ubuntu names):

```bash
sudo apt-get install pkg-config libasound2-dev libx11-dev libx11-xcb-dev libxcb1-dev libxcursor-dev libgl1-mesa-dev
```

## Development

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
UPDATE_GOLDEN=1 cargo test ...     # rewrite goldens; explain every change in the commit
```

- **UI for tests**: drawn from a Dreist checkout next to this one (`../dreist`; `DREIST_UI` / `DREIST_RESOURCES` override).
- **Skipped tests**: those needing the UI skip without it; Dreist's CI runs them against its assets.
- **Content tests**: skip without the factory content (`DREIST_CONTENT` sets its location).

## Contributing

- Commits, checks, branches and releases: [CONTRIBUTING.md](CONTRIBUTING.md).
- Dreist and Einst include this repository as a submodule at `commons`.

## Related projects

- [Dreist](https://github.com/koeves/dreist): the Geist2 rebuild on these crates.
- [Einst](https://github.com/koeves/einst): the Geist v1 rebuild on these crates.

## Licence

- MIT OR Apache-2.0 ([LICENSE](LICENSE)).
- Not affiliated with or endorsed by FXpansion.
