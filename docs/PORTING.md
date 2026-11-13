# Porting map: Any2Odd 0.7.3 → Any2Odyssey

Every upstream source file and where its behaviour lives now. Status: **done**, **in progress**, **dropped** (with reason), **replaced**.

## Workspace layout

```
crates/core      any2odyssey-core      platform-neutral logic (all OSes)
crates/windows   any2odyssey-windows   Win32/DWM/WGC/D3D11, Samsung engine worker (Windows only)
crates/app       any2odyssey           the GUI executable (controls, tray, worker supervision; the Linux stereo viewer)
tools/iconmaker  any2odyssey-iconmaker brand asset generator
contrib/         Linux helpers for Leia's runtime: wine-lens (lens, eye tracker and weaver under Wine), lens-helper (MIT)
installer/       PowerShell installer family
scripts/         packaging (Windows ZIP, Linux archive and its third-party list)
packaging/       Linux desktop entry, the release archive's installer and the Arch Linux PKGBUILD
```

## C# (Windows application)

| Upstream file | Any2Odyssey | Status |
| --- | --- | --- |
| `Companion.cs` — `Files` (paths, JSON save/log/tail) | `core::paths`, `core::fsutil`, `core::log` | done |
| `Companion.cs` — `Preferences` | `core::prefs` (same JSON keys; Any2Odd's preferences are imported whenever Any2Odyssey has no settings file of its own, not just once) | done |
| `Companion.cs` — `Win` P/Invoke | `windows::win` | done (verified on the G90XF, 0.8.0) |
| `Companion.cs` — `Mirror` (offscreen capture surface, DWM thumbnails) | `windows::mirror` | done (verified on the G90XF, 0.8.0) |
| `Companion.cs` — `Engine` (validate, SaveOverlay, worker `Run`) | `core::engine` (hash pin, messages), `core::overlay_settings`, `windows::engine` (LoadLibrary/Start/Stop, worker loop) | done |
| `Companion.cs` — `MainForm` logic (worker start/stop, poll, recovery) | `core::supervisor` (decisions), `app` (process ownership) | done |
| `Companion.cs` — `Program` (args, single instance, named events) | `app::main`, `core::ipc`, `windows::ipc` | done (verified on the G90XF, 0.8.0) |
| `MainWindow.cs` | `app::ui::controls` | done (verified on the G90XF, 0.8.0) |
| `DesktopTheme.cs` | `core::theme` (palette), `app::ui::theme` (egui style, ornaments) | done |
| `DesktopControls.cs` (ResponsiveGrid, ScrollSurface, Gold* widgets) | `app::ui::widgets` | done (verified on the G90XF, 0.8.0) |
| `HelpContent.cs` + `Documentation/help.json` | `core::help` + `crates/core/src/help.json` (rebranded) | done |
| `DonationWindow.cs` | — | dropped (donation content removed) |
| `OverlayController.cs` | `windows::overlay` | done (verified on the G90XF, 0.8.0) |
| `RendererTaskbar.cs` | `windows::taskbar` | done (verified on the G90XF, 0.8.0) |
| `TaskbarMenu.cs` (WPF JumpList) | `windows::taskbar` (ICustomDestinationList) | done (verified on the G90XF, 0.8.0) |
| `Projection.cs` (layout probe) | `windows::diagnostics` | done (verified on the G90XF, 0.8.0) |
| `DesktopCapture.cs` (+ exclusion/offscreen tests) | `windows::capture`, `windows::diagnostics` | done (verified on the G90XF, 0.8.0) |
| `GpuDesktopCapture.cs` (D3D11, HDR→SDR shader) | `windows::capture::gpu` | done (verified on the G90XF, 0.8.0) |
| `GpuSurface.cs` (swap chain) | `windows::capture::surface` | done (verified on the G90XF, 0.8.0) |
| `DisplayColor.cs` (SDR white level) | `windows::display_color` | done (verified on the G90XF, 0.8.0) |
| `WgcFrameSource.cs` | `windows::capture::wgc` | done (verified on the G90XF, 0.8.0) |
| `IconMaker.cs` | `tools/iconmaker` | done |
| `tools/DocumentationScreenshots.cs` | `Any2Odyssey --screenshot FILE [--dark] [--help-page]` (isolated preview, no worker, no saved preferences) | replaced |

## C# tests

| Upstream | Any2Odyssey | Status |
| --- | --- | --- |
| `--self-test` geometry | `core::geometry` tests | done |
| `tests/SliderValidation.cs` | `core::slider` tests | done |
| `tests/RecoveryValidation.cs` | `core::supervisor` tests | done |
| `tests/UiValidation.cs` (layout at 980×780, stacking, scrolling) | `--screenshot` previews (no automated layout test) | replaced (layout verified by eye on the G90XF, 0.8.0) |
| `--test-capture-exclusion`, `--test-offscreen` | `windows::diagnostics` (same flags) | done (verified on the G90XF, 0.8.0) |

## Python (portable edition)

| Upstream | Any2Odyssey | Status |
| --- | --- | --- |
| `portable/stereo.py` | `core::stereo` | done |
| — (new; upstream's portable edition has no depth) | `core::dibr` (stereo pair from an image and its depth map), `app::depth` (Depth Anything V2 Small through ONNX Runtime) | in progress: still images and a live screen; run on the CPU under WSL 2 and on a GPU on a CachyOS desktop (`docs/PLATFORMS.md`) |
| `portable/app.py` | `app::ui::viewer` (image source, input and output formats, half-width, swap, preview, output window, PNG export), `app::platform::unix` (single instance, hand-over, file dialogs), `app::capture` and `app::live` (the live screen source, experimental) | in progress: the image viewer is done and run under WSL 2; the live screen source is there from 0.9.0 (`docs/LENS-SWITCH.md`); the tray icon is not started |
| `tests/test_portable.py` (stereo assertions) | `core::stereo` tests | done |
| `portable/run.sh`, `launch-linux.sh`, `Launch-macOS.command`, `requirements.txt`, `START-HERE.md` | — | dropped (native build replaces the Python bundles) |
| `tools/package-portable.py` | — | dropped |

## PowerShell / CMD

| Upstream | Any2Odyssey | Status |
| --- | --- | --- |
| `install.ps1`, `InstallSupport.ps1`, `Install-Launcher.ps1`, `Uninstall.ps1`, `Install.cmd`, `Install-GUI.cmd` | `installer/` (new identity, no dependency manifest) | done (installed on the G90XF PC, 0.8.0; tests run in CI under PowerShell 7 and 5.1) |
| `restore.ps1` (SharpDX NuGet) | — | dropped (no bundled DLLs) |
| `build.ps1` | `cargo build --release` | replaced |
| `package.ps1` | `scripts/package.ps1` | done (runs in CI and the release workflow) |
| `tests/test-installer.ps1`, `test-install-flow.ps1`, `test-controls-install.ps1`, `test-install-launcher.ps1` | `installer/tests/` (plus `test-uninstall-flow.ps1`, `test-uninstall-helper.ps1`, `test-dpi-manifest.ps1`) | done (CI runs them on `windows-latest` under PowerShell 7 and 5.1) |

## Documentation and assets

| Upstream | Any2Odyssey | Status |
| --- | --- | --- |
| `README.md`, `Documentation/README.md`, `PLATFORMS.md`, `TESTING.md`, `CHANGELOG.md`, `RELEASE-NOTES.md` | `README.md`, `docs/` | rewritten |
| `USER-GUIDE.html`, `tools/export-guide.py`, `tests/check-documentation.py` | offline guide exported from `help.json` | done (verified on the G90XF, 0.8.0) |
| `Documentation/DONATIONS.md`, `assets/donations/*`, `sponsor-button.svg`, `.github/FUNDING.yml` | — | dropped |
| `assets/Any2Odd.ico/.png/-wordmark.png` | `assets/Any2Odyssey.*` (regenerated) | done |
| `THIRD-PARTY-LICENSES.txt` | `THIRD-PARTY-LICENSES.md` | rewritten |
