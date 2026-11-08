# Porting map: Any2Odd 0.7.3 → Any2Odyssey

Every upstream source file and where its behaviour lives now. Status: **done**, **in progress**, **dropped** (with reason), **replaced**.

## Workspace layout

```
crates/core      any2odyssey-core      platform-neutral logic (all OSes)
crates/windows   any2odyssey-windows   Win32/DWM/WGC/D3D11, Samsung engine worker (Windows only)
crates/app       any2odyssey           the GUI executable (controls, tray, worker supervision; Linux portable features)
tools/iconmaker  any2odyssey-iconmaker brand asset generator
installer/       PowerShell installer family
scripts/         packaging
```

## C# (Windows application)

| Upstream file | Any2Odyssey | Status |
| --- | --- | --- |
| `Companion.cs` — `Files` (paths, JSON save/log/tail) | `core::paths`, `core::fsutil`, `core::log` | done |
| `Companion.cs` — `Preferences` | `core::prefs` (same JSON keys; one-time Any2Odd import) | done |
| `Companion.cs` — `Win` P/Invoke | `windows::win` | done (awaiting G90XF hardware test) |
| `Companion.cs` — `Mirror` (offscreen capture surface, DWM thumbnails) | `windows::mirror` | done (awaiting G90XF hardware test) |
| `Companion.cs` — `Engine` (validate, SaveOverlay, worker `Run`) | `core::engine` (hash pin, messages), `core::overlay_settings`, `windows::engine` (LoadLibrary/Start/Stop, worker loop) | done |
| `Companion.cs` — `MainForm` logic (worker start/stop, poll, recovery) | `core::supervisor` (decisions), `app` (process ownership) | done |
| `Companion.cs` — `Program` (args, single instance, named events) | `app::main`, `core::ipc`, `windows::ipc` | done (awaiting G90XF hardware test) |
| `MainWindow.cs` | `app::ui::controls` | done (awaiting G90XF hardware test) |
| `DesktopTheme.cs` | `core::theme` (palette), `app::ui::theme` (egui style, ornaments) | done |
| `DesktopControls.cs` (ResponsiveGrid, ScrollSurface, Gold* widgets) | `app::ui::widgets` | done (awaiting G90XF hardware test) |
| `HelpContent.cs` + `Documentation/help.json` | `core::help` + `crates/core/src/help.json` (rebranded) | done |
| `DonationWindow.cs` | — | dropped (donation content removed) |
| `OverlayController.cs` | `windows::overlay` | done (awaiting G90XF hardware test) |
| `RendererTaskbar.cs` | `windows::taskbar` | done (awaiting G90XF hardware test) |
| `TaskbarMenu.cs` (WPF JumpList) | `windows::jumplist` (ICustomDestinationList) | done (awaiting G90XF hardware test) |
| `Projection.cs` (layout probe) | `windows::diagnostics` | done (awaiting G90XF hardware test) |
| `DesktopCapture.cs` (+ exclusion/offscreen tests) | `windows::capture`, `windows::diagnostics` | done (awaiting G90XF hardware test) |
| `GpuDesktopCapture.cs` (D3D11, HDR→SDR shader) | `windows::capture::gpu` | done (awaiting G90XF hardware test) |
| `GpuSurface.cs` (swap chain) | `windows::capture::surface` | done (awaiting G90XF hardware test) |
| `DisplayColor.cs` (SDR white level) | `windows::display_color` | done (awaiting G90XF hardware test) |
| `WgcFrameSource.cs` | `windows::capture::wgc` | done (awaiting G90XF hardware test) |
| `IconMaker.cs` | `tools/iconmaker` | done |
| `tools/DocumentationScreenshots.cs` | `Any2Odyssey --screenshot FILE [--dark] [--help-page]` (isolated preview, no worker, no saved preferences) | replaced |

## C# tests

| Upstream | Any2Odyssey | Status |
| --- | --- | --- |
| `--self-test` geometry | `core::geometry` tests | done |
| `tests/SliderValidation.cs` | `core::slider` tests | done |
| `tests/RecoveryValidation.cs` | `core::supervisor` tests | done |
| `tests/UiValidation.cs` (layout at 980×780, stacking, scrolling) | `app` layout tests | done (awaiting G90XF hardware test) |
| `--test-capture-exclusion`, `--test-offscreen` | `windows::diagnostics` (same flags) | done (awaiting G90XF hardware test) |

## Python (portable edition)

| Upstream | Any2Odyssey | Status |
| --- | --- | --- |
| `portable/stereo.py` | `core::stereo` | done |
| `portable/app.py` | Linux feature set of `app` | planned (phase 3) |
| `tests/test_portable.py` (stereo assertions) | `core::stereo` tests | done |
| `portable/run.sh`, `launch-linux.sh`, `Launch-macOS.command`, `requirements.txt`, `START-HERE.md` | — | dropped (native build replaces the Python bundles) |
| `tools/package-portable.py` | — | dropped |

## PowerShell / CMD

| Upstream | Any2Odyssey | Status |
| --- | --- | --- |
| `install.ps1`, `InstallSupport.ps1`, `Install-Launcher.ps1`, `Uninstall.ps1`, `Install.cmd`, `Install-GUI.cmd` | `installer/` (new identity, no dependency manifest) | done, untested on Windows |
| `restore.ps1` (SharpDX NuGet) | — | dropped (no bundled DLLs) |
| `build.ps1` | `cargo build --release` | replaced |
| `package.ps1` | `scripts/package.ps1` | done, untested on Windows |
| `tests/test-installer.ps1`, `test-install-flow.ps1`, `test-controls-install.ps1`, `test-install-launcher.ps1` | `installer/tests/` | ported, untested on Windows |

## Documentation and assets

| Upstream | Any2Odyssey | Status |
| --- | --- | --- |
| `README.md`, `Documentation/README.md`, `PLATFORMS.md`, `TESTING.md`, `CHANGELOG.md`, `RELEASE-NOTES.md` | `README.md`, `docs/` | rewritten |
| `USER-GUIDE.html`, `tools/export-guide.py`, `tests/check-documentation.py` | offline guide exported from `help.json` | done (awaiting G90XF hardware test) |
| `Documentation/DONATIONS.md`, `assets/donations/*`, `sponsor-button.svg`, `.github/FUNDING.yml` | — | dropped |
| `assets/Any2Odd.ico/.png/-wordmark.png` | `assets/Any2Odyssey.*` (regenerated) | done |
| `THIRD-PARTY-LICENSES.txt` | `THIRD-PARTY-LICENSES.md` | rewritten |
