# Progress

## 2026-11-03: dreist-commons created

- The shared crates moved out of Dreist (from Dreist beta 1) and were renamed `dreist-*`; the
  engine type is `Dreist`, environment variables are `DREIST_*`.
- App profile (`dreist_formats::profile`), effect-node factory (`dreist_dsp::fx::install_factory`),
  golden harnesses for apps (`dreist_engine::harness::check_scenarios`,
  `dreist_editor::screens::check_screens`).
- Files: three extension families (legacy `.geist*`, `.dreist*`, `.einst*`); every app opens all
  of them and saves its own. A project of another family opens without a save path, so Save goes
  through Save As in the app's family.
- Settings: the profile's folder; an earlier folder name is moved on first use.
- Reverse-engineering references are evidence tags; legacy on-disk names are constants in
  `dreist_formats::legacy`; port tables are named by effect.
- Porting fix: Modulated Filter computes one FM coefficient per sample from the left input, as
  the original does (fx golden `kind29-one` changed).

Goldens: identical to Dreist beta 1's except `fx.txt kind29-one` (the fix above).

## Next

- Keep fixing porting bugs found while comparing Einst's v1 effects (each in its own commit).
