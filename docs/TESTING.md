# Testing

## Automated (any OS)

```sh
cargo test --workspace            # on Windows: everything, including the Windows crate
cargo test -p any2odyssey-core    # platform-neutral logic
cargo test -p any2odyssey-iconmaker
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
```

The core tests port Any2Odd's checks: the `--self-test` geometry (fit, offscreen placement across monitor arrangements), `SliderValidation` (slider position/value/readout mapping), `RecoveryValidation` (queued retry cancelled by disabling recovery, re-enabling does not start, running worker left alone) and `test_portable.py`'s stereo assertions (anaglyph channel mapping, SBS eye swap, half-width geometry, flat pass-through, invalid input). Help content is checked for rebranding and the absence of donation material.

On Linux the Windows crate is only compile-checked: `cargo check --workspace --target x86_64-pc-windows-msvc`.

## Windows diagnostics (provided by the Windows crate)

Run these with conversion stopped. They are GUI executables; use `Start-Process -Wait -PassThru` to read the exit code.

- `Any2Odyssey.exe --self-test`: geometry and multi-monitor source-isolation checks (exit 0 on success).
- `Any2Odyssey.exe --test-capture-exclusion`: visible diagnostic windows check that the output is excluded from capture and that moved/closed content clears; result in `%LOCALAPPDATA%\Any2Odyssey\capture-exclusion-test.json`.
- `Any2Odyssey.exe --test-offscreen`: verifies a GPU surface outside every physical screen stays live and capturable; result in `offscreen-surface-test.json`.
- `Any2Odyssey.exe --inspect <pid>` and `--layout-probe <parentPid> <workerPid>`: window and projection diagnostics written as JSON to the data folder.

## Installer (Windows only)

Build the release, copy `target\release\Any2Odyssey.exe` to `build\`, then run from the repository root in 64-bit PowerShell:

```powershell
.\installer\tests\test-installer.ps1
.\installer\tests\test-install-launcher.ps1
.\installer\tests\test-controls-install.ps1
.\installer\tests\test-install-flow.ps1 [-PreviousPackage <extracted older release>]
```

They use temporary folders and an isolated `HKCU:\Software\Any2OdysseyInstallerTests` key; the Samsung engine hash and elevation are simulated. No real installation is performed and no Samsung file is read or written. CI runs the same scripts on `windows-latest`.

## Hardware acceptance (target machine)

Not automated. On the Windows PC with the G90XF, Odyssey 3D Hub 1.5.1 and the RTX GPU: full install, automatic start, each capture mode, SBS mode, depth/pop-out apply, Ctrl+Alt+F8/F9/F10, overlay behind a borderless game, recovery after killing the worker, uninstall. Record the results in `CHANGELOG.md` before tagging a release. Optical depth must be judged by a person at the monitor; screenshots cannot show it.

## UI previews

`Any2Odyssey --screenshot FILE [--dark] [--help-page]` opens the controls in an isolated preview (no single-instance guard, no worker, no saved preferences, hotkeys not registered), captures the window after a few frames and exits. The PNGs under `docs/assets/` were produced this way on Linux; they show the desktop controls, not optical 3D.
