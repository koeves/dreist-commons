# Packaging

Install scripts the apps ship in their release archives: `install.sh` (Linux, macOS) and `install.ps1` (Windows).

- Each script installs the standalone app, VST3 and CLAP that sit next to it, and finds the app's name from them.
- The same options everywhere: `--user`, `--system` (all users), `--uninstall`, `--app <Name>` (uninstall without the files), `--help`.
- Default: all users on Windows (administrator PowerShell), this user on Linux and macOS.
- Every part is checked to be a complete build before anything installed is touched; a reinstall replaces the old version whole.
- Settings and content stay on uninstall.
- `DESTDIR=<dir>` (install.sh) stages the layout under `<dir>` for packages and tests.

## Locations

This table is the definition; the scripts, package manifests and app READMEs follow it. `<App>` is the app's name, `<app>` the same in lower case.

### macOS

| Part | This user (default) | `--system` |
|---|---|---|
| VST3 | `~/Library/Audio/Plug-Ins/VST3/<App>.vst3` | `/Library/Audio/Plug-Ins/VST3/<App>.vst3` |
| CLAP | `~/Library/Audio/Plug-Ins/CLAP/<App>.clap` | `/Library/Audio/Plug-Ins/CLAP/<App>.clap` |
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

| System | Next to the script |
|---|---|
| macOS | `<App>.vst3`, `<App>.clap`, `<App>.app` |
| Windows | `<App>.vst3`, `<App>-CLAP\` (`<App>.clap`, `Resources\`), `<App>-App\` (`<App>.exe`, `Resources\`) |
| Linux | `<App>.vst3`, `<App>-CLAP/` (`<App>.clap`, `Resources/`), `<App>-App/` (`<App>`, `Resources/`, `<app>.desktop`, `icons/`) |

## Tests

    sh packaging/test-install.sh        # Linux, macOS: fake app in a scratch home and DESTDIR
    pwsh packaging/test-install.ps1     # Windows: installs a fake app for real (CI runners)
