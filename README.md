# dreist-commons

The shared crates of **Dreist** (a Rust rebuild of FXpansion Geist2) and **Einst** (a rebuild of
Geist v1 on top of them). Each app depends on these crates, plugs in its names, resources, file
family and effect nodes through an app profile, and brings its own editor.

| Crate | Role |
|---|---|
| `dreist-formats` | document tree, project/kit/preset readers, effect tables, resource paths, the app profile (`profile`) and the legacy on-disk names (`legacy`) |
| `dreist-dsp` | sampler voice DSP and the effect nodes; `fx::install_factory` lets an app supply its own nodes |
| `dreist-engine` | engine runtime: step and song sequencer, mixing, offline render, MIDI; `harness` runs the engine goldens |
| `dreist-gui` | skin-driven GUI toolkit (tiny-skia, baseview) |
| `dreist-testkit` | golden fingerprints and allocation counting for tests |
| `dreist-host` | the system side: audio devices (cpal), MIDI ports (midir), file dialogs (rfd), the standalone's relaunch session, local time; owns those dependencies (features `audio`, `midi`, `dialog`, `session`, `clap-sys`) |

`vendor/` holds patched `baseview` (re-entrancy guard for modal dialogs; background mode for
tests on macOS and X11), `softbuffer` (a per-library macOS observer class name), `nih-plug` and
`signalsmith-stretch` (fixed phase-randomisation seed). Apps add them as `[patch]` entries.

## The app profile

    dreist_formats::profile::install(&PROFILE);            // names, resources, files, logo
    dreist_dsp::fx::install_factory(NodeFactory { .. });   // optional: the app's own effect nodes

`Profile` holds the product name, the default theme, the UI and icon font families, the
header logo's mark, where the UI graphics and the factory data are found (environment override,
bundle folder, development folders) and the extension family the app saves (`LEGACY` `.geist*`,
`DREIST` `.dreist*`, `EINST` `.einst*`; every app opens all three). Without an installed
profile, `BASE` (Dreist's) applies. The editor (panels, dialogs, settings, effect menu, document
readers) is each app's own.

## Development

Tests and goldens here run under `BASE` and draw Dreist's UI from a Dreist checkout next to this
repository (`../dreist`; `DREIST_UI` / `DREIST_RESOURCES` override); tests that need it are
skipped without one, and Dreist's CI runs them against its assets. Demo-content tests are
skipped without the factory content (`DREIST_CONTENT` overrides its location).

    cargo test --workspace
    cargo clippy --workspace --all-targets -- -D warnings
    cargo fmt --all --check
    UPDATE_GOLDEN=1 cargo test ...     # rewrite goldens; explain every change in the commit

Platforms: macOS (Apple Silicon), Windows (x64) and Linux (x86_64, X11). On Linux the window
code uses X11 (baseview, softbuffer), the host crate ALSA (cpal, midir) and the XDG desktop
portal for file dialogs (rfd, no GTK); settings folders are under `$XDG_CONFIG_HOME` (else
`~/.config`). Building with `dreist-gui/window` or the `dreist-host` features there needs
(Debian / Ubuntu names):

    sudo apt-get install pkg-config libasound2-dev libx11-dev libx11-xcb-dev libxcb1-dev libxcursor-dev libgl1-mesa-dev

Native drags out of the window and the app-wide menu bar are macOS only; REX files load only on
macOS (the REX helper).

## Contributing

- Commits follow Conventional Commits (`feat(scope): ...`); the changelog and release notes
  are generated from them ([CONTRIBUTING.md](CONTRIBUTING.md)).
- Checks: the commands under Development.
- Dreist and Einst include this repository as a submodule at `commons`.

## Related projects

- [dreist-commons](https://github.com/koeves/dreist-commons): the shared library (formats, DSP,
  engine, GUI toolkit) that both apps are built on.
- [Dreist](https://github.com/koeves/dreist): the Geist2 rebuild, an app on dreist-commons.
- [Einst](https://github.com/koeves/einst): the Geist v1 rebuild, an app on dreist-commons.

## Licence

MIT OR Apache-2.0 (LICENSE). Not affiliated with or endorsed by FXpansion.
