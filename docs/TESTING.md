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

On Linux the Windows crate is only compile-checked: `cargo check --workspace --target x86_64-pc-windows-msvc`. The pinned toolchain in `rust-toolchain.toml` does not ship that target; add it once with `rustup target add x86_64-pc-windows-msvc --toolchain 1.99.1` before the cross-check or clippy run.

## Linux

`python3 contrib/wine-lens/tests/test-camera-bridge.py` builds the native camera bridge with the system C compiler and tests its loopback authentication: the exchange of random numbers and proofs in both directions, a wrong answer, the bridge's own proof offered as the client's, an answer made for another connection's number, silent and trickling clients, and a client that stops reading. It uses a temporary test secret and `/dev/null` instead of a camera; no Wine, hardware or root access is needed. Both Linux CI architectures run it.

`python3 contrib/wine-lens/tests/test-noxon.py` builds the serial port's keeper (`src/noxon.c`) and runs it against a pseudo-terminal, with a process of a made-up name standing in for Leia's service: flow control is switched off again each time it is switched on, no other line setting is touched, the keeper leaves when that process has gone and waits for a port that is not there. No lens board, Wine or root access is needed. `NOXON_TEST_BINARY` names an already built keeper to test instead.

`python3 contrib/wine-lens/tests/test-mkedid.py` loads `tools/mkedid.py` and checks the monitor name it writes into the registry file for an EDID: a real maker keeps its three letters, and no maker code, including the malformed ones that used to come out as `[`, `\` and `]`, can break the file's syntax. No monitor, Wine or `xrandr` is needed. (Python leaves `tools/__pycache__` behind in the checkout; the packaging scripts leave it out and CI refuses a package that has it.)

`python3 contrib/wine-lens/tests/test-weaver-watcher.py` runs `any2odyssey-weaver` against stand-ins for the scripts it starts: when the helper is killed outright, the process it left behind still stops eye tracking, and a `flock` without a timed wait (BusyBox) does not turn that process's wait into a busy loop. No Wine, monitor, desktop or root access is needed.

`python3 contrib/wine-lens/tests/test-tracking-stop.py` runs `tools/tracking.sh` against stand-ins for Wine, Leia's service and its eye tracker with chosen delays: a stop waits for a tracker that is slow to leave (so a start right after it really starts one), a stop during a start that is still under way leaves no tracker behind, a tracker that never leaves holds a stop for about 10 s and no longer, and a normal stop is not slowed. `python3 contrib/wine-lens/tests/test-setup-pgrep.py` checks that `setup.sh` stops before creating anything when the only `pgrep` is one without `-u` (BusyBox's). Neither needs Wine, the monitor or root access.

The release workflow runs every test of this section and of the ones below with the same commands as CI, before anything is signed or published.

`python3 packaging/linux/tests/test-install.py` runs the release archive's `install.sh` in temporary folders, with a stand-in for `/` (`A2O_INSTALL_TEST_ROOT`: a mount table, `/sys` block devices and mounted "Windows drives" made of empty files), a fixture program in place of the executable and of `setup.sh`, and no root access (while that variable is set the script uses sudo for nothing). It covers the desktop launcher for awkward prefixes (it needs `gio` and `desktop-file-validate`), `--help`, `--dry-run`, the prefix without GNU `readlink`, and the choice of a Windows drive: the one internal drive used with `--yes`; USB drives, memory cards, flagged-removable disks and mapped volumes over them not used; several candidates listed and none chosen; a network share or a user's FUSE mount refused with `--yes`; a folder without a Windows system folder or with another user's files (that case needs root) passed over; `--windows-root` taken as given. It also covers what `--uninstall` stops and in which Wine prefix. `A2O_INSTALL_TEST_SHELL` runs the same tests under another shell (`dash`, `"busybox sh"`); CI uses the system's `sh`.

`cargo deny --locked check advisories bans sources` (cargo-deny 0.20, policy in `deny.toml`) checks `Cargo.lock` against the RustSec advisory database (vulnerabilities, unsound code and yanked versions anywhere in the tree; unmaintained crates only among direct dependencies), refuses wildcard version requirements and any source but crates.io. CI runs it as its own job, and so does the release workflow, where the signing and publishing jobs wait for it. It fetches the advisory database, so a new advisory fails it without any change here (on a tag too, before anything is published): update the crate, or list the advisory under `ignore` in `deny.toml` with the reason.

The UDP shim's wrappers can also be checked without Wine or hardware:

```sh
cc -std=c11 -O1 -g -Wall -Wextra -Werror -fsanitize=address,undefined -pthread \
  contrib/wine-lens/tests/test-connreset.c -o /tmp/test-connreset
