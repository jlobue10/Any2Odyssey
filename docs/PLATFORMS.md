# Platform support

| Capability | Windows x64 | Linux |
| --- | --- | --- |
| Light/dark controls, Help | Native, custom chrome (egui) | Same chrome and Help; the home page is the stereo viewer |
| Tray / menu | Windows notification icon, jump list | No tray icon yet (planned: StatusNotifier where the desktop supports it); Minimize and Close act normally |
| Ordinary 2D → AI depth | Installed Samsung engine | Unavailable; 2D stays flat |
| Existing SBS input | Samsung/Leia calibrated output | Stereo viewer: an SBS image as red/cyan anaglyph or SBS, half-width stretch, eye swap, preview, output window with full screen, PNG export |
| G90XF tracking, calibration and lens output | Samsung/Leia runtime required | Not available |
| Whole-desktop source | Windows Graphics Capture | Not yet. Planned: a bounded capture of one screen through the portal/X11, shown on a different display; Wayland stays disabled until source/output exclusion is verified |
| Selected / follow-active window | DWM thumbnails | Not planned |
| Click-through output | Win32 layered + transparent styles | No: the output is an ordinary window you place and make full screen |
| Global shortcuts, automatic start, recovery | Yes | Local shortcuts only (Ctrl+Alt+F10 output, F1 Help, F11/Esc in the output window); output starts manually |
| One instance, `--show` / `--quit` | Named mutex and events | A Unix socket in `$XDG_RUNTIME_DIR`; a second launch hands over its request or file |
| Update check | GitHub Releases, on request or daily | No |
| Installer | Version-aware PowerShell installer | Arch Linux package built from the tree (`packaging/arch/PKGBUILD`); no published package |

## What is distributed

- Windows x64: `Any2Odyssey-<version>-windows-x64.zip` with the executable and installer (built by CI on `windows-latest`).
- Linux: nothing is published yet. Arch-based distributions (Arch, CachyOS, EndeavourOS, Manjaro) build and install a package from a checkout:

  ```sh
  git clone https://github.com/jlobue10/Any2Odyssey.git
  cd Any2Odyssey/packaging/arch
  makepkg -si
  ```

  This installs `/usr/bin/any2odyssey`, a desktop entry (it appears as an "Open with" choice for PNG, JPEG and BMP images), the icon, the guide and the license files. `zenity` or `kdialog` provides the Open and Save dialogs; without either, type the path or drop the file on the window. Other distributions: `cargo build --release -p any2odyssey` and run `target/release/Any2Odyssey`.
- The CI builds and tests the core, the icon tool and the application crate on x86_64 and aarch64 Linux, and builds the Arch package in an Arch Linux container.

## What has been run on Linux

The stereo viewer was built and run on Arch Linux under WSL 2 (x86_64) with the package above: X11 under a virtual display with the Openbox window manager, software OpenGL. Checked there: opening an image by argument and by hand-over from a second launch, the anaglyph preview, the refusal of an odd-width pair, the output window, full screen and Esc, `--show` and `--quit`. **Not yet run:** a physical Linux desktop, a Wayland session, a GPU driver, the zenity/kdialog dialogs, drag and drop, more than one display, and aarch64 beyond CI's build and tests.

## Why Samsung's engine cannot be used on Linux

Conversion is done by Samsung's `ConversionWrapper.dll` (CUDA/ONNX, Windows x64) and the calibrated glasses-free output needs the LeiaSR runtime, eye tracking from the monitor's camera and the lens calibration. None of these exist for Linux. Running them under Wine/Proton is unvalidated and outside this project's scope. Any2Odyssey's Linux edition is therefore the stereo viewer described above, with the capture feature to follow.

## Development machines

The maintainer develops on an aarch64 Linux laptop (cross-checks the Windows code with `cargo check --target x86_64-pc-windows-msvc`) and tests the Windows converter on an x64 Windows PC with a G90XF. Hardware results are recorded in `CHANGELOG.md` and `docs/TESTING.md`.
