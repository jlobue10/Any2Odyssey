# Changelog

Any2Odyssey follows [Semantic Versioning](https://semver.org/). Versions continue from Any2Odd 0.7.3, whose own changelog ships with its source archive on [itch.io](https://zhandragon.itch.io/any2odd).

## Unreleased

### Changed

- **Docs: the pointer on the Odyssey's own screen.** 0.9.0's notes list the desktop's real pointer looking doubled over the 3D picture as a known issue. It looked right by the end of the same session on the G90XF, after the eye tracking's timing had been corrected; `docs/LENS-SWITCH.md` and `docs/PLATFORMS.md` now say so. Nothing in the program changed.

## 0.9.0 (2026-11-11)

The Linux edition: a stereo viewer that, on an Odyssey 3D, shows images and a live screen glasses-free through the user's own copy of Leia's runtime under Wine. The Windows converter is unchanged from 0.8.7 apart from its version.

### Added

- **Linux: the stereo viewer.** The Linux build opened the Windows controls, which could only report that conversion needs Windows. Its home page is now a stereo viewer: open a side-by-side PNG, JPEG or BMP (Open… through zenity or kdialog, a typed path, a dropped file, or `any2odyssey FILE`), choose pair or flat input, red/cyan anaglyph or side-by-side output, the half-width stretch and the eye swap, see the preview, show the result in an output window (F11 or double-click for full screen, Esc to leave) and save it as PNG.
- **Linux: 2D → 3D for images.** A new input format, 2D image → 3D (AI depth), estimates depth with an open model (Depth Anything V2 Small, Apache-2.0) through the system's ONNX Runtime and renders the left and right views from it (`core::dibr`), with the depth-strength and pop-out controls of the Windows edition. `any2odyssey --install-depth-model` downloads the model (about 95 MB) and checks it against a pinned SHA-256; ONNX Runtime comes from the distribution (`onnxruntime-cpu` or `onnxruntime-cuda`). Without either, the mode says what to install and the rest of the viewer works. For still images here; the live screen and the Odyssey 3D output are entries of their own below.
- **Linux: lens-interleaved output (experimental).** A third output format, Odyssey lens interleave, assigns every red, green and blue subpixel to the left or the right view for a slanted lens sheet (`core::weave`), for a stereo pair or an AI-converted image. Lens pitch, slant and offset are the user's own numbers: a calibration pattern (white for the left eye, black for the right) and keys in the output window (arrows, Page Up/Down, Shift for fine steps, C for the pattern) tune them, and they are kept in `lens.json`. It is a plain two-view interleave written for this project: no eye tracking, nothing of Samsung's weaver, and no reading of Samsung's calibration files. With the lens off it shows fine stripes; the picture and the calibration have not been seen through the lens.
- **Linux: asking for the Odyssey's lens.** The lens is switched by Leia's SR runtime, which alone talks to the monitor's lens board, and its state does not survive a restart from Windows. While lens output is full screen the viewer now asks for the lens and withdraws the request when that ends (`lens_switch`); the line under the lens controls reports off, on, no helper, or the reason for a failure. The asking is done by a separate program, `any2odyssey-lens-helper`, so Any2Odyssey itself contains and links nothing of Leia's; it is looked for in `$ANY2ODYSSEY_LENS_HELPER`, beside the program, in the user's data folder and in the package's library folders. Without a helper nothing changes. See `docs/LENS-SWITCH.md`.
- **Linux: the lens switch through Wine.** `contrib/wine-lens` (MIT) makes the lens helper, for a dual-boot PC (Windows with the Odyssey 3D Hub on a drive Linux can mount): `setup.sh` copies your own installation of Leia's Windows SR runtime (it comes with the Odyssey 3D Hub) from a mounted Windows drive into a Wine prefix and builds the small stand-ins Wine needs before Leia's unmodified service runs there: a toast-notifier class, USB device-tree navigation, device status and the monitor-power notice (`cfgmgr32`), read-only USB descriptors of the lens board (`a2ousb.sys`), the physical-monitor count (`dxva2`), the session state (`wtsapi32`), and the monitors' EDIDs paired by desktop position. Leia's service does the talking to the lens board; nothing of Leia's is distributed, and Any2Odyssey sends the board nothing. A second helper source, `contrib/lens-helper`, is for Leia's commercial srSDK for Linux and is untested.
- **Linux: Leia's eye tracker under Wine .** `contrib/wine-lens/setup.sh` now also copies Leia's eye tracker from your Windows installation and gives it the monitor's webcam: a native bridge (`any2odyssey-camera-bridge`, V4L2, loopback only, protected by a secret file) feeds a Media Foundation camera stand-in (`a2ocam.dll`), since Wine has no Media Foundation video capture. `tools/tracking.sh start | stop | status` runs it beside the lens helper. On a G90XF the tracker followed a viewer and delivered eye positions to a test program through Leia's service. The Odyssey 3D output uses them. The udev rule file now also keeps USB power saving off for the webcam, which stops sending pictures after a suspend until the monitor is power-cycled. See `docs/LENS-SWITCH.md`.
- **Linux: the picture through Leia's own weaver (experimental).** A new output format, Odyssey 3D, hands the left and right views to Leia's weaver instead of interleaving them itself: the panel's factory calibration and the viewer's eye positions, as on Windows. `contrib/wine-lens` gains `any2odyssey-weaver`, which starts Leia's service and eye tracker in the Wine prefix and runs a small weaver host there; the viewer writes its pictures into a file the two share (`crates/app/src/weaver.rs`). The picture opens full screen on the Odyssey by itself, and a status line says whether the tracker sees you. `setup.sh` builds the host when `clang++` is installed. See `docs/LENS-SWITCH.md`.
- **Linux: a live screen in 3D (experimental).** A new input format, Live screen → 3D, shows one of your screens on the Odyssey 3D in depth: the desktop's screen-cast portal provides the screen (PipeWire; the choice is remembered), the depth model runs on its own thread, and every frame is rendered into two views and handed to the weaver (`capture.rs`, `live.rs`, `core::live`). Depth strength and pop-out apply while it runs, with three more controls: sharpness, depth steadiness and detail. On KDE Plasma 6 the screen can be the Odyssey's own: a KWin script loaded by `any2odyssey-weaver` keeps the 3D picture out of the capture, and its window lets clicks through; elsewhere it has to be another screen. Building the Linux edition now needs the PipeWire development files and clang, and the package depends on `libpipewire`.
- **Linux: AI depth runs on the NVIDIA GPU where the distribution's package could not.** Arch's `onnxruntime-cuda` 1.29 ships a CUDA provider that is not linked against cuDNN and fails to load over a missing cuDNN function, so depth silently ran on the processor. Any2Odyssey now opens cuDNN before the runtime (3 ms a depth map instead of 75 on the test machine). On the processor the model is limited to a quarter of the cores (at least two, at most six) instead of all of them.
- **Linux: depth detail.** A new setting for AI depth, for images and the live screen: how large the model sees the picture (Standard, Fine, Finer, Finest; the long side at 518 to 1274 pixels). Larger finds thin things and edges better; on an RTX 5090 a depth map of a 3840 × 2160 screen takes 3, 5, 13 and 26 ms.
- **Linux: Leia's eye tracker pauses under Wine.** It ran on at a full processor core after the last program had stopped asking for eye positions, because Wine does not report a refused UDP datagram the way Windows does. `any2odyssey-udp.so` (preloaded into the prefix) and a few lines in `a2ocam.dll` restore that. On a G90XF it now pauses (about 1 % of a core) and resumes as on Windows; see `docs/LENS-SWITCH.md`.
- **Linux: one instance per user.** A Unix socket in `$XDG_RUNTIME_DIR` is the instance guard; a second launch hands `--show`, `--quit` or its file to the running instance. Global shortcuts, automatic start and the update check are not attempted on Linux.
- **Arch Linux package.** `packaging/arch/PKGBUILD` builds, tests and packages the Linux edition from the checkout (`makepkg -si`), with a desktop entry and icon. CI builds it in an Arch Linux container. From this version a release also carries the Linux edition for x86_64: that package (`any2odyssey-<version>-1-x86_64.pkg.tar.zst`, built in an Arch Linux container) and an archive for other distributions (`Any2Odyssey-<version>-linux-x86_64.tar.gz`, built on Ubuntu 24.04 by `scripts/package-linux.sh`), each with a `.sha256`. The archive has an installer (`install.sh`): it installs the program for the user and, asking before each step, the packages it needs (Arch, Debian/Ubuntu and Fedora families), the udev rules for the Odyssey 3D, the depth model, and the Wine side of the Odyssey 3D from a mounted Windows drive; `--dry-run` and `--uninstall` are there too. The Arch package installs the udev rules itself and offers the further steps as `any2odyssey-setup`. Both include the Wine setup for the Odyssey 3D (`wine-lens`) and `THIRD-PARTY-LICENSES-LINUX.md`, the notices of the crates a Linux build contains beyond the Windows list (`scripts/third-party-linux.py` writes it; CI checks it against the dependencies).

### Hardware verification

Partly done for Linux. Otherwise run only on Arch Linux under WSL 2 (X11, virtual display, Openbox, software OpenGL; AI depth on the CPU). The lens interleave was checked there only as pixels (the full-screen pattern matches the expected subpixel assignment exactly). The lens switch through Wine was run on a G90XF (CachyOS, x86_64, KDE Wayland, NVIDIA, Wine 11.21, SR runtime 1.36.4): from the viewer, full-screen lens output switched the lens on within about half a minute of a cold start and at once afterwards, and leaving full screen switched it off; the interleaved picture looked reasonable after tuning by hand, with display scaling at 125 %. The helper for Leia's Linux runtime has been neither compiled against that SDK nor run, because the runtime could not be obtained. Leia's eye tracker, the weaver helper and the tracker's pause were run on that machine as recorded in `docs/LENS-SWITCH.md`, and AI depth ran on its GPU (RTX 5090, `onnxruntime-cuda` 1.29). On the same machine the Odyssey 3D output was seen from the viewer on the panel: a still image with AI depth (upright, depth the right way round, following the viewer), and the live screen with the Odyssey's own desktop as the screen on KDE Plasma 6.8, including video playing. Known there: the desktop's real pointer is drawn flat over the 3D picture and looked doubled through the lens (it looked right later in the same session; see Unreleased above); some flicker while the head moves remained after the camera's time stamps were corrected (which reduced it); the repeated stop and start and the 2160-line detail were fixed afterwards and checked by measurement only. Also to check: the anaglyph and side-by-side results viewed with glasses or on a side-by-side display. To check on a physical Arch-based desktop: Wayland and X11 sessions, the Open and Save dialogs with zenity and with kdialog, drag and drop, the output window made full screen on a second display, an image opened from the file manager's "Open with".

## 0.8.7 (2026-11-10)

Ninth round: a third full audit by reviewers who had not seen the earlier reports, an inspection of the published 0.8.6 package itself, and a parallel round whose three PRs are folded in (#62 to #64). Built and tested in CI; the hardware checks at the end of this entry are still to be run on the G90XF before this version counts as verified, as are the 0.8.1 to 0.8.6 ones below.

### Fixed

Build and release:

- **The program no longer needs the Visual C++ Redistributable.** The published 0.8.6 executable imports `VCRUNTIME140.dll` when it loads; that DLL is not part of Windows, so on a PC that never had the redistributable installed the program does not start. A full installation usually escapes because Samsung's software brings the runtime along; a controls-only installation on a clean Windows 11 is the likely failure. The C runtime is now linked statically, and the built-executable check fails if the import returns. (#58)
- **The release job packaged an executable other than the one it tested.** Its guide check ran `cargo run -p any2odyssey` after the tests, which relinked the executable, so 0.8.6 shipped a binary the manifest check and both installer test passes had never seen. The check runs the built executable now, and packaging refuses one whose hash changed since it was verified. The release builds with a read-only token and publishes from a separate job that runs no repository code; it verifies the expanded ZIP with the installer's preflight and never replaces the assets of an existing release. (#58, #63, #65)

Worker (Windows):

- **A lost capture device or display is detected.** After a graphics-driver reset or a monitor that re-enumerated, nothing reported the broken pipeline: the last image could stay on screen under status "3D active" and recovery never ran. The worker now asks the Direct3D device for its removed reason once a second and listens for the captured display being closed; both go to recovery. A frame whose size differs from the display's is an error instead of black output. (#59)
- A window close sent to the capture surface (`taskkill` without `/F`) no longer ends the worker as "Renderer stopped cleanly" with a stale status; the surface ignores it, and a message loop that ends without a stop request says so and exits as an error. (#59)
- **Selected-window and active-window modes say when there is nothing to show.** A closed, minimized or uncapturable source left the fill colour on screen with status "3D active" and nothing in the log. The status now reads "No source window to show … waiting for it", stays up when conversion becomes ready meanwhile, and gives way when a source returns; each transition is logged once with its reason. (#59, #64)
- The log line for raising the overlay on a foreground change is summarised once a minute instead of written every time; the watchdog names a stop the worker began itself; a process name loses its `.exe` in any letter case; unused code and features are removed. (#59)

Controls:

- **The status line wraps.** A long message ran on one line and pushed Turn on 3D and the Apply card out of the window, including the damaged-settings message that tells you to click Apply. (#61)
- **Refresh can no longer sit under Apply.** The source-window box grew with the selected title; with a long title a click on Refresh landed on Apply and restarted conversion. Long titles are elided, with the full title on hover. (#61)
- **Capture mode, input format, source window and Keep 3D above apps wait for Apply**, as the sliders already did. They were written straight into the live settings, so any other save kept them and a recovery relaunch or the tray's Restart used them without a word. Unapplied changes appears beside Apply; Turn on 3D, the tray and recovery use the applied settings. (#61)
- **The controls refuse to start a worker of another version** and say which is which; the installer refuses to replace a newer worker that another Windows account installed, unless `-AllowDowngrade`. On a shared PC a second account's installation used to swap the worker under the first account's controls. (#61, #65)
- Shortcut and tray problems are shown in a notice line of their own, which automatic start cannot overwrite; an action's message stays on the status line for a few seconds instead of being replaced within a quarter second; an OS light/dark switch no longer leaves a half-styled page; Show controls keeps a maximized window maximized; starting conversion is refused during a deferred quit. (#61)
- Custom buttons and Help topics announce a name and role to assistive software, and Help topics can be reached and chosen from the keyboard. Non-Unicode arguments and an option-like `--export-guide` path are usage errors; usage errors print to the terminal in every mode; the About page wraps long paths. (#61)

Core:

- **Automatic recovery counts failures in a row.** The count was cleared only by starting conversion yourself, so three recovered hiccups in one session used recovery up for good; a display colour-mode change restarts the renderer through recovery, so toggling HDR four times turned 3D off. The count starts again once a relaunched worker has run normally for a minute. Unticking *Recover* during a restart you asked for no longer cancels conversion because of an earlier, completed recovery. (#60)
- A window fitted into the output is no longer one pixel short for about one source size in fourteen (1680×1050 was 3455 wide instead of 3456). Each `companion.log` line is written in one piece, so lines from the controls and the worker no longer interleave. A conversion log trimmed to its tail no longer yields "ready" from the previous session's marker, and a log that cannot be read is reported. Samsung's key order survives the rewrite of `OverlaySettings.json`. (#60)

Installer:

- **Reinstalling an older release after an uninstall works**, and `Install.cmd -AllowDowngrade` exists: the record kept on uninstall counted as an installed version, and `Install.cmd` forwarded no arguments. (#62, #65)
- **Uninstall completes when Windows reports no Desktop or Start-menu folder**; it used to fail after the worker had already been removed. Shortcuts are written and read through the shell's Unicode interface, so a profile folder outside the ANSI code page gets a correct shortcut. (#62, #65)
- Failures say what failed: a backup that cannot be deleted after a complete installation is not "Installation did not complete", a file in use is retried and not reported as a rollback problem, and the elevated uninstall helper's reason is shown instead of "retry using the same account". The uninstall window opened from Installed apps stays open when it has something to tell you. Running instances are judged per Windows session; the launchers say to extract the ZIP when started from inside it and no longer inherit PowerShell 7's module path. The full converter's need for an administrator account is stated. (#62, #65)
- The installer tests no longer signal the real quit event (running them next to a live Any2Odyssey told it to quit), and the elevated uninstall helper's real code runs in a test for the first time. (#65)

Documentation, notices and CI:

- `THIRD-PARTY-LICENSES.md` carries the license texts and per-crate notices instead of a pointer to crates.io, and lists the six embedded typefaces; `installer/README.md` ships in the ZIP where README links to it. `rust-version` is 1.88, what the locked dependencies need. Every push to `main` keeps its CI run (0.8.6 said so; GitHub still cancelled the queued one). (#63, #65)

### Notes on earlier entries

- 0.8.6, worker: "each of the three ways the message loop can end" missed a fourth, the unrequested window close fixed above.
- 0.8.6, CI: "every push to `main` keeps its artifacts" was not yet true; see above.
- 0.8.6, package: the published executable differs from the one the release job tested, and needs the Visual C++ Redistributable; both are fixed here. Its contents otherwise match the tagged source (verified file by file) and its checksum verifies.

### Hardware verification

Pending. To check on the G90XF with the 0.8.7 package: a normal whole-desktop start and both capture diagnostics (the new frame-size check must not trip) (#59); disable and re-enable the display adapter while converting, and power-cycle the monitor (#59); `taskkill` the worker without `/F` (#59); Selected-window mode with the source closed, minimized and restored, and started with it minimized (#59, #64); toggle HDR more than three times a minute apart (#60); a long window title in the source box, then click Refresh (#61); maximize, hide to the tray, restore (#61); change the capture mode without Apply, hide to the tray and reopen (#61); copy an older worker over the installed one and press Turn on 3D (#61); uninstall 0.8.7, install 0.8.0, then `Install.cmd -AllowDowngrade` paths (#65); decline and accept the elevation prompt on uninstall (#65); start the controls on a Windows PC or VM without the Visual C++ Redistributable (#58). The first tag is also the first run of the two-job release workflow (#65).

## 0.8.6 (2026-11-10)

Eighth round: a second full audit of the code, installer and workflows after the sixth and seventh rounds, plus three Codex follow-ups (#49, #55, #56). Built and tested in CI; the hardware checks at the end of this entry are still to be run on the G90XF before this version counts as verified, as are the 0.8.1 to 0.8.5 ones below.

### Fixed

Worker (Windows):

- **The watchdog now covers the exit itself.** It stood down as soon as the message loop had finished, and the worker then left through the normal process exit, which runs the unload code of Samsung's engine and its CUDA, ONNX and LeiaSR dependencies under the loader lock after their threads were killed. A blocked unload left an invisible process holding the renderer name, so every later start reported another renderer running until Task Manager. After its final log and status lines the worker now terminates itself directly; nothing is lost, since its output is discarded, the status file is complete and the engine is never unloaded. (#50)
- A stop that arrives during the prerequisite checks is honoured before Samsung's `OverlaySettings.json` is rewritten and before the engine is loaded, instead of after both. Each of the three ways the message loop can end is named in `companion.log`; the engine closing itself and the controls process ending exit with the renderer error code and a status-file message instead of "Renderer stopped cleanly" and exit 0. (#50)

Controls:

- **The status line names why the renderer stopped**, from the last state the worker wrote before exiting, instead of only a number; the log carries the same line. Exits that a retry cannot cure (another renderer already running, a prerequisite check failed) turn 3D off with a message naming the cause instead of three pointless 8-second attempts; a clean exit while 3D is still wanted is treated as the engine having closed itself and recovered like any other unexpected stop. (#53)
- Choosing *Always convert desktop (AI)* while a save of `settings.json` fails no longer stops the running conversion and then fails to restart it; like the other mode commands it keeps the worker and reports the failed save. (#49)
- A tray double-click, Ctrl+Alt+F9 or the menus can no longer bring the window back and take the focus during a deferred quit; only `--show` was suppressed before, and a show request queued in the same poll as the Quit is discarded as well. The release-page and download buttons open the browser off the GUI thread, which 0.8.5 claimed and only did for *View logs*. (#53, #56)
- An unreadable Any2Odd `settings.json` is logged and skipped instead of putting Any2Odyssey into recovery mode: only damage to its own file disables automatic start. Because a truncated Any2Odd file may have held the opposite choices, the controls then start with automatic conversion and the daily update check off until the user turns them on; valid current or legacy settings keep their values. Help now says the import happens whenever Any2Odyssey has no settings of its own. Any2Odd's integer capture-mode values (how C# JSON libraries write enums by default) import alongside the names. (#51, #53, #55)
- Minimizing without a native window handle minimizes rather than hiding into a state nothing could restore; each preview run logs to its own temporary file. (#53)

Core:

- Samsung's `OverlaySettings.json` is flushed to disk before the rename that publishes it, like Any2Odyssey's own files since 0.8.4; a power loss right after the rename could leave it truncated, after which every start refused with no recovery path. A mis-spelled release tag (`V0.9.0`, `v0.09.0`) is reported as a spelling mismatch with the expected form instead of "not a version number". Release notes keep line and paragraph separators as line breaks and the narrow no-break space as a space instead of dropping them. (#51)

Installer:

- **A full uninstall leaves a worker and helper whose owner marker names another account**, and says so; 0.8.5's marker protected controls-only records only, so on a shared PC one administrator's uninstall still removed the worker another administrator had since installed, without telling either. The installer says, before the elevation prompt, when it is taking over another account's worker and helper. A controls-only record never runs the helper; the marker-based exception was unreachable through any supported flow and is removed with its test. (#52)
- A declined elevation prompt is recognised by its Windows error code; any other launch failure keeps its own message instead of being reported as "Elevation was declined", including on the already-elevated path where no prompt exists. An unparsable registry `DisplayVersion` no longer blocks installation. README lists `UninstallWorker.owner`, the shared-PC rule and the `-Pause` switch the Installed-apps command carries. (#52)

CI:

- The toolchain action installs the pinned 1.99.1 instead of whatever stable is current (identical today; two downloads per run once 1.100 ships); superseded runs are cancelled only for pull requests, so every push to `main` keeps its artifacts; `docs/TESTING.md` notes that the pinned toolchain needs the Windows target added for the Linux cross-check. (#50, #52)

### Hardware verification

Pending. To check on the G90XF with the 0.8.6 package: a normal stop still ends `companion.log` with "Renderer stopped cleanly" and exit 0 or 4 (#50); kill the controls from Task Manager while converting and confirm the worker is gone and a relaunch starts (#50); toggle 3D off during the prerequisite phase and confirm "the engine was not loaded" with no `OverlaySettings.json` rewrite (#50); close Samsung's engine window during conversion and read the status line and log (#50, #53); start conversion with another renderer running and confirm 3D turns off with the named cause and no retries (#53); quit from the tray while hidden and double-click the tray icon during the stop (#53); decline the elevation prompt during uninstall and read the message, then make `powershell.exe` fail some other way and confirm the message differs (#52); on a PC with two administrator accounts, install from both and uninstall from the first (#52).

## 0.8.5 (2026-11-09)

Seventh audit round: a fresh read of 0.8.4 found two regressions introduced by the sixth round and a gap in its watchdog. Built and tested in CI; the hardware checks at the end of this entry are still to be run on the G90XF before this version counts as verified, as are the 0.8.1 to 0.8.4 ones below.

### Fixed

Controls:

- **Quit from the tray completes again.** The controls hid into the tray with a plain hide, and a hidden window receives no paint message, which is the only thing that runs the application frame. Everything the frame does for a hidden window waited for a paint that never came: the tray tooltip and menu went stale, and the deferred Quit (tray *Quit completely*, `--quit`, the installer's shutdown request before an upgrade) never turned into a window close. 0.8.4 removed the accidental un-hide on Quit that had supplied that one frame, so quitting from the tray left a process only Task Manager could end and an upgrade attempted with the controls in the tray failed with "Quit it from its tray". The controls now minimize without activation as a tool window (no taskbar button, no Alt+Tab entry) instead of hiding, so frames keep flowing; the poller also ends the process itself 3 seconds after a quit request if the window still has not closed. (#45)
- A `--show` that arrives during a deferred quit no longer un-hides the window for a frame; the shortcut-conflict status no longer replaces the settings-recovery message; `--inspect` and `--layout-probe` reject a non-numeric process id instead of probing PID 0 and reporting success; opening a folder or URL runs off the GUI and poller threads; the launch sweep also removes stale `.overlay-backup-*` temporaries; the legacy-import error names Any2Odd's settings file. (#45)

Worker (Windows):

- The 0.8.4 watchdog waited only on the controls process and the stop event, so a stop the worker decided on itself (ready timeout, overlay or capture error, engine closed) called the engine's `Stop` with no backstop; if that call never returned, the worker never exited and recovery never ran until *Turn off* was pressed. The worker now signals the stop event itself before every call into the engine's `Stop`, so the same 7-second grace applies to every stop, including the two paths outside the normal shutdown that the first fix missed. (#43, #48)
- A stop that arrives while prerequisites run, while the capture is being built (up to 10 seconds) or just before `Start` is honoured without starting the model. The watchdog log line names which handle fired and a failed wait is logged; the ready timeout, a failed prerequisite check and a second renderer instance leave a line in `companion.log`. The controls pass their resolved data folder to the worker, which completes the 0.8.4 `ANY2ODYSSEY_DATA_DIR` change. (#43)

Installer:

- **A controls-only uninstall no longer removes another account's full installation.** Since 0.8.4 a controls-only record ran the Program Files helper whenever it existed, so on a shared PC user B's uninstall prompted for elevation and, if approved, deleted user A's worker and helper. A full install now writes an owner marker (the installing account) beside the helper; a controls-only uninstall runs the helper only when the marker names the current account and otherwise says what it left alone. Helpers placed by 0.8.4 or earlier carry no marker, so a controls-only record no longer removes them; `Install-GUI.cmd`'s refusal explains how to clear one (run `Install.cmd`, or remove `C:\Program Files\Any2Odyssey` as an administrator). (#46)
- A truncated `version.json` no longer blocks uninstall or the installer's version lookup; the registered uninstall command names the system `powershell.exe` by full path; the staging folder is cleaned up on every failure path; a declined elevation prompt during install prints a plain message and the "no result" message mentions mapped drives; the `.cmd` launchers use `Sysnative` from a 32-bit host; the `.sha256` file ends with LF so `sha256sum -c` works on Linux and macOS (the 0.8.4 file ends with CRLF). (#46)

Documentation, notices and CI:

- A folder named `--list` with three duplicate images was tracked by the 0.8.4 release commit and is removed; the icon maker refuses option-shaped folder names. README said 0.8.3 was current. NOTICE pointed at a sponsors page that Help and README deliberately do not; the About credits name both embedded typefaces; the tray Help topic no longer says explicit Minimize to tray is always available; `docs/PORTING.md` no longer claims an automated layout test. (#44)
- The toolchain is pinned to 1.99.1 (clippy runs with warnings denied, so a new stable could turn an unrelated PR or a tag push red); `.html`, `.txt` and `.manifest` files check out with LF so a Windows checkout matches the Linux job's byte comparison; CI cancels superseded runs and keeps artifacts 14 days; the release workflow refuses a version without a changelog entry; every crate is marked `publish = false`. (#44)

### Notes on earlier entries

- 0.8.4 (#40), controls: the "no flash on quit" change is what exposed the hidden-window frame problem above; with 0.8.5 both hold (no flash, and Quit completes).
- 0.8.4 (#38), installer: "the helper now runs whenever it exists" is narrowed to helpers this account installed; see the installer entry above.
- 0.8.4, hardware list: "confirm the status line reports the missing notification icon" holds only until the automatic start reports its own status; the log line is the reliable trace.

### Hardware verification

Pending. To check on the G90XF with the 0.8.5 package: hide to the tray and confirm no taskbar button and no Alt+Tab entry; restore via tray double-click, Ctrl+Alt+F9 and `--show`; quit from the tray while hidden with a game in the foreground (no flash, process gone within a second) (#45); run `Install.cmd` over 0.8.5 with the controls in the tray (the installer's quit request must succeed) (#45); force a ready timeout (start with Samsung's player holding the engine, or disconnect the camera) and confirm the worker exits and recovery runs (#43); toggle 3D on and off within a second during model load (no 7-second kill in the log) (#43); after a full install, confirm `C:\Program Files\Any2Odyssey\UninstallWorker.owner` holds your SID and that uninstall removes it (#46); verify the published `.sha256` with `sha256sum -c` on a Linux machine (#46).

## 0.8.4 (2026-11-09)

Sixth audit round: a fresh read of the code, installer, workflows and documentation after 0.8.3, with one Codex follow-up (#37). Built and tested in CI; the hardware checks at the end of this entry are still to be run on the G90XF before this version counts as verified, as are the 0.8.1 to 0.8.3 ones below.

### Fixed

Worker (Windows):

- An in-process watchdog ends the worker when its message loop cannot: every stop and liveness check ran on that one thread, so an engine `Stop` that never returned or a window call blocked inside the engine would have left the click-through overlay on screen once the controls were gone (the controls themselves kill the worker after 7 seconds while they are alive). The watchdog waits on the controls process and the stop event, allows the same 7 seconds, then terminates the process. A poll or overlay timer that cannot be created now fails the start instead of leaving the loop idle. (#41)
- Overlay and capture errors exit with the renderer error code and no longer log "Renderer stopped cleanly" while `worker-status.json` holds the error; the overlay issues the frame-change notification Win32 requires after its style change whether or not Keep 3D above apps is on; the overlay is not re-shown between the engine's own close and the next poll; a DLL-directory registration error is read before its buffer is freed; the per-worker event names mix the process id as their comment claimed; the update check asks WinHTTP for certificate revocation checking. (#41)
- A failed display-colour query at worker start is retried briefly instead of seeding the FP16 fallback at once, and when the first real answer does replace that fallback the message says so ("became available after startup") instead of claiming the display changed. The restart through recovery still costs one attempt. (#39)

Controls:

- Minimize and Close hide into the tray only when the notification icon was actually created, not merely on Windows; if its creation fails, Minimize stays on the taskbar, Close quits, the tray preference is disabled, the title-bar button and menu read "Minimize", and the status line says why. (#37, #40)
- Quit from the tray or `--quit` while the controls were hidden un-hid and activated the window for one frame, stealing the focus from the foreground game. The controls now stay hidden while the deferred quit runs. (#40)
- `--toggle` and `--show` could be lost when they raced a starting instance that had not yet created its control events; the request is retried and the command exits 1 when it was never accepted. (#40)
- "Automatic recovery cancelled" survives the relaunch that was already validating when recovery was unchecked; it was replaced a moment later by "3D is off". (#40)
- Changing the automatic-start, recovery or tray checkbox no longer saves a depth or pop-out that was dragged but not applied; Apply is the only path that commits the sliders, as the tooltips say. (#40)
- A damaged `ui-settings.json` (theme, last update-check time) is logged before it is reset; an unreadable `ConversionWrapper.dll` is reported as unreadable rather than missing; a relative `ANY2ODYSSEY_DATA_DIR` is resolved against the controls' working directory and an empty value means unset. (#39)

Core:

- `settings.json` and `ui-settings.json` are flushed to disk before the rename that publishes them, so a power loss right after Apply cannot leave an empty file that the next launch treats as damaged; temporaries left by an interrupted write are swept from the data folder at launch after an hour. (#39)
- The package version is parsed at compile time, so a pre-release version in `Cargo.toml` fails the build instead of panicking in `--check-updates` and on the About page. The update check requires the published tag to be spelled exactly as the download links are built (`v0.8.4`); `V0.8.4` or `v0.08.4` parsed but every link would have led to a 404. (#39)

Installer:

- Uninstall failures were invisible from Settings > Apps: the console closed with the script, so an instance that was still running, a missing helper or a declined elevation prompt left the app listed with no explanation. The registered uninstall command now keeps the window open with the reason until a key is pressed; a declined prompt gets a plain message. (#38)
- A controls-only record left by a pre-0.8.3 reinstall over a full installation kept the Program Files helper, which blocked `Install-GUI.cmd` while uninstall never ran the helper for that record; the helper now runs whenever it exists. Repairs and upgrades no longer clear values on the registration key that the installer does not own; a failed installation removes its staging folder and the folders it created. (#38)

Documentation and CI:

- Help: three passages still said a shortcut conflict blocks automatic start (changed in 0.8.3), and the input topic promised that checkbox changes save the slider values. (#39, #40)
- The release workflow runs the offline-guide drift check before packaging and fails when no package matched. `docs/PORTING.md` no longer calls the installer and its tests untested on Windows; `docs/TESTING.md` lists every installer test; README states which release is current and which was last confirmed on hardware. (#38)

### Notes on earlier entries

- 0.8.3, CI: the release workflow ran the Windows CI checks before packaging, not "the full CI checks"; the offline-guide drift check only ran in the Linux CI job until 0.8.4.
- 0.8.4 (#39), `ANY2ODYSSEY_DATA_DIR`: the controls resolved a relative value against their own working directory, but the worker, which runs from the engine folder, still resolved it separately; the controls pass the resolved folder to the worker since 0.8.5.

### Hardware verification

Pending. To check on the G90XF with the 0.8.4 package: kill the controls from Task Manager while converting and again while the model is loading (the overlay must be gone within about 8 seconds even if the engine's Stop hangs) (#41); quit from the tray while the controls are hidden with a game in the foreground (no flash, no focus loss) (#40); `Any2Odyssey --toggle` twice in quick succession from a cold start (#40); uncheck recovery during a recovery countdown and confirm the cancel message stays (#40); drag Depth, toggle a checkbox, restart the app and confirm the old depth is still saved (#40); uninstall from Settings > Apps with the controls still running and read the message (#38); stop Explorer, start conversion, and confirm the status line reports the missing notification icon and Minimize stays on the taskbar (#37, #40).

## 0.8.3 (2026-11-09)

Fifth audit round: the five follow-up fixes to 0.8.2 (#25–#29) and a full-repository audit of the code, installer, workflows and notices (#30–#35). Built and tested in CI; the hardware checks at the end of this entry are still to be run on the G90XF before this version counts as verified, as are the 0.8.1 and 0.8.2 ones below.

### Fixed

Worker (Windows):

- The worker holds a handle to the controls process instead of polling its PID. If the controls died and Windows reused the PID within half a second, the worker believed its parent was alive and the click-through overlay stayed up with no way to stop it short of Task Manager, while a relaunched controls window's new worker exited as already running. (#32)
- Conversion no longer aborts when the renderer's taskbar entry cannot be removed (Explorer stopped or restarting); the cleanup is logged and retried, and only the input-safety checks remain fatal. The diagnostic modes write their report instead of panicking when no frame ever arrived; after a shutdown the message loop stops overlay and capture work so a late error cannot overwrite the recorded stop reason; the update fetch has a 30-second overall deadline and refuses redirects on the request handle as well as the session. (#32)

Controls:

- Minimize and Close hide into the tray only on builds that have one. Without a notification icon (the Linux build) a hidden window could be recovered only by a global shortcut, which not every desktop offers; the checkbox is disabled there with an explanation. (#31)
- `--screenshot` exits 1 when the image is not delivered or cannot be saved, and rejects an output path that looks like an option; the preview no longer imports or saves preferences, creates the tray icon or touches the data directory. (#29, #31)
- `--check-updates`, `--self-test`, `--export-guide` and the diagnostics attach to the parent console on Windows, where the release build is a GUI-subsystem process whose printed line was otherwise lost (it may appear after the prompt returns). `--quit` exits 1 when no instance received the request; a `--toggle` that loses the single-instance race toggles instead of showing. (#31)
- Turning 3D off while a start was still validating no longer leaves "Stopping 3D…" on screen when that start then fails. A shortcut conflict is reported but no longer blocks the automatic start; the button, tray and `--toggle` never required the shortcuts. (#31)
- An unreadable `settings.json` also disables the daily update check, and the recovery message says so and where to turn it back on; the saved last-check time can no longer overflow the due-date arithmetic. The About page's commit identification refreshes after a commit on the current branch. (#25, #26, #27, #31)

Core:

- The *Download package* and checksum links are built from the release version and asset name, like the release page link already was; GitHub's document only confirms the asset exists. Release-note sanitisation removes every bidirectional-control character, the tag characters used to hide text, soft hyphens and line separators, and keeps the zero-width joiners that emoji and cursive scripts need. One malformed asset entry no longer fails the check. (#30)
- A UTF-8 byte-order mark in `settings.json`, `ui-settings.json` or Samsung's `OverlaySettings.json` (Notepad's "UTF-8 with BOM", PowerShell 5.1's `-Encoding UTF8`) is no longer treated as damage. (#30)
- The one-time `Samsung-OverlaySettings-before.json` backup and the `settings-recovery-*.json` backup are written through a temporary file, synced to disk and published without replacing an existing file, so neither a failed write nor a power loss can claim the one-time name with a truncated copy. (#28, #30)

Installer:

- Uninstall works when every process already holds an administrator token (UAC disabled, built-in Administrator); it ran the refusal meant for the normal-user path and nothing could be removed. An instance started elevated no longer crashes the uninstaller with an access-denied exception. The hidden elevated installer runs non-interactively. A controls-only install can no longer orphan the Program Files uninstall helper of a former full install, and the emptied program folder is removed on uninstall. (#34)
- Tests: the full-installation uninstall flow is exercised end to end, the "never launches the per-user executable" check inspects the parsed script rather than two literal strings, fixtures and the shared test registry key are cleaned up, and the Windows 11 24H2 gate can be skipped by the tests. `installer/README.md` states the installer's trust model. (#34)

Packaging, notices and CI:

- `THIRD-PARTY-LICENSES.md` lists the crates compiled into the Windows binary and every embedded font. It previously said Libre Baskerville was not shipped (it is the display face) and did not attribute egui's bundled Ubuntu Light, Hack, Noto Emoji and emoji-icon-font. The ZIP ships the README screenshots, and packaging refuses an archive with backslash entry names. (#33)
- Workflows pin every action to a commit, run with `--locked`, grant the CI token read access only, pass the tag through the environment, run the full CI checks before a release is packaged, run the installer tests under Windows PowerShell 5.1 as well as PowerShell 7, and fail when `USER-GUIDE.html` drifts from the embedded Help. (#35)

### Notes on earlier entries

- 0.8.1, display colour: a running capture tolerates four consecutive failed queries; the fifth stops it ("up to five" above overstated it by one).
- 0.8.2, update check: a draft or pre-release at the `latest` endpoint is rejected and reported as a failed check rather than silently ignored (GitHub never returns one there), and the request carries the standard `Accept` and `X-GitHub-Api-Version` headers besides the client name.
- 0.8.2, `--check-updates`: on the published 0.8.2 build the printed line is visible only when redirected to a file; the exit code and the log line were always reliable.

### Hardware verification

Pending. To check on the G90XF with the 0.8.3 package: kill the controls from Task Manager while converting (the worker and overlay should stop within a second) and relaunch (#32); start conversion with Explorer stopped (#32); `--check-updates` from a terminal shows its line (#31); `--screenshot` to a read-only folder exits 1 (#31); turn 3D on and immediately off while the model is loading and confirm the status settles (#31); fix a damaged `settings.json` in Notepad with "UTF-8 with BOM" and confirm it loads (#30); uninstall on a machine with UAC disabled (#34); the CI 5.1 installer-test run and the packaged ZIP's `docs/assets` (#33, #35).

## 0.8.2 (2026-11-08)

Fourth audit round plus an About page and an update check. Built and tested in CI; the hardware checks at the end of this entry are still to be run on the G90XF before this version counts as verified, as are the 0.8.1 ones below.

### Added

- **About page**, from the About button in the title bar, the right-click menu and the tray menu: running version, the commit the build came from, platform, executable folder, the installer's record (`version.json`: installation time, install/upgrade/repair, full converter or controls only, worker location) or a note that the copy is not installed, credits, license and links to the project page, releases, changelog and issue tracker. The controls footer shows the version under the shortcuts line. (#22)
- **Check for updates.** *Check now* on the About page, and *Check for updates* in the right-click and tray menus, ask GitHub for the newest published release and compare it with the running version; the result, publication date and release notes (plain text) appear on the page with *Open release page* and *Download package* buttons that only ever open GitHub's own hosts. With the new preference *Check for updates automatically once a day* (default on) the controls run one check when they open, at most once per 24 hours; the time of the last successful check is kept in `ui-settings.json`. The tray tooltip names an available version. Nothing is downloaded or installed automatically. The check runs over WinHTTP with the system proxy settings and certificate store, refuses redirects, caps the response at 1 MiB and sends a plain `Any2Odyssey/<version>` client name and nothing else; drafts and pre-releases are ignored, and release notes are stripped of control and bidirectional-override characters before display. The worker process never performs network I/O. (#17, #21, #23)
- `Any2Odyssey --check-updates` runs the same check from a terminal and exits 0 (current or ahead of the latest release), 10 (update available) or 1 (error). (#21)
- Help topic *About & update check*; the *Install, update & uninstall* and *Window, tray & shortcuts* topics mention the new entries. The offline `USER-GUIDE.html` is regenerated; the copy shipped with 0.8.1 still carried the 0.8.0 header.

### Fixed (audit round four, PRs #18–#20)

- An unreadable or malformed `settings.json` is no longer treated as a first run. It disables automatic conversion, blocks Start and the automatic saves at hide and quit, and shows how to recover; *Apply* becomes the explicit way to replace the file and first preserves the original as `settings-recovery-*.json`. (#18)
- Selected-window mode requires an explicit choice each time the controls open: startup and *Refresh* no longer pick the first enumerated window, and a vanished choice is cleared instead of replaced. The source box shows *Choose a window*. (#19)
- The worker stops with an error instead of starting when Samsung's `OverlaySettings.json` cannot be updated, so stale depth, pop-out or side-by-side values are never applied silently; the update now happens before the engine is loaded. (#20)

### Hardware verification

Pending. To check on the G90XF with the 0.8.2 package: the About page from the title bar, the right-click menu and the tray menu on an installed copy, with the installation rows matching `install-result.json`, and the *not installed* line from a portable copy; *Check now* reporting the newest release on the published build and an available update on a build with an older `VERSION`, with *Open release page* landing on GitHub; the startup check running once and then not again the same day (`LastUpdateCheck` in `ui-settings.json`), and not at all with the switch off; a check while offline giving the failed line and nothing else; the tray tooltip naming an available version; `--check-updates` exit codes; recovery of a damaged `settings.json` through *Apply* with the backup present (#18); Selected window asking for a choice on each launch (#19); and a start with Odyssey 3D Hub open and holding `OverlaySettings.json` (#20).

## 0.8.1 (2026-11-08)

Third audit round (PRs #12–#16), merged the same day 0.8.0 was confirmed on hardware. Everything below was built and tested in CI; the hardware checks listed at the end of this entry still have to be run on the G90XF before this version counts as verified.

### Fixed

- The worker receives an immutable JSON snapshot of the validated preferences on its command line instead of re-reading `settings.json`, and Start, Apply and Restart stop with a visible error when the preferences cannot be saved. Previously a failed save let the worker launch from the old file, so Selected window could start Whole desktop capture. The internal `--worker` command gains a required preferences argument; GUI and worker must be installed together, as the full installer does. (#12)
- The executable carries a PerMonitorV2 DPI-awareness manifest (`asInvoker`), so the worker and the diagnostic modes, which bypass the GUI event loop, no longer see virtualized coordinates on scaled or mixed-DPI monitors. The Windows build fails if the resources cannot be embedded, and CI and the release workflow read the manifest back from the built executable without running it. (#13)
- A failed display-colour query is no longer reported as an HDR/default-white-level result. A running capture tolerates up to five consecutive one-second query failures and retries the `QueryDisplayConfig` buffer-sizing race, while a confirmed colour-mode or SDR-white-level change still recreates the pipeline. (#14)
- The per-frame feedback guard checks monitor overlap through the `EnumDisplayMonitors` rectangle callback with no allocations or name lookups, stops at the first overlap, and treats an empty, invalid or failed enumeration as unsafe. (#15)
- The controls window repaints only when the poller reports changed visible state or pending commands, tray label and tooltip are rewritten only when they change, and a hidden or minimized window skips content rendering after processing show/quit requests. Screenshot previews keep their timed frames. (#16)

### Hardware verification

Pending. To check on the G90XF with the 0.8.1 package: Start, Apply and rapid Restart in each capture mode (#12); 125–200 % display scaling and a mixed-DPI arrangement (#13); toggling HDR and changing the SDR white level while converting (#14); rearranging monitors while converting (#15); tray restore, the Ctrl+Alt hotkeys, quit while hidden, and idle CPU/GPU use of the controls window (#16).

## 0.8.0 (2026-11-08)

First Any2Odyssey release. Everything below was built and tested in CI, reviewed, and then confirmed working on the target hardware the same day (see [Hardware verification](#hardware-verification) at the end of this entry).

### Audit fixes before release, second round (PRs #7–#11)

- Atomic file writes use an exclusively created temporary sibling instead of a fixed `.tmp` name, so concurrent writers cannot clobber each other and a planted symlink cannot redirect the write. (#7)
- A malformed or unreadable Samsung `OverlaySettings.json` is left untouched and reported instead of being replaced by an empty document; only a missing file is initialized, and the one-time backup stores exactly the parsed original. (#8)
- Capture-pool failures (a closed or failed `TryGetNextFrame`) now stop the worker and enter recovery instead of leaving a frozen image reported as active; checked-out frames are returned on every error path. (#9)
- Selected-window and follow-active capture check only the candidate handles instead of enumerating every top-level window every 250 ms, keep the DWM thumbnail across moves and update it on resize. (#10)
- The worker restricts DLL resolution to System32 and the fixed Samsung/CUDA/ONNX/LeiaSR folders (`SetDefaultDllDirectories` plus `AddDllDirectory`), and loads the engine with explicit search flags; PATH and the current directory no longer supply code. Missing engine dependencies fail instead of falling back. (#11)

### Audit fixes before release, first round (PRs #1–#6)

- Uninstall no longer elevates a script from user-writable `%LOCALAPPDATA%`. The only elevated step is `UninstallWorker.ps1`, installed under `%ProgramFiles%\Any2Odyssey`, which checks the approving account and its own path and removes only the fixed worker path and itself. The per-user uninstaller signals the quit event directly instead of launching the installed executable. (#6)
- Worker start and the seven-second shutdown wait run off the GUI's shared-state lock, so Stop, Apply and Restart no longer freeze rendering; stale start results are discarded by generation, and Quit waits for cleanup while the window stays responsive. (#5)
- Engine readiness is detected only from `Conversion.log` data written after `Start`, so a marker left by an earlier Samsung session cannot mark a failed new start as ready. (#2)
- Capture frame notifications are coalesced and control checks (stop, parent liveness, readiness, overlay) run between dispatched messages, so sustained capture cannot starve them. (#3)
- `ID3D11Multithread` protection is enabled on the immediate context, where it belongs, and verified. (#1)
- SDR displays are captured as BGRA8 directly; the FP16 path with the SDR-white-level shader is kept for HDR or unknown colour state, a colour-mode change restarts conversion, and the CPU readback texture is allocated only by diagnostics. (#4)

### Added

- `--screenshot FILE [--dark] [--help-page]` renders the controls in an isolated preview for documentation and UI checks.

First Any2Odyssey release: a complete rewrite of Any2Odd 0.7.3 (Zhandragon, GPL-3.0-only) in Rust, under a new name and identity.

### Changed

- Rewritten in Rust as a single native executable. The .NET Framework 4.8 runtime, WinForms, WPF and the four bundled SharpDX DLLs are gone; Win32, DWM, Windows Graphics Capture and Direct3D 11 are used through the `windows` crate.
- New identity, a clean break from Any2Odd: application folder `%LOCALAPPDATA%\Programs\Any2Odyssey`, data folder `%LOCALAPPDATA%\Any2Odyssey`, worker `Any2OdysseyWorker.exe`, Installed-apps entry `Any2Odyssey`, new named events and mutexes. An installed Any2Odd is never touched; its conversion preferences are imported once on first launch if present.
- Installer simplified: no dependency manifest, no shared-folder ownership tracking, no NuGet restore. Version detection reads the executable's Windows version resource.
- Icon and wordmark regenerated by a Rust tool (`tools/iconmaker`) with the bundled Libre Baskerville font (SIL OFL 1.1).

### Removed

- All donation and sponsorship material (Donate page, QR codes, wallet addresses, PayPal link, GitHub Sponsors button, FUNDING.yml). Upstream credit stays in the README, NOTICE and the Origins & credits Help topic.
- The Python/PySide6 "portable" launch bundles for macOS/Linux. Their feature set is the target of the planned native Linux build.

### Inherited from Any2Odd 0.7.3 (behaviour preserved)

- Whole-desktop, selected-window and follow-active-window capture; AI and manual side-by-side input; depth strength and pop-out; Apply restarts active conversion.
- Click-through, no-activation 3D output; Keep 3D above apps; Ctrl+Alt+F8/F9/F10 global hotkeys; F1 Help; tray and jump-list commands; minimize/close to tray.
- Automatic start on launch; automatic recovery (8 s, up to three attempts) with the 0.7.3 fix that disabling recovery cancels an already queued retry.
- Prerequisite checks: ConversionWrapper.dll SHA-256 pin, worker present, 3840 × 2160 primary display, working 3D Stereo WebCam, Samsung player not running.
- Capture output excluded from capture, hardware cursor excluded, offscreen capture surface placed outside all monitors, HDR to SDR normalization using the display's SDR white level, 16 ms minimum capture interval, borderless capture when Windows grants permission.
- Themed light/dark controls, responsive layout (980 × 780 default), written Help topics.

### Hardware verification

Confirmed working on 2026-11-08 with the released `Any2Odyssey-0.8.0-windows-x64.zip`.

| Component | Tested |
| --- | --- |
| Display | Samsung Odyssey 3D G90XF, primary, 3840 × 2160, DisplayPort + USB upstream, in a four-monitor arrangement |
| GPU | NVIDIA GeForce RTX 5090, driver 621.14 |
| OS | Windows 11 Pro x64, build 26300 |
| Samsung software | Odyssey 3D Hub 1.5.1 with the pinned `ConversionWrapper.dll` |

Results:

- `Install.cmd` performed a clean full installation (worker placed beside Samsung's engine, engine hash check passed, Installed-apps entry registered); the installer's `install-result.json` and the app's `version.json` were written as designed.
- First launch imported the existing Any2Odd preferences once, as documented.
- Automatic start on launch worked: whole-desktop WGC GPU capture started (cursor excluded, 16 ms minimum interval), the renderer came up as a click-through, no-activation overlay hidden from Alt+Tab, the output was excluded from capture (no feedback trails) and the status read `3D active — click-through overlay; stays above apps`.
- Glasses-free 3D output on the G90XF was judged good at the monitor with depth 0.9 and pop-out 0.
- Keep 3D above apps restored the overlay without stealing focus across dozens of foreground-window changes; no worker failure, recovery attempt or capture error was logged.
- Capture statistics over a nine-minute session: 31,400 frames, about 57 fps, 0.38 ms average capture time.

Not covered by this pass and still open for a later entry: selected-window and follow-active-window capture, manual side-by-side input, a borderless game behind the overlay, killing the worker to exercise recovery, and uninstall.