ASAN_OPTIONS=detect_leaks=0 /tmp/test-connreset
```

This runs the production send/receive/close wrappers against a simulated Winsock provider, including completion before the receive call returns, recycled receive storage, and socket handle reuse. It does not exercise import routing or Leia's tracker. After changing these wrappers, verify pause/resume and camera release on the target Wine installation, including a receive submitted before the first destination is known.

Building needs the PipeWire development files and clang beside the desktop libraries (Ubuntu: `libpipewire-0.3-dev libclang-dev clang`; Arch: `libpipewire clang`). `cargo test -p any2odyssey-core -p any2odyssey` runs everywhere and starts no helper, capture or model. Three measurements are kept as ignored tests for a machine with the hardware: `live::bench` (stage timings, needs the depth model), `capture::probe` (what the screen cast of the remembered screen contains) and `live::probe` (the whole live path against the Odyssey; `PROBE_ROUNDS`, `PROBE_LINES`, `PROBE_DETAIL`, `PROBE_SECONDS`). Run one with `cargo test --release -p any2odyssey <name> -- --ignored --nocapture`. `scripts/package-linux.sh` stages the Linux release archive from a release build and `scripts/third-party-linux.py` writes `THIRD-PARTY-LICENSES-LINUX.md`; CI runs both and refuses a list that no longer matches the dependencies. What has been checked on a G90XF is recorded in `docs/LENS-SWITCH.md` ("What has been checked") and `docs/PLATFORMS.md`.

### Camera reader startup lifetime

```sh
cc -std=c11 -O1 -g -Wall -Wextra -Werror -fsanitize=address,undefined \
  contrib/wine-lens/tests/test-camera-stream.c -o /tmp/test-camera-stream
ASAN_OPTIONS=detect_leaks=0 /tmp/test-camera-stream
```

This compiles the production camera reader start/stop paths with a simulated bridge. It checks a late frame after a failed start and reader release, failed-start retry, repeated start/stop, and restart after stopping. AddressSanitizer detects a retained callback into the freed reader. It does not exercise Media Foundation, bridge threading, or the physical camera; startup, failure recovery, and camera release on the target Wine installation remain hardware checks. Both Linux CI architectures run the native test.

### Camera reader threaded shutdown

```sh
cc -std=c11 -O1 -g -Wall -Wextra -Werror -fsanitize=address,undefined -pthread \
  contrib/wine-lens/tests/test-camera-reader.c -o /tmp/test-camera-reader
ASAN_OPTIONS=detect_leaks=0 timeout 30 /tmp/test-camera-reader
```

This runs the production reader thread, final release and cleanup paths with pthread-backed handle mocks. A barrier keeps a callback active while the simulated five-second shutdown wait times out or fails, then lets it return; the reader must remain alive until then and be freed exactly once. Other cases cover duplicate-handle failure, release from read/flush callbacks, an idle reader, nonfinal references, and event/thread creation failure. The timeout is simulated so the test does not sleep five seconds. Media Foundation, Winsock and physical Wine/camera behavior still need target-system validation.

### Camera bridge client

```sh
cc -std=c11 -O1 -g -Wall -Wextra -Werror -fsanitize=address,undefined -pthread \
  contrib/wine-lens/tests/test-camera-client.c -o /tmp/test-camera-client
