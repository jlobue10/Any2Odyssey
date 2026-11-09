# Changelog

Any2Odyssey follows [Semantic Versioning](https://semver.org/). Versions continue from Any2Odd 0.7.3, whose own changelog ships with its source archive on [itch.io](https://zhandragon.itch.io/any2odd).

## 0.8.4 (unreleased)

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
- A damaged `ui-settings.json` (theme, last update-check time) is logged before it is reset; an unreadable `ConversionWrapper.dll` is reported as unreadable rather than missing; a relative `ANY2ODYSSEY_DATA_DIR` is resolved once so the controls and the worker use the same folder; an empty value means unset. (#39)

Core:

- `settings.json` and `ui-settings.json` are flushed to disk before the rename that publishes them, so a power loss right after Apply cannot leave an empty file that the next launch treats as damaged; temporaries left by an interrupted write are swept at launch after an hour. (#39)
- The package version is parsed at compile time, so a pre-release version in `Cargo.toml` fails the build instead of panicking in `--check-updates` and on the About page. The update check requires the published tag to be spelled exactly as the download links are built (`v0.8.4`); `V0.8.4` or `v0.08.4` parsed but every link would have led to a 404. (#39)

Installer:

- Uninstall failures were invisible from Settings > Apps: the console closed with the script, so an instance that was still running, a missing helper or a declined elevation prompt left the app listed with no explanation. The registered uninstall command now keeps the window open with the reason until a key is pressed; a declined prompt gets a plain message. (#38)
- A controls-only record left by a pre-0.8.3 reinstall over a full installation kept the Program Files helper, which blocked `Install-GUI.cmd` while uninstall never ran the helper for that record; the helper now runs whenever it exists. Repairs and upgrades no longer clear values on the registration key that the installer does not own; a failed installation removes its staging folder and the folders it created. (#38)

Documentation and CI:

- Help: three passages still said a shortcut conflict blocks automatic start (changed in 0.8.3), and the input topic promised that checkbox changes save the slider values. (#39, #40)
- The release workflow runs the offline-guide drift check before packaging (0.8.3 said the release workflow ran the full CI checks; the guide check only ran in the Linux CI job) and fails when no package matched. `docs/PORTING.md` no longer calls the installer and its tests untested on Windows; `docs/TESTING.md` lists every installer test; README states which release is current and which was last confirmed on hardware. (#38)

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
