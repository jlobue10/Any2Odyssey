# Changelog

Any2Odyssey follows [Semantic Versioning](https://semver.org/). Versions continue from Any2Odd 0.7.3, whose own changelog ships with its source archive on [itch.io](https://zhandragon.itch.io/any2odd).

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