ASAN_OPTIONS=detect_leaks=0 timeout 60 /tmp/test-camera-client
```

This runs the production bridge client of `a2ocam.c` (the handshake, commands and replies, the reading thread and its shutdown) against a simulated bridge on the other end of a socket pair, with pthread-backed Windows handles. It checks commands and an orderly close, a wrong secret, impostors on the port (no proof, a wrong one, the client's own answer handed back, a proof made without its label, a picture sent before the proof), fresh random numbers for each connection, a reply that arrives after its command timed out (the connection is closed instead of the reply answering the next command), a bridge that goes away, a close while the reading thread is busy, that the end of the connection and a timeout are reported, and that nothing is left behind when the thread or its event cannot be made. Timeouts are shortened and a stuck thread is simulated. It needs no Wine, camera or Media Foundation; the same behaviour under Wine with the real tracker is a hardware check. Both Linux CI architectures run it.

### Camera bridge buffers

```sh
cc -std=c11 -O1 -g -Wall -Wextra -Werror -fsanitize=address,undefined \
  contrib/wine-lens/tests/test-camera-buffers.c -o /tmp/test-camera-buffers
ASAN_OPTIONS=detect_leaks=0 /tmp/test-camera-buffers
```

This compiles the production camera bridge (`src/camera-bridge.c`) with a stand-in for the V4L2 calls and makes its capture start fail at each step (too few buffers, the allocation, and for every buffer the query, the mapping and the queueing, then the stream start). After each failure nothing may stay allocated or mapped, stopping again must be harmless, and a second start on the same open camera must work. It needs no camera; that the real driver accepts a second start after a failed one is a hardware check.

## Windows diagnostics (provided by the Windows crate)

Run these with conversion stopped. They are GUI executables; use `Start-Process -Wait -PassThru` to read the exit code.

- `Any2Odyssey.exe --self-test`: geometry and multi-monitor source-isolation checks (exit 0 on success).
- `Any2Odyssey.exe --test-capture-exclusion`: visible diagnostic windows check that the output is excluded from capture and that moved/closed content clears; result in `%LOCALAPPDATA%\Any2Odyssey\capture-exclusion-test.json`.
- `Any2Odyssey.exe --test-offscreen`: verifies a GPU surface outside every physical screen stays live and capturable; result in `offscreen-surface-test.json`.
- `Any2Odyssey.exe --inspect <pid>` and `--layout-probe <parentPid> <workerPid>`: window and projection diagnostics written as JSON to the data folder.
- `Any2Odyssey.exe --check-updates`: one HTTPS request to GitHub for the latest release; prints and logs one line and exits 0 (current or ahead of the latest release), 10 (update available) or 1 (error). Needs network access; nothing is downloaded. The release build is a GUI-subsystem executable: it attaches to the terminal it was started from, so the line may appear after the prompt has returned; the exit code and the `companion.log` line are the reliable results.

## Installer (Windows only)

Build the release, copy `target\release\Any2Odyssey.exe` to `build\`, then run from the repository root in 64-bit PowerShell:

```powershell
.\installer\tests\test-installer.ps1
.\installer\tests\test-install-launcher.ps1
.\installer\tests\test-controls-install.ps1
.\installer\tests\test-install-flow.ps1 [-PreviousPackage <extracted older release>]
.\installer\tests\test-uninstall-flow.ps1
.\installer\tests\test-uninstall-helper.ps1
.\installer\tests\test-uninstall-helper-elevated.ps1   # needs an elevated console; says so and skips otherwise
.\installer\tests\test-dpi-manifest.ps1   # reads target\release\Any2Odyssey.exe, not build\
```

They use temporary folders and an isolated `HKCU:\Software\Any2OdysseyInstallerTests` key, both removed after a passing run; the Samsung engine hash and elevation are simulated, and the tests set `ANY2ODYSSEY_SKIP_OS_CHECK=1` so `install.ps1`'s Windows 11 24H2 gate does not depend on the CI image. No real installation is performed and no Samsung file is read or written. They are safe to run on the PC where Any2Odyssey is installed and running: process discovery and the `--quit` launch are stubbed, the uninstaller's quit event is routed to a test-only name, and each fixture refuses to run ("Fixture routing incomplete") if any real path, registry key or event name is left in the script it is about to execute. `test-uninstall-helper-elevated.ps1` is the one test that runs the worker-removal helper as a real administrator process, through the uninstaller's real `Start-Process` launch, against routed copies in a temporary folder; the real `-Verb RunAs` prompt cannot be automated and stays a hardware check. CI runs the same scripts on `windows-latest`, where the runner account is an administrator, under PowerShell 7 and under Windows PowerShell 5.1.

## Hardware acceptance (target machine)

Not automated. On the Windows PC with the G90XF, Odyssey 3D Hub 1.5.1 and the RTX GPU: full install, automatic start, each capture mode, SBS mode, depth/pop-out apply, Ctrl+Alt+F8/F9/F10, overlay behind a borderless game, recovery after killing the worker, uninstall. Record the results in `CHANGELOG.md` before tagging a release. Each release entry's "Hardware verification" section lists the checks that version's changes added (for 0.8.1: rapid Apply/Restart, 125–200 % and mixed-DPI scaling, HDR/SDR-white-level toggles, monitor rearrangement, tray restore and quit while hidden; for 0.8.2: the About page from all three entry points against `version.json`, Check now on the published and on an older build, the once-a-day startup check and its switch, offline behaviour, the tray tooltip, recovery of a damaged `settings.json` through Apply, the per-launch source choice, and a start with Odyssey 3D Hub holding `OverlaySettings.json`; for 0.8.3 to 0.8.7: see the "Hardware verification" section of each entry). Optical depth must be judged by a person at the monitor; screenshots cannot show it.

Results so far: 0.8.0 passed full install, preference import, automatic start, whole-desktop AI conversion, click-through overlay and Keep 3D above apps on a G90XF with an RTX 5090 (2026-11-08; details in the CHANGELOG). The remaining items in the list above are still to be recorded.

Useful evidence to collect while testing: `companion.log` (with `companion.old.log`, if there is one), `worker-status.json`, `desktop-capture-status.json` (frame count, fps, average capture time) and `install-result.json`, all in `%LOCALAPPDATA%\Any2Odyssey`.

## Pending hardware verification

The changes made in 0.9.1 (audit rounds 11 to 14) are covered by the tests above and have not been run with Wine, the camera, the lens or the monitor. These checks are still to do, on a dual-boot PC with a G90XF; record the results in `CHANGELOG.md`.

Linux installer:

- With the Windows partition mounted: `./install.sh --dry-run --yes`; step 5 must show `would run: .../setup.sh <drive>`.
- A USB stick carrying `Program Files/LeiaSR/...` and `Windows/System32`: expect `not used: ... (a removable drive)`. The same on a memory card in a built-in reader, if the PC has one.
- After the lens stack has run: `install.sh --uninstall`, then `pgrep -f noxon` is empty and Wine programs in other prefixes are still running.
- Install with `--prefix` to another folder, then run the uninstall command the installer printed: that folder's installation is removed and one in `~/.local` is left alone.

The Wine side (`contrib/wine-lens`):

1. Run `setup.sh` again, then `tracking.sh start`: `C:\a2ocam.log` shows "source: created" and "reader: streaming", and the weaver reports `eyes 1`.
2. Stop and restart the weaver: the tracker logs "in pause mode", the camera is released, and it resumes.
3. Kill the bridge while tracking: the log shows "connection closed" and the tracker gets an error instead of hanging. Note whether it reopens the camera.
4. `SIGSTOP` the tracker for 1 s and for more than 10 s: no stale-picture jump; the bridge drops the client and takes a new one.
5. `pgrep -a any2odyssey-noxon` while the stack is up; the lens switches; `stty -F /dev/odyssey-lens -a` shows `-ixon`; the keeper is gone within about 10 s of `wineserver -k`.
6. `A2O_WEAVER_STATS`: weaves per second match the refresh rate.
7. Live screen on KDE: "capture: excluded" still works; killing Any2Odyssey stops tracking at once and the next start works.
8. With another program holding the webcam's stream (for example `ffplay /dev/videoN`), `tracking.sh start`: the camera does not start and nothing hangs. Close the other program while the bridge stays up: the tracker's next attempt reaches "reader: streaming" (the capture start that failed part-way left the camera usable).

The live screen:

1. Still desktop: `top -H` and `nvidia-smi dmon` go idle a few seconds after the last change (up to about 20 s at Steadiness 95).
2. On a still screen each control acts within about a quarter second; Detail triggers exactly one inference.
3. Depth eases in and stops without a jump; removing the model mid-run goes flat and recovers.
4. 2160 lines with video, sharpening on and off: pictures per second; no centre seam; bars and swap correct on a screen that is not 16:9.
5. `pkill -STOP -f any2odyssey-weaver.exe`, then Stop: wrapper and host gone after about 3 s, lens off, SR service alive, next Start works.
6. Stop 5 s into a cold start, then Start again.
   Also Start then Stop within a second: 30 s later `tracking.sh status` shows the tracker and the bridge stopped. And Stop then Start at once: eyes are found in the second session. Note how long `wine sc start` takes to return and how long the tracker takes to leave after `wine sc stop` (the stop waits at most 10 s for it).
7. Ctrl+C in a terminal: lens off, no host left.
8. `env -u XDG_RUNTIME_DIR`: the frame file is in `/dev/shm` and Wine opens it.
9. Cancel the portal dialog: no sharing indicator left; restart works.
10. While live, disable or unplug the captured screen: the page reads "Live capture ended: the screen cast was ended by the desktop." instead of staying on a frozen picture; a normal start still reports "Live: ..." after its first frame.

The viewer:

- Odyssey output in AI mode: open a new image or change depth detail. Expect a black panel with the lens on, then the new picture without pressing Start; repeat with the controls minimised.
- The first AI conversion still works; alter one byte of the data-folder model and expect the "Install it again" message.
- `any2odyssey FILE` while an instance runs, including a file name that is not UTF-8.
- Open the Open... dialog (zenity, and kdialog on KDE), then close the viewer, and separately `kill -9` it: the dialog goes away within a second both times; a normal pick and a cancel still work.
- With a live screen on the Odyssey, choose another output format: the weaver window closes, capture stops and the page reads "A live screen is shown through the Odyssey 3D output only."; choosing the Odyssey again and pressing Start works.
- Lens output on an AI-depth image, then make depth fail (rename the model and change Depth detail): the output closes with the reason, and the next lens output shows a correct picture.

Windows (G90XF PC):

1. A normal start and stop still log "Renderer stopped cleanly".
2. The start line reads "... ms minimum interval (display at X Hz ...)"; `desktop-capture-status.json` shows a steady 60 fps at 120 Hz.
3. The overlay is found and configured at start. Hidden to tray: it stays above apps and the restore hotkey works. Controls open: controls and tray menu stay above the overlay.
4. Rearranging or plugging in a monitor so that the surface overlaps one still stops conversion.
5. An HDR toggle and an SDR-brightness change still restart through recovery.
6. Turning 3D off by hotkey, Apply and the tray commands act promptly; no 7-second kill lines in the log.
7. Camera plugged in: starts. Unplugged: the USB message.
8. Quit then immediate relaunch, `--toggle` / `--show` / `--quit`, an installer upgrade over a running instance: no "Control events unavailable" in the log. Run `Install.cmd` the moment Any2Odyssey has been started: it closes the app instead of stopping with "Could not request a clean shutdown".
9. View logs and the release-page button open Explorer and the browser; View logs still opens the data folder with an empty `Any2Odyssey.cmd` placed in `%LOCALAPPDATA%`.

The workflows run only on GitHub. The pull request that brings these changes is the first run of the pinned Arch container's key set-up and of the dependency job; the first tag after it is the first run of the attestation job and of the Linux gates and the dependency job in `release.yml`. Both need watching.

## UI previews

`Any2Odyssey --screenshot FILE [--dark] [--help-page]` opens the controls with default preferences in an isolated preview (no single-instance guard, no worker, no preference loading or legacy import, no preference saves, no window enumeration, hotkeys not registered), captures the window after a few frames and exits 0, or exits 1 when the image was not delivered or could not be saved. The PNGs under `docs/assets/` were produced this way on Linux; they show the desktop controls, not optical 3D.
