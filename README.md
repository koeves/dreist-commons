<div align="center">

# dreist-commons

[![Release](https://img.shields.io/github/v/release/koeves/dreist-commons)](https://github.com/koeves/dreist-commons/releases/latest)
[![CI](https://github.com/koeves/dreist-commons/actions/workflows/ci.yml/badge.svg?branch=dev)](https://github.com/koeves/dreist-commons/actions/workflows/ci.yml?query=branch%3Adev)
[![CodeQL](https://github.com/koeves/dreist-commons/actions/workflows/codeql.yml/badge.svg?branch=main)](https://github.com/koeves/dreist-commons/actions/workflows/codeql.yml?query=branch%3Amain)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue)](LICENSE)
[![Platforms](https://img.shields.io/badge/platforms-macOS%20%7C%20Windows%20%7C%20Linux-lightgrey)](#platforms)

</div>

The shared crates of [Dreist](https://github.com/koeves/dreist) (a Rust rebuild of FXpansion Geist2)
and [Einst](https://github.com/koeves/einst) (a rebuild of Geist v1).

## Features

- **One engine, two apps**: each app plugs in its names, resources, file family and effect nodes through an app profile.
- **Own editor per app**: panels, dialogs, settings and the effect menu live in each app.
- **Opens every family**: `.geist*`, `.dreist*` and `.einst*` files load in both apps.
- **Diagnostics**: every app and plugin keeps a log (`dreist_engine::diag`) and exports a shareable copy without the user name and private paths.

## Crates

| Crate | Role |
|---|---|
| `dreist-formats` | Document tree, project / kit / preset readers, effect tables, resource paths, the app profile and the legacy on-disk names |
| `dreist-dsp` | Sampler voice DSP and the effect nodes, which an app can extend with its own (`fx::install_factory`) |
| `dreist-engine` | Step and song sequencer, mixing, offline render, MIDI |
| `dreist-gui` | Skin-driven GUI toolkit (tiny-skia, baseview) |
| `dreist-testkit` | Golden fingerprints and allocation counting for tests |
| `dreist-editor-core` | Editor parts the apps share: commands to the engine, undo history, options, tab panels, effect value texts, graphs |
| `dreist-host` | Audio devices (cpal), MIDI ports (midir), file dialogs (rfd), the standalone's session |

`vendor/` holds patched dependencies, added by the apps as `[patch]` entries:

- **`baseview`**: re-entrancy guard for modal dialogs, background mode for tests on macOS and X11, keys with Command for a text box being edited.
- **`softbuffer`**: a per-library macOS observer class name.
- **`nih-plug`**: the plugin framework, with fixes for the standalone's audio setup.
- **`nih_log`**: nih-plug's logger, with `std::io::IsTerminal` in place of the unmaintained `atty`.
- **`signalsmith-stretch`**: a fixed phase-randomisation seed, the same on every platform.

`packaging/` holds the install scripts the apps ship in their release archives, one behaviour on every system ([packaging/README.md](packaging/README.md)).

## App profile

```rust
dreist_formats::profile::install(&PROFILE);            // names, resources, files, logo
dreist_dsp::fx::install_factory(NodeFactory { .. });   // optional: the app's own effect nodes
```

- **`Profile`**: product name, default theme, UI and icon fonts, header logo, resource locations and the saved file family.
- **File families**: `LEGACY` (`.geist*`), `DREIST` (`.dreist*`), `EINST` (`.einst*`).
- **Default**: without an installed profile, `BASE` (Dreist's) applies.

## Using it in an app

- **Submodule**: Dreist and Einst include this repository at `commons`, pinned to a release tag.
- **Dependencies**: the crates by path (`commons/crates/...`) and `vendor/` as `[patch]` entries.

## Platforms

- **macOS**: 11 or newer, Apple Silicon or Intel.
- **Windows**: 10 or newer, x64.
- **Linux**: x86_64, glibc 2.39 or newer (Ubuntu 24.04, Fedora 40), X11 or XWayland.

## Limitations

- **Drags out of the window**: macOS only.
- **Drops into the window**: macOS and Windows only.
- **REX files**: macOS only.

## Building and testing

```bash
cargo test --workspace
cargo clippy --workspace --all-targets --features dreist-gui/window -- -D warnings
cargo fmt --all --check
```

- **Tools**: Rust through [rustup](https://rustup.rs), plus the Xcode Command Line Tools on macOS and Visual Studio Build Tools (C++) with LLVM on Windows.
- **Linux packages** (Debian / Ubuntu): `sudo apt-get install build-essential clang libclang-dev pkg-config libasound2-dev libjack-jackd2-dev libx11-dev libx11-xcb-dev libxcb1-dev libxcursor-dev libgl1-mesa-dev`.
- **UI and content for tests**: taken from a Dreist checkout next to this one (`../dreist`).
- **Goldens**: `UPDATE_GOLDEN=1 cargo test ...` rewrites them.
- **Contributing**: [CONTRIBUTING.md](CONTRIBUTING.md).

## Related projects

- [Dreist](https://github.com/koeves/dreist): the Geist2 rebuild on these crates.
- [Einst](https://github.com/koeves/einst): the Geist v1 rebuild on these crates.

## Licence

- MIT OR Apache-2.0 ([LICENSE](LICENSE)).
- Not affiliated with or endorsed by FXpansion.
