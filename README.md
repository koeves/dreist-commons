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

## Highlights

- **One engine, two apps**: each app plugs in its names, resources, file family and effect nodes through an app profile.
- **Own editor per app**: panels, dialogs, settings and the effect menu live in each app.
- **Opens every family**: `.geist*`, `.dreist*` and `.einst*` files load in both apps.

## Contents

- [Crates](#crates)
- [App profile](#app-profile)
- [Platforms](#platforms)
- [Diagnostics](#diagnostics)
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

## Platforms

- **macOS**: 11 or newer, Apple Silicon.
- **Windows**: 10 or newer, x64.
- **Linux**: x86_64, glibc 2.39 or newer (Ubuntu 24.04, Fedora 40); X11, Wayland through XWayland.
- **Linux backends**: X11 windows (baseview, softbuffer), ALSA audio and MIDI (cpal, midir), XDG portal file dialogs (rfd).
- **macOS only**: native drags out of the window, the app-wide menu bar and REX files.

Linux build packages (Debian / Ubuntu names; Clang builds the time-stretch bindings, the rest serve `dreist-gui/window` and the `dreist-host` features):

```bash
sudo apt-get install build-essential clang libclang-dev pkg-config libasound2-dev libjack-jackd2-dev libx11-dev libx11-xcb-dev libxcb1-dev libxcursor-dev libgl1-mesa-dev
```

## Diagnostics

Every app and plugin keeps a diagnostics log (`dreist_engine::diag`), and the main menu exports it as one file to share.

- **Log**: `~/Library/Logs/<App>/<app>.log` (macOS), `$XDG_STATE_HOME/<App>/<app>.log` (Linux), `%LOCALAPPDATA%\<App>\Logs\<app>.log` (Windows); 5 MB, three older files kept.
- **Shared**: one log per app; with several plugin instances in a host, the first one's audio thread reports.
- **Audio thread**: block timing, the song position and late blocks go through atomics only, without locks or allocations.
- **Line format**: `<UTC time> +<seconds>s <kind> key=value ...`, values with spaces in double quotes.
- **Kinds**: `start` (system, versions, pid), `audio` (format, host, sample rate, block size), `stats` (DSP load %, worst block %, late blocks, memory, output peak), `late` and `slow` blocks (song position, timing), `summary`.
- **Stats**: every second while playing or after a late block, else once a minute.
- **Export**: "Export diagnostics" writes `<app>-diagnostics-<time>.txt` next to the log and shows it; "Copy diagnostics" puts the same text on the clipboard.
- **Export format**: `# system`, `# audio`, `# run`, `# settings` and `# log` sections of `key=value` or log lines.
- **Redaction**: paths outside the app's settings and log folders become `<path>`, the login name `<user>`.
- **Debug runs**: `--debug` echoes the log to stderr (`--debug-json`: JSON lines) with stats every second; `--play`, `--duration`, `--until-end` and `--fail-on-late` drive playback tests.

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
