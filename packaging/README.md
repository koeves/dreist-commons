# Packaging

Install scripts the apps ship in their release archives: `install.sh` (Linux, macOS) and `install.ps1` (Windows).
The Audio Unit build for macOS is in `auv2/`.

- Each script installs the standalone app, VST3, CLAP and (macOS) Audio Unit that sit next to it, and finds the app's name from them.
- The same options everywhere: `--user`, `--system` (all users), `--uninstall`, `--purge` (with `--uninstall`: the diagnostics logs too), `--app <Name>` (uninstall without the files), `--help`.
- Default: all users on Windows (administrator PowerShell), this user on Linux and macOS.
- Every part is checked to be a complete build before anything installed is touched; a reinstall replaces the old version whole.
- macOS: a build for the other kind of Mac (Apple Silicon or Intel) is refused and the right archive named; Rosetta counts as Apple Silicon.
- Settings, content and the diagnostics logs stay on uninstall; `--purge` removes the logs of the user running it.
- `DESTDIR=<dir>` (install.sh) stages the layout under `<dir>` for packages and tests.

## Locations

This table is the definition; the scripts, package manifests and app READMEs follow it. `<App>` is the app's name, `<app>` the same in lower case.

### macOS

| Part | This user (default) | `--system` |
|---|---|---|
| VST3 | `~/Library/Audio/Plug-Ins/VST3/<App>.vst3` | `/Library/Audio/Plug-Ins/VST3/<App>.vst3` |
| CLAP | `~/Library/Audio/Plug-Ins/CLAP/<App>.clap` | `/Library/Audio/Plug-Ins/CLAP/<App>.clap` |
| Audio Unit | `~/Library/Audio/Plug-Ins/Components/<App>.component` | `/Library/Audio/Plug-Ins/Components/<App>.component` |
| App | `/Applications/<App>.app` (`~/Applications` without write access) | `/Applications/<App>.app` |
| Settings | `~/Library/Application Support/<App>` | same, per user |

### Windows

| Part | `--user` | All users (default) |
|---|---|---|
| VST3 | `%LOCALAPPDATA%\Programs\Common\VST3\<App>.vst3` | `%CommonProgramFiles%\VST3\<App>.vst3` |
| CLAP | `%LOCALAPPDATA%\Programs\Common\CLAP\<App>\` | `%CommonProgramFiles%\CLAP\<App>\` |
| App | `%LOCALAPPDATA%\Programs\<App>\` | `%ProgramFiles%\<App>\` |
| Start menu | `<App>.lnk` in the user's Programs | `<App>.lnk` in all users' Programs |
| Settings | `%APPDATA%\<App>` | same, per user |

### Linux

| Part | This user (default) | `--system` |
|---|---|---|
| VST3 | `~/.vst3/<App>.vst3` | `/usr/local/lib/vst3/<App>.vst3` |
| CLAP | `~/.clap/<App>/` | `/usr/lib/clap/<App>/` |
| App | `~/.local/share/<App>/`, command `~/.local/bin/<app>` | `/opt/<App>/`, command `/usr/local/bin/<app>` |
| Launcher, icons | `~/.local/share/applications`, `~/.local/share/icons/hicolor` | `/usr/local/share/applications`, `/usr/local/share/icons/hicolor` |
| Settings | `~/.config/<App>` | same, per user |

- `XDG_DATA_HOME` and `XDG_CONFIG_HOME` replace `~/.local/share` and `~/.config` when set.
- System-wide CLAPs go to `/usr/lib/clap`, the system folder the CLAP standard names.

## Archive layout

One archive per system, `<App>-v<version>-<system>.zip` (Linux `.tar.gz`, keeping executable bits), unpacking into a folder of that name.

| System | Next to the script |
|---|---|
| `macOS-AppleSilicon`, `macOS-Intel` | `<App>.vst3`, `<App>.clap`, `<App>.component`, `<App>.app` |
| `Windows` | `<App>.vst3`, `<App>-CLAP\` (`<App>.clap`, `Resources\`), `<App>-App\` (`<App>.exe`, `Resources\`) |
| `Linux` | `<App>.vst3`, `<App>-CLAP/` (`<App>.clap`, `Resources/`), `<App>-App/` (`<App>`, `Resources/`, `<app>.desktop`, `icons/`) |

## Audio Unit

`auv2/build.sh` wraps an app's CLAP as an AUv2 `.component` for Logic Pro and other Audio Unit hosts.

- Built with [clap-wrapper](https://github.com/free-audio/clap-wrapper) v0.18.0 (MIT), Apple's [AudioUnitSDK](https://github.com/apple/AudioUnitSDK) 1.1.0 (Apache-2.0) and the [CLAP SDK](https://github.com/free-audio/clap) 1.2.10 (MIT), each fetched at a pinned commit.
- The `.component` carries the app's `.clap` in `Contents/PlugIns` and runs it from there.
- AU type `aumu` (instrument), manufacturer `Drst` (Dreist), one subtype per app.
- Universal like the CLAP it wraps, signed ad-hoc or with `--sign <identity>` (hardened runtime and timestamp).
- Needs Xcode's command line tools, CMake and git.

    packaging/auv2/build.sh --clap target/bundled/<App>.clap --subtype <code> --bundle-id <id>

## Tests

    sh packaging/test-install.sh        # Linux, macOS: fake app in a scratch home and DESTDIR
    packaging/auv2/test.sh              # macOS: a test CLAP as a universal Audio Unit, installed and checked with auval
    pwsh packaging/test-install.ps1     # Windows: installs a fake app for real (CI runners)
