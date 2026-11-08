# Platform support

| Capability | Windows x64 | Linux (planned) |
| --- | --- | --- |
| Light/dark controls, Help | Native, custom chrome (egui) | Same controls, native window chrome |
| Tray / menu | Windows notification icon, jump list | Tray where the desktop supports it (StatusNotifier/AppIndicator); taskbar fallback |
| Ordinary 2D → AI depth | Installed Samsung engine | Unavailable; 2D stays flat |
| Existing SBS input | Samsung/Leia calibrated output | SBS viewing or red/cyan anaglyph; optional eye swap; PNG export |
| G90XF tracking, calibration and lens output | Samsung/Leia runtime required | Not available |
| Whole-desktop source | Windows Graphics Capture | Screen capture through the portal/X11; a separate output display is required; Wayland live output disabled until exclusion is verified |
| Selected / follow-active window | DWM thumbnails | Not planned |
| Click-through output | Win32 layered + transparent styles | Compositor-dependent; to be verified on hardware |
| Global shortcuts, automatic start, recovery | Yes | Local shortcuts only; output starts manually |
| Installer | Version-aware PowerShell installer | Archive or distribution package; no installer |

## What is distributed

- Windows x64: `Any2Odyssey-<version>-windows-x64.zip` with the executable and installer (built by CI on `windows-latest`).
- Linux: not yet. The CI builds the core, the icon tool and the application crate on x86_64 and aarch64 Linux to keep the port honest, but no Linux package is published until the portable features exist and have been run on a real desktop.

## Why Samsung's engine cannot be used on Linux

Conversion is done by Samsung's `ConversionWrapper.dll` (CUDA/ONNX, Windows x64) and the calibrated glasses-free output needs the LeiaSR runtime, eye tracking from the monitor's camera and the lens calibration. None of these exist for Linux. Running them under Wine/Proton is unvalidated and outside this project's scope. Any2Odyssey's Linux target is therefore the stereo viewer and capture feature set described above.

## Development machines

The maintainer develops on an aarch64 Linux laptop (cross-checks the Windows code with `cargo check --target x86_64-pc-windows-msvc`) and tests the Windows converter on an x64 Windows PC with a G90XF. Hardware results are recorded in `CHANGELOG.md` and `docs/TESTING.md`.
