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

`crates/core/tests/state_compat.rs` reads what every release since 0.8.0 saved: `crates/core/tests/fixtures/releases/<tag>/` holds `settings.json`, `ui-settings.json`, `lens.json`, the worker's status files and the installer's `version.json` as that release's own code wrote them (the README there says how each was made, and how to add a release). Every value must load as it was saved, nothing may be called damaged, a save by this release must keep all of it, and the installation records must show on the About page. Two tests look the other way, for the user who goes back to an older release: this release must write, byte for byte, what the newest release in the corpus wrote, and those files must still have every key an older release wrote, with a value of the same kind. `crates/app/src/ui/lens.rs` loads the same lens files the way the viewer does, and `installer/tests/test-record-compat.ps1` reads the installation records the way the installer and the uninstaller do, under both PowerShells.

On Linux the Windows crate is only compile-checked: `cargo check --workspace --target x86_64-pc-windows-msvc`. The pinned toolchain in `rust-toolchain.toml` does not ship that target; add it once with `rustup target add x86_64-pc-windows-msvc --toolchain 1.99.1` before the cross-check or clippy run.

## Linux

`python3 contrib/wine-lens/tests/test-camera-bridge.py` builds the native camera bridge with the system C compiler and tests its loopback authentication: the exchange of random numbers and proofs in both directions, a wrong answer, the bridge's own proof offered as the client's, an answer made for another connection's number, silent and trickling clients, and a client that stops reading. It uses a temporary test secret and `/dev/null` instead of a camera; no Wine, hardware or root access is needed. Both Linux CI architectures run it.

`python3 contrib/wine-lens/tests/test-noxon.py` builds the serial port's keeper (`src/noxon.c`) and runs it against a pseudo-terminal, with a process of a made-up name standing in for Leia's service: flow control is switched off again each time it is switched on, no other line setting is touched, the keeper leaves when that process has gone and waits for a port that is not there. No lens board, Wine or root access is needed. `NOXON_TEST_BINARY` names an already built keeper to test instead.

`python3 contrib/wine-lens/tests/test-mkedid.py` loads `tools/mkedid.py` and checks the monitor name it writes into the registry file for an EDID: a real maker keeps its three letters, and no maker code, including the malformed ones that used to come out as `[`, `\` and `]`, can break the file's syntax. No monitor, Wine or `xrandr` is needed. (Python leaves `tools/__pycache__` behind in the checkout; the packaging scripts leave it out and CI refuses a package that has it.) It also checks that the entries of a monitor that was there at an earlier start and is gone now are deleted.

`python3 contrib/wine-lens/tests/test-mkboard.py` runs `tools/mkboard.py` as a program against a made-up `/sys` (`A2O_MKBOARD_TEST_ROOT` names the folder whose `sys` is read): what it writes into `board.h` and `device.reg` for a lens board, and that descriptors that are cut short or are of another kind, another device's ids and a tty that is no USB device leave both files as they were. No lens board, Wine or root access is needed.

`python3 contrib/wine-lens/tests/test-weaver-watcher.py` runs `any2odyssey-weaver` against stand-ins for the scripts it starts: when the helper is killed outright, the process it left behind still stops eye tracking, and a `flock` without a timed wait (BusyBox) does not turn that process's wait into a busy loop. No Wine, monitor, desktop or root access is needed.

`python3 contrib/wine-lens/tests/test-tracking-stop.py` runs `tools/tracking.sh` against stand-ins for Wine, Leia's service and its eye tracker with chosen delays: a stop waits for a tracker that is slow to leave (so a start right after it really starts one), a stop during a start that is still under way leaves no tracker behind, a tracker that never leaves holds a stop for about 10 s and no longer, and a normal stop is not slowed. `python3 contrib/wine-lens/tests/test-setup-pgrep.py` checks that `setup.sh` stops before creating anything when the only `pgrep` is one without `-u` (BusyBox's), and that `--help` prints the lines at the top of the script instead of being taken for the Windows drive. Neither needs Wine, the monitor or root access.

`python3 contrib/wine-lens/tests/test-start-stack.py` runs `tools/start-stack.sh` and `tools/ensure-stack.sh` against stand-ins for Wine, which keep two things a real Wine (11.21) showed in round 21 with a stand-in for Leia's service. Wine ends a prefix's services when no ordinary program of it has run for as long as its server has patience (three seconds, unless the server was started with `wineserver -pN`): the test fails where two of the script's programs are further apart than that, which is how Leia's service was ended in the middle of its own start. `sleep` is a stand-in too and moves a clock instead of waiting. And a server that is killed outright leaves the prefix's programs behind: the test starts programs under the name of Wine's loader and checks that a start ends the ones of this prefix and nothing else (not another prefix's, not the serial port's keeper), and that Leia's service and session without a server of this prefix are not taken for a running stack. `A2O_START_STACK_TEST_SHELL` names another shell; BusyBox's has a `sleep` and a `pgrep` of its own, and the tests are skipped there. No Wine, monitor or root access is needed; a C compiler is.

The same file keeps what two and more real Wines (11.0 to 11.22, swapped under one prefix in round 22) showed about an upgrade of Wine. A stack whose Wine server's program file has been replaced is not a running stack (after an upgrade while it was up, the old server turned every new program away). Where the copies of Wine's libraries in the prefix are not the installed Wine's, the first program of a start is one that does nothing, run with Wine's own five libraries in place of the stand-ins, so that Wine's update of the prefix does not have to get past them; the copies are replaced after it, and an update that fails is tried again at the next start. And a stand-in whose list of names is not that of the installed Wine's library ends the start before any program of Wine runs, with a reason that `ensure-stack.sh` passes on. `python3 contrib/wine-lens/tests/test-setup-pgrep.py` also checks that `setup.sh` refuses a Wine older than 11.6 before anything is made, and which answers of `wine --version` count as older.

The release workflow runs every test of this section and of the ones below with the same commands as CI, before anything is signed or published.

`python3 packaging/linux/tests/test-install.py` runs the release archive's `install.sh` in temporary folders, with a stand-in for `/` (`A2O_INSTALL_TEST_ROOT`: a mount table, `/sys` block devices and mounted "Windows drives" made of empty files), a fixture program in place of the executable and of `setup.sh`, and no root access (while that variable is set the script uses sudo for nothing). It covers the desktop launcher for awkward prefixes (it needs `gio` and `desktop-file-validate`), `--help`, `--dry-run`, the prefix without GNU `readlink`, and the choice of a Windows drive: the one internal drive used with `--yes`; USB drives, memory cards, flagged-removable disks and mapped volumes over them not used; several candidates listed and none chosen; a network share or a user's FUSE mount refused with `--yes`; a folder without a Windows system folder or with another user's files (that case needs root) passed over; `--windows-root` taken as given. It also covers what `--uninstall` stops and in which Wine prefix, and two runs in one prefix: an installation or an `--uninstall` started while another installation is copying waits for it (through `flock`; skipped without it), Ctrl+C on a pseudo-terminal ends a run that waits, and the lock is given up before the program, the Wine setup or Wine's server is started. Three tests run without the stand-in for `/`, because with it the installer never comes to the package manager's command: they are dry runs (`--dry-run` runs and writes nothing) with a stand-in `dnf` or `zypper` as the one package manager on the PATH, and check that the command names the option that leaves out a package the lists do not have. `A2O_INSTALL_TEST_SHELL` runs the same tests under another shell (`dash`, `"busybox sh"`). CI runs them twice with the system's `sh`: in the Linux job as an ordinary user (dash; the two tests that need root are skipped there), and in the Arch job as root (bash), where a skipped test fails the step. Up to 0.9.4 the two tests that need root ran in no job. In a test run the installer also looks for the udev rules, the lens board and the webcam under the stand-in for `/`; it uses no sudo there and names its `udevadm` calls instead of running them.

`scripts/check-action-pins.sh` asks GitHub whether each commit by which the workflows name an action is still under a tag or on a branch of that action's repository (it needs the GitHub CLI and a token in `GH_TOKEN`); a commit that a rebuilt branch has left behind, as the `dtolnay/rust-toolchain` commit of 0.9.4 was fourteen hours after that release, or one that was only pushed to a fork, fails it. `python3 scripts/tests/test-check-action-pins.py` runs that script against a stand-in for `gh` that answers from files, with the answers it has to refuse: such a commit, a tag or a short id in place of a commit, an answer that never arrives, and one that arrives only at the second try (the stand-in then fails first the way `gh` does, with the error's body on standard output). `A2O_TEST_SHELL` runs it under another shell. CI runs both in its dependency job.

`THIRD-PARTY-LICENSES.md` is written by hand; `scripts/third-party-linux.py` compares its two tables and the number in its text with the crates of a Windows build before it writes the Linux list, and CI's Linux job runs it. `python3 scripts/tests/test-third-party-linux.py` runs that comparison against a stand-in for `cargo` with what it has to refuse: a crate that is missing, one too many, a license that is not the manifest's, a wrong number, and a crate that a table names twice. CI runs it in its dependency job.

`python3 scripts/tests/test-apt-install.py` runs `scripts/apt-install.sh`, which the workflows install their Ubuntu packages with, against stand-ins for apt-get, timeout and sudo: a download that is stopped or fails is tried again, and the third failure fails the step. CI runs it in its dependency job.

The programs name the source files of their dependencies in panic messages by the path each file had where the program was built. The workflows and the `PKGBUILD` give the build machine's folders other names (`--remap-path-prefix`: `/cargo/registry/src/...` for cargo's folder, `/any2odyssey/...` for the checkout; `cargo\...` and `any2odyssey\...` on Windows), so a published program holds no path of the runner and `makepkg` no longer reports a reference to `$srcdir`. On Linux the flags are in `RUSTFLAGS` for the whole job; on Windows they are written to the account's cargo configuration, because `RUSTFLAGS` would take the place of the flags in `.cargo/config.toml` (the static C runtime) where configuration joins them. `scripts/check-build-paths.sh PROGRAM FOLDER...` fails when a program names one of the folders given, and CI and the release workflow run it on the Windows executable, the program in the Linux archive and the one the Arch package installs, with the runner's home folder and checkout. `python3 scripts/tests/test-check-build-paths.py` gives that script stand-ins for a program with what it has to refuse and to accept (`A2O_TEST_SHELL` runs it under another shell); CI runs it in its dependency job. A build made by hand (`cargo build --release`) is as before: it holds the paths of the machine it was made on.

`cargo deny --locked check advisories bans sources licenses` (cargo-deny 0.20, policy in `deny.toml`) checks `Cargo.lock` against the RustSec advisory database (vulnerabilities, unsound code and yanked versions anywhere in the tree; unmaintained crates only among direct dependencies), refuses wildcard version requirements and any source but crates.io, and refuses a crate whose license is not in the list `deny.toml` gives with its reasons (the licenses the tree has today, each one that the GPL-3.0 can be distributed with; a license no crate states any more fails it too, so the list stays exact). CI runs it as its own job, and so does the release workflow, where the signing and publishing jobs wait for it. It fetches the advisory database, so a new advisory fails it without any change here (on a tag too, before anything is published): update the crate, or list the advisory under `ignore` in `deny.toml` with the reason.

The UDP shim's wrappers can also be checked without Wine or hardware:

```sh
cc -std=c11 -O1 -g -Wall -Wextra -Werror -fsanitize=address,undefined -pthread \
  contrib/wine-lens/tests/test-connreset.c -o /tmp/test-connreset
ASAN_OPTIONS=detect_leaks=0 /tmp/test-connreset
```

This runs the production send/receive/close wrappers against a simulated Winsock provider, including completion before the receive call returns, recycled receive storage, and socket handle reuse. It does not exercise import routing or Leia's tracker. After changing these wrappers, verify pause/resume and camera release on the target Wine installation, including a receive submitted before the first destination is known.

Building needs the PipeWire development files and clang beside the desktop libraries (Ubuntu: `libpipewire-0.3-dev libclang-dev clang`; Arch: `libpipewire clang`). `cargo test -p any2odyssey-core -p any2odyssey` runs everywhere and starts no helper, capture or model. One of its suites, `crates/app/tests/outputs.rs` (Linux and other Unix systems), starts the built program itself, in modes that only print and leave (`--help`, a usage error, the Windows-only diagnostics), with both outputs at a pipe nobody reads, and compares the exit codes; it opens no window, reaches no running instance and logs into a folder of its own. Four measurements are kept as ignored tests for a machine with the hardware: `live::bench` (stage timings, needs the depth model), `depth::tests::the_model_runs_where_it_says_it_does` (the model itself, a map of each kind, and whether it ran on the NVIDIA GPU or on the processor and why; needs the depth model and ONNX Runtime), `capture::probe` (what the screen cast of the remembered screen contains) and `live::probe` (the whole live path against the Odyssey; `PROBE_ROUNDS`, `PROBE_LINES`, `PROBE_DETAIL`, `PROBE_SECONDS`). Run one with `cargo test --release -p any2odyssey <name> -- --ignored --nocapture`. Two more need neither the model nor the monitor and are the ones to run before and after a change to the hot paths: `live::bench::turn_timings` (a turn of the live loop on a 4K screen that changes, stage by stage, with the hand-over to the frame file) and `weaver::bench` (a still picture composed for the Odyssey 3D output and handed over). `scripts/package-linux.sh` stages the Linux release archive from a release build and `scripts/third-party-linux.py` writes `THIRD-PARTY-LICENSES-LINUX.md`; CI runs both and refuses a list that no longer matches the dependencies. What has been checked on a G90XF is recorded in `docs/LENS-SWITCH.md` ("What has been checked") and `docs/PLATFORMS.md`.

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
.\installer\tests\test-record-compat.ps1   # version.json of every release since 0.8.0, and this one's; needs no build
.\installer\tests\test-install-launcher.ps1
.\installer\tests\test-controls-install.ps1
.\installer\tests\test-install-flow.ps1 [-PreviousPackage <extracted older release>]
.\installer\tests\test-uninstall-flow.ps1
.\installer\tests\test-uninstall-helper.ps1
.\installer\tests\test-uninstall-helper-elevated.ps1   # needs an elevated console; says so and skips otherwise
.\installer\tests\test-dpi-manifest.ps1   # reads target\release\Any2Odyssey.exe, not build\
```

The setup program (`installer/Any2Odyssey.iss`) is built with `.\scripts\package.ps1` followed by `.\scripts\build-setup.ps1`, which needs Inno Setup 6.3 or newer. What it adds to the launcher, the report it reads back and the checks-only run, is covered by `test-install-launcher.ps1`. `installer\tests\wizard\test-setup-wizard.ps1` covers the wizard's own decisions and is as safe as the tests above: it compiles `Any2Odyssey.iss` around the real launcher and a stand-in `install.ps1` that writes only into the test's temporary folder, and runs that program silently with `/CONTROLSONLY`, so nothing is installed and nothing asks for elevation. It checks the exit codes (0, and 7 for a refusal), the installer's reason and lines in Setup's log, `/ALLOWDOWNGRADE`, PowerShell being ended during the checks and during the installation, that a notice is shown once, and that Setup removes its unpacked package. It needs Inno Setup's compiler and is skipped without it (`-RequireIscc`, which the workflows pass, makes that a failure); security software that holds an unknown program can make it take a few minutes. What it cannot cover is the wizard's interactive pages: the question before the elevation prompt and the Back button were checked by hand with the same kind of stand-in. `installer\tests\live\test-setup-program.ps1 -AllowRealInstall` runs the built program itself and is **not** like the tests above: it installs, repairs and uninstalls the desktop controls for the current Windows account, refuses to start where Any2Odyssey or Odyssey 3D Hub is installed, and is meant for a disposable PC. CI runs it on its runner in the Windows job, after the tests above.

The tests above use temporary folders and an isolated `HKCU:\Software\Any2OdysseyInstallerTests` key, both removed after a passing run; the Samsung engine hash and elevation are simulated, and the tests set `ANY2ODYSSEY_SKIP_OS_CHECK=1` so `install.ps1`'s Windows 11 24H2 gate does not depend on the CI image. No real installation is performed and no Samsung file is read or written. They are safe to run on the PC where Any2Odyssey is installed and running: process discovery and the `--quit` launch are stubbed, the uninstaller's quit event is routed to a test-only name, and each fixture refuses to run ("Fixture routing incomplete") if any real path, registry key or event name is left in the script it is about to execute. `test-uninstall-helper-elevated.ps1` is the one test that runs the worker-removal helper as a real administrator process, through the uninstaller's real `Start-Process` launch, against routed copies in a temporary folder; the real `-Verb RunAs` prompt cannot be automated and stays a hardware check. CI runs the same scripts on `windows-latest`, where the runner account is an administrator, under PowerShell 7 and under Windows PowerShell 5.1.

## Hardware acceptance (target machine)

Not automated. On the Windows PC with the G90XF, Odyssey 3D Hub 1.5.1 and the RTX GPU: full install, automatic start, each capture mode, SBS mode, depth/pop-out apply, Ctrl+Alt+F8/F9/F10, overlay behind a borderless game, recovery after killing the worker, uninstall. Record the results in `CHANGELOG.md` before tagging a release. Each release entry's "Hardware verification" section lists the checks that version's changes added (for 0.8.1: rapid Apply/Restart, 125–200 % and mixed-DPI scaling, HDR/SDR-white-level toggles, monitor rearrangement, tray restore and quit while hidden; for 0.8.2: the About page from all three entry points against `version.json`, Check now on the published and on an older build, the once-a-day startup check and its switch, offline behaviour, the tray tooltip, recovery of a damaged `settings.json` through Apply, the per-launch source choice, and a start with Odyssey 3D Hub holding `OverlaySettings.json`; for 0.8.3 to 0.8.7: see the "Hardware verification" section of each entry). Optical depth must be judged by a person at the monitor; screenshots cannot show it.

Results so far: 0.8.0 passed full install, preference import, automatic start, whole-desktop AI conversion, click-through overlay and Keep 3D above apps on a G90XF with an RTX 5090 (2026-11-08; details in the CHANGELOG). The remaining items in the list above are still to be recorded.

Useful evidence to collect while testing: `companion.log` (with `companion.old.log`, if there is one), `worker-status.json`, `desktop-capture-status.json` (frame count, fps, average capture time) and `install-result.json`, all in `%LOCALAPPDATA%\Any2Odyssey`.

## Pending hardware verification

The changes made in 0.9.1 to 1.0.3 (audit rounds 11 to 24, the Windows setup program, and the five pull requests of 1.0.0) are covered by the tests above and have not been run with Wine, the camera, the lens, the monitor or a real Wayland desktop. These checks are still to do, on a dual-boot PC with a G90XF; record the results in `CHANGELOG.md`.

Some of the Windows checks below end with a note *Seen in a window*. In audit round 22 the Windows controls were run in their own window on Windows 11 without the monitor: a build with no conversion engine, on a desktop that is never shown, with input posted to the window as messages, a display scale of 150 or 200 % given to the window by the message Windows sends for one, and pictures taken by the program itself. What a note names was seen there; what it names as not seen is still to do, and so is every item without a note.

Since 1.0.3 (unreleased; measured from outside on 1.0.3, none of the changes seen on the monitor):

- Linux, live screen, the Odyssey's own screen on KDE Plasma, a film of 24 pictures a second: pans are as even as on the screen without 3D. Measured from outside while it runs, the pictures reach the output 41.7 ms apart, give or take a few, and no longer 32 and 48 (the sequence number at byte 12 of `$XDG_RUNTIME_DIR/any2odyssey-weaver-*.frames` rises by 2 for every picture; 1.0.3 gave 597 gaps of 32.1 ms and 841 of 48.4 in a minute). A video of 60 a second: 16.7 ms apart, without a gap of 32 twice a second.
- Linux, live screen, a game or a test picture at 120 a second: 60 pictures a second on the page, even to the eye. The page's rate for a still desktop is as before ("not changing, so nothing is redrawn"), and the capture thread (`top -H`, `screen-capture`) uses less of a core under a film than in 1.0.3 (22 % there).
- Linux, live screen on GNOME and on a wlroots desktop: the live screen starts and runs as before now that 125 frames a second are asked for (KDE Plasma 6.8.3 and PipeWire 1.6 are what it was built on).
- Linux, the G90XF, live screen at 1080, 1440 and 2160 lines, with and without sharpening, a screen of another shape than 16:9 among them: the picture is whole, the black beside it is black (also right after the lines or the screen change), no tearing, no stale stripes. At 2160 lines a video of 60 a second now reaches 60 pictures a second on the page.
- Linux, after `any2odyssey-setup` and a start of the stack: log files of more than two weeks ago are gone from `ProgramData/Simulated Reality/*/Log` in the prefix, newer ones and everything else are there, and the stack starts as before.
- Linux, the G90XF, `env A2O_WEAVER_STATS='Z:\tmp\weaver-stats.txt' any2odyssey` (after `any2odyssey-setup`), live screen at 2160 lines: note the first line (`latency`), and `late turns` and `mean upload` with a film and with a video of 60 a second. Then the same with `A2O_WEAVER_LATENCY_US` 8000, 17000, 25000 and 60000 while moving the head from side to side, each line's `latency` saying the number that was asked for: say at which the picture follows best, and whether 60000 is visibly worse (if it is not, the setting does nothing here).

New in 1.0.3 (one change, a film's black bars on a live screen; none of it was seen on the monitor before the release, and 1.0.2 is the release without it):

- Linux, live screen, a film with black bars above and below, full screen, with 1.0.2 beside it if possible: the bars lie at the back and do not float in front of the film, and the film has more depth than in 1.0.2, a wide shot most of all. The edge between the film and a bar is clean: no line of the film lies back with the bar, and no line of the bar stands forward.
- Linux, live screen, the same film with subtitles in the lower bar: the film's depth does not pump when a line comes and goes. The subtitles lie at the back with the bar, behind the film: say whether that reads well. The same with the player's controls shown over a bar, and with the pointer resting in one.
- Linux, live screen, a film or a game with a dark scene and no bars, full screen: nothing of the picture is flattened or pushed back at its edges, and a fade to black and back shows nothing odd.
- Linux, live screen, an upright video in a player with black at both sides: the black lies at the back, the video keeps its depth.
- Linux, live screen, a desktop in a dark theme with windows: panels and title bars lie where they did in 1.0.2.

New in 1.0.2:

- Linux, the G90XF, `any2odyssey-setup` once after the update (`setup.sh`, `start-stack.sh` and `ensure-stack.sh` changed), with eye tracking: it ends with "done", and prints no line about a wine that "is not kept beside Wine's libraries" where the `wine` is a distribution's or WineHQ's; there is no `wine-lens/wine.libs` in the data folder then. This is the first thing to do.
- Linux, the G90XF, the first Start output after a login: the picture comes with eye tracking (the page shows no "tracking:" reason), and the start takes no longer than it did. If it takes about three seconds longer, Leia's session has another name among the processes than the two the start knows: `pgrep -af SRSession` while the output is on says which, and that is worth a report.
- Linux, the G90XF, Start output twice in a row: the second is there at once. `wine-lens/stack.started` in the data folder holds one number, the same that `stat -c %Y /dev/odyssey-lens` prints. Then the monitor into standby and back with the output off: the next Start output is a cold start again and shows the picture.
- Linux, the G90XF, the lens output and the Odyssey 3D output asked for at the same moment from a stack that is down, if there is a way to do that by hand: one start of about half a minute, and both come.
- Linux, live screen at *Depth steadiness* 60, with 1.0.1 beside it if possible. A desktop with windows at different depths and nothing moving but a clock or a cursor: the whole picture should breathe in depth about half as much as in 1.0.1. A tooltip or a small window that opens over a far background: the rest of the picture moves less than in 1.0.1. A film with hard cuts: the new scene has its depth within half a second on the processor, where 1.0.1 took about a second and up to three. And the one thing 1.0.1 did better by the numbers: a video that is paused and a window dragged in over it: the background steps back at once by about 14 pixels on a 3840-wide view, where 1.0.1 had about 10. This replaces the check of the following range under "New in 1.0.1".
- Linux, live screen at *Depth steadiness* 0 across a hard cut: nothing of the scene before is left in the first picture of the new one.
- Linux, KDE Plasma or GNOME, with the output stopped: `head -c 22 /dev/zero > ~/.local/share/any2odyssey/capture-token`, then Start with *Live screen*: the desktop's dialog comes as at a first start, the screen shows, and the next Start asks nothing.
- Linux, where someone has it: a `wine` on the PATH that is a script of their own. The set-up prints "the wine on the PATH is not kept beside Wine's libraries; they are taken from" with the folder of the Wine that the script starts, and the output starts.
- Windows: a right click on the taskbar button of the running Any2Odyssey shows "Turn 3D on / off" and "Open 3D controls". (The list was read back from Windows with both titles; the menu itself was not looked at.)

New in 1.0.1:

- Linux, the G90XF, `any2odyssey-setup` once after the update (`setup.sh`, `start-stack.sh`, `ensure-stack.sh`, `tracking.sh` and `mkedid.py` changed), with eye tracking: it ends with "done". This is the first thing to do, and the one check of the release that can stop a set-up that worked: the set-up now asks Wine whether the camera's entries are in the registry (`wine reg query`) and stops when Wine says no. If it stops there on a machine where 1.0.0's set-up ended well, that is a defect of 1.0.1.
- Linux, the G90XF, two cold starts of the lens helper or of the Odyssey 3D output one after the other (quit the program between them): both come in about the usual half minute, and neither ends with "did not get the lens board ready". The start now waits for a line that Leia's service wrote during this start, not for one in its newest log. Look in Leia's log folder whether the service starts a file for each run or goes on in the one before.
- Linux, the G90XF, with 3D running: the monitor into standby and back, then the lens or the Odyssey 3D output asked for again: it starts again and comes on.
- Linux, the G90XF, a second monitor connected at one start and unplugged before the next: `edid.pairs` in the Wine side's folder shows the monitors that are there, and the 3D picture is on the Odyssey.
- Linux, the G90XF, eye tracking on, then `rm $XDG_RUNTIME_DIR/any2odyssey-camera-bridge.pid`: `tracking.sh status` still says the bridge runs, `tracking.sh start` prints "on", and `tracking.sh stop` ends the bridge.
- Linux, live screen at *Depth steadiness* 60: something small and near that comes into the picture or leaves it (the pointer over a far background, a tooltip, the edge of a window): the rest of the picture keeps its depth and does not step back or forward as a whole. This is the change of the release that only eyes can judge. (1.0.2 measured it with the depth model and took the following range back: the check under "New in 1.0.2" replaces this one, and what follows here about a hard cut stands.) Then a hard cut between two scenes of quite different depth (two videos, or a switch between a desktop and a full-screen picture): the new scene has its depth at once, with no spell of some seconds in which it is flat or its near things are all equally near. The same at steadiness 0 and 95.
- Linux, live screen, a screen on which nothing changes: the page reads "not changing, so nothing is redrawn" after two or three seconds at steadiness 60, as in 1.0.0, and the depth does not creep while it gets there.
- Linux, live screen, *Finest* on a 3840 × 2160 screen and back to *Standard*: the program's memory goes down again (1.0.1's notes said by about 36 MB; 13 to 26 MB were measured for 1.0.2). Stop and start the live screen quickly: the depth comes once, without a second pause for the model.
- Linux, Lens output, F11 for full screen, *Output format* to Odyssey 3D and back to Lens: the new window is a window, with the F11 hint, and the lens is off; F11 makes it full screen and the lens comes on; Esc in the window closes the output. Once more with Anaglyph.
- Linux, the Odyssey 3D or Lens output on, quit from the menu and with `any2odyssey --quit`: the helper's window goes, the lens is off, the process is gone within about five seconds, and `companion.log` has no line "The window did not close after the quit request". With a helper that does not leave (`ANY2ODYSSEY_LENS_HELPER` naming a script that ignores the end of its input): it is ended after about three seconds.
- Linux, a PNG without an ending, one named `.jpg`, and one named `.png.part`, opened from the file manager and dropped on the window: each opens. *View logs*: the folder opens and the window does not stand still while it does.
- Linux, Arch with an NVIDIA card and `onnxruntime-cpu` installed, a package the set-up needs removed (`zenity`, say), `any2odyssey-setup --yes`: the missing package is installed, `onnxruntime-cpu` stays, and a note names the command for the CUDA runtime.
- Linux, an archive installation beside the package, `~/.local/share/any2odyssey/install.sh --uninstall --prefix ~/.local`: the question about the Wine side reads `[y/N]`, Enter keeps it, and the packaged program still starts the Odyssey 3D output.
- Windows, the full converter starting, *Restore overlay* (Ctrl+Alt+F8) pressed before the 3D picture is there: the picture is on top when it comes, and the log has "Restored 3D overlay". The uninstall started from a 32-bit program (`%SystemRoot%\SysWOW64\WindowsPowerShell\v1.0\powershell.exe -File "%LOCALAPPDATA%\Programs\Any2Odyssey\Uninstall.ps1"`): it removes the program and does not stop at "Use 64-bit PowerShell".

New in 1.0.0:

- Linux, live screen at *Standard*, *Depth steadiness* at its default (60), a window dragged across the shown screen or a video with a figure that crosses the picture: the depth of what moves is where it is, and no shape of it stays behind where it was a moment ago. The same at steadiness 95. This is the change of the release that only eyes can judge.
- Linux, live screen, the same settings with nothing moving for a minute (a desktop with windows at different depths, then a paused video): flat surfaces and the edges between near and far are as calm as in 0.9.9. If an edge or a surface now jumps between two depths where it was calm before, note the steadiness and what was shown: a change of more than about a tenth of the depth range is no longer calmed.
- Linux, live screen of a screen more than twice as wide or high as the picture (a 5120 × 1440 screen at 1080 lines, a 3840 × 2160 screen at 720 lines), a page of small text or a spreadsheet scrolled slowly: thin lines and small letters keep their weight as they move, and do not come and go. The page still reads about 60 pictures a second.
- Linux, live screen of a 3840 × 2160 screen at 1080 lines, or of a 1920 × 1080 screen: as in 0.9.9 (these are made as before).
- Linux, `install.sh` started in two terminals at once over an installation: the second says that another installation is at work and waits; when the first is done it goes on and ends with its usual lines. (That a run which has waited five minutes now stops with "could not acquire the busy lock" needs a first run that hangs, and is covered by the tests.)
- Linux, the G90XF, `any2odyssey-setup` once after the update (`tools/mkboard.py` of the Odyssey 3D set-up changed): it ends with "done", and the lens helper and the Odyssey 3D output start as before.

New in 0.9.9:

- Linux, the G90XF, the Odyssey 3D set-up (`setup.sh`, `start-stack.sh` and `ensure-stack.sh` changed, and they are the ones seen working there): run `any2odyssey-setup` once after the update; it ends with "done". A cold start of the lens helper and of the Odyssey 3D output takes about half a minute as before, a second request is immediate, and eye tracking comes.
- Linux, the G90XF, after the next upgrade of Wine: the first Start takes about 36 seconds (Wine updates the prefix by itself first), the picture comes, and the next Start is immediate. With the output used in the session before the upgrade: the first request afterwards is one cold start, not "the helper stopped unexpectedly".
- Linux, the G90XF, Odyssey 3D output with a still picture and with a live screen: the picture is there as in 0.9.8, and `ls -l $XDG_RUNTIME_DIR/any2odyssey-weaver-*.frames` shows `-r--------`.
- Linux, 2D to 3D on an image: under *Depth detail* the page reads "Depth is computed on the NVIDIA GPU.", and View logs has "Depth model: runs on the NVIDIA GPU". With `CUDA_VISIBLE_DEVICES= any2odyssey`: 2D to 3D works, and the line says that the processor is used and why.
- Linux, live screen at *Standard*, something moving on the shown screen: the page's "depth … times a second" is several times what 0.9.8 showed, the depth of what moves follows it sooner, and *Fine*, *Finer* and *Finest* differ from each other again. The depth of a page of small text scrolling slowly does not shimmer.
- Linux, `nvidia-smi` while a live screen runs at *Finest*, after Stop output, and after *Standard* is chosen again: the program's memory on the card goes down each time. `top -H`: how busy the `live-depth` thread is while a map is computed on the card (one core was busy in WSL).
- Linux, KDE Plasma, live screen with the 165 Hz Odyssey or another screen faster than 60 Hz as the shown screen, a video or a scrolling page on it: the page reads 62 to 64 pictures a second, and the motion has no small hitch a few times a second.
- Linux, KDE Plasma, live screen with the Odyssey's own screen: as before, the 3D picture is not in the capture and the page carries no warning. On a desktop that is not KDE: the page reads "This desktop cannot keep the 3D picture out of a capture: do not show the Odyssey's own screen." for as long as a live screen shows.
- Linux, GNOME, live screen of another screen with *Draw the pointer into the picture* off: move only the pointer for ten seconds: the page reads "not changing, so nothing is redrawn". A clock or a blinking cursor on any desktop still shows each change.
- Linux, Save preview PNG… with zenity installed: the dialog opens in the folder the program was started in (the home folder from the launcher), not at "/".
- Linux, Hyprland, if one is at hand: a live screen runs there at all, which no round has seen; Start, then Esc in the picker, reads "Live capture ended: no screen was chosen."
- Either edition: right-click the Controls page (Windows) or the viewer (Linux), on the page background and on an empty part of a card: the menu opens; choose *About*. On a touch screen a finger still drags the page, and holding a finger on the page opens the menu.
- Linux: change a choice of the viewer, minimize the controls within a second, wait two seconds: `viewer-settings.json` has the change.
- Windows, two monitors with different scaling, or one whose scale is changed in Settings while the controls are open: on each display the window can be dragged down to 700 × 360 points and no further.
- Windows, any PC: open the desktop shortcut's Properties in a program that holds the file (or deny yourself Write on it), uninstall from Settings > Apps: the window names the shortcut and waits for a key.
- After the release: `strings /usr/bin/any2odyssey | grep -c /home/` gives 0, and `makepkg` prints no warning about `$srcdir`.

New in 0.9.8:

- Linux, the G90XF, after the next upgrade of Wine (or after `wine reg add "HKLM\Software\Microsoft\WindowsRuntime\ActivatableClassId\Windows.UI.Notifications.ToastNotificationManager" /v DllPath /d "C:\windows\system32\wpnapps.dll" /f` in the set-up's prefix): Start output brings the picture after one cold start, `SRSession.exe` is among the processes, and a second Start is immediate.

- Linux viewer: choose *Finer* depth detail and 2160 lines (live screen) or another output, wait two seconds, quit and start again: the choices are as they were left, the output is off, and `viewer-settings.json` is in the data folder. `--screenshot` shows the default choices and does not change the file. (Not a check for Windows: the viewer's page cannot be reached there. *Seen in a window:* the title bar, F1 and the tray menu lead to Controls, Help and About only; no `viewer-settings.json` is written, and one that is there, changed, damaged, read-only or a folder, is left as it is. The choices of the Controls page, the theme and the update switch were as left after a quit and a start, and after each way the program ends: Quit of the tray menu and of the page, the close button with and without the tray preference, `--quit`, the process ended, and the messages of a logoff.)
- Linux, live screen, a 3840 × 2160 or wider screen with a page of small text scrolling slowly: the depth of the text's window does not shimmer.

New in 0.9.7:

- Linux, the G90XF, the Odyssey 3D set-up (three of its scripts changed, and they are the ones seen working there): a cold start of the lens helper and of the Odyssey 3D output takes about half a minute as before, and a second request is warm. `ps` shows `wineserver -p60` and no `start-stack.sh` left beside `SRSession.exe`.
- Linux, the G90XF, with the output on: `pkill -9 wineserver`, then Stop output and Start output: the picture comes back after one cold start, with eye tracking.
- Linux, the G90XF: `sleep 60 | ~/.local/share/any2odyssey/any2odyssey-lens-helper` after a login: the lens is on for about half a minute.
- Linux, the G90XF, eye tracking: start and stop the output three times in one login: the eyes are found each time, and the tracker's log says "in pause mode now" at each stop. (With a stand-in tracker under Wine the pause did not survive a second round; on the monitor it was seen working.)
- Linux, KDE Plasma, live screen: log out and in once after updating. Start: the page reads the shown screen's own size ("Live: a 3840 × 2160 screen"), not 1920 × 1080, and small text is sharper at 2160 lines of detail. Note the pictures a second: each frame now has up to four times the pixels.
- Linux, KDE Plasma, live screen with the 165 Hz Odyssey as the shown screen and a video playing: the page reads about 60 pictures a second, and the viewer's share of the processor is lower than with 0.9.6.
- Linux, KDE Plasma: turn the shown screen by 90 degrees in System Settings, then Stop and Start: the picture is upright with black bars at the sides. (Turned while live it keeps the old shape until Stop and Start.)
- Linux, KDE Plasma, another screen shown, with "Draw the pointer into the picture" on: the pointer is in the picture; with it off it is not.
- Linux, KDE Plasma without zenity: Save preview PNG… opens kdialog in the folder the program was started in, with the file name in the Name box.
- Linux, sway or Hyprland, if one is at hand: `output ... transform 90` on the shown screen: upright on the Odyssey, and the page names the size with its sides swapped.
- Linux, GNOME: after `systemctl --user restart xdg-desktop-portal-gnome` the first Start works; a 60 Hz screen with a video playing reads about 60 pictures a second.
- Linux, live screen: change the shown screen's resolution to something larger and back: `ps -o rss` of the viewer is back where it was a few seconds later.
- Linux, a desktop: open a file whose name is Japanese, Chinese or Korean: the path box shows the name in its letters.
- Linux, a real session: `XDG_RUNTIME_DIR=/etc/hostname any2odyssey` in two terminals: the second hands over and exits, and `any2odyssey --quit` ends the first.
- Windows, any PC: open a window whose title is Japanese, one Chinese, one Korean (a browser tab on such a page will do), choose *Selected window* and open *Source window*: the titles are in their letters, on the line of the Latin text beside them, and boxes are seen for a moment at most the first time. `companion.log` names the typeface file that was read, and the Maximize button's square looks as it did. *Seen in a window,* with windows made for the purpose (a Japanese, a Simplified and a Traditional Chinese and a Korean title, one with all of them, a very long one, an empty one, one renamed while the list was open): the titles are in their letters in both themes, on the line of the Latin text; the first picture, a fifth of a second after the list opened, has boxes and the next has letters; the log names `YuGothM.ttc` and `malgun.ttf`; the title bar is the same pixel for pixel before and after. Not seen: a browser's tab, and a PC whose display language is Chinese, Japanese or Korean.
- Windows: a Hebrew and an Arabic title: letters, in reversed order (known). *Seen in a window.*
- Windows at 200 % on a 1920 × 1080 screen, or at 150 % on 1366 × 768: the controls open inside the screen, and the window can be dragged down to 360 points of height, where each page scrolls. *Seen in a window* at 100 %, and at 150 and 200 % by the message: in a window of 700 × 360 points the wheel brings the last line of Controls, Help (topics and text) and About into view, and Tab keeps the focus in view at each of its 21 stops. Not seen: the window opening on such a screen, and a drag by hand.
- Either edition, a window made shorter than its page: click a checkbox and the primary button while the mouse is still moving: the page stays where it is. The wheel and the scroll bar scroll it; on a touch screen a finger drags it. *Seen in a window* on Windows: with the pointer travelling 3 or 4 pixels while the button is held, a checkbox, the primary button and Apply each acted and the page moved by 0 pixels, where a build of 0.9.6 given the same messages moved by 5 to 7; the wheel and the scroll bar scroll it. Not seen: a touch screen.
- Windows: mark a shortcut to Any2Odyssey as hidden (Properties) and uninstall: it is gone with the rest.
- Arch, from a script without a terminal: `./install.sh --yes --no-model --no-odyssey` installs the packages it names without stopping at pacman's question.

New in 0.9.6:

- Linux, the G90XF, a still picture on the Odyssey 3D output: the picture is as it was (upright, both views, black borders for a picture that is not 16:9), and dragging a depth slider follows the pointer.
- Linux, the G90XF, live screen at 1080, 1440 and 2160 lines, with and without sharpening: the picture is as it was, with no tearing and no black frames while a video plays, and `A2O_WEAVER_STATS` shows the weaves a second as before. This is the check that Wine's view of the frame file shows what is copied into its memory. With `env -u XDG_RUNTIME_DIR` the frame file is in `/dev/shm` and the picture shows.
- Linux, the same: with the runtime folder filled to within 30 MB, start the output at 2160 lines: the page says that the picture could not be written, and the viewer goes on. In `top` the viewer's RES is larger by the frame file's pictures while the output is on and falls back when it is stopped.
- Linux, the lens-interleave output: the picture is as it was.
- Linux, KDE Plasma, live screen with a remembered screen: "The screen was chosen. Waiting for its first picture" shows for a moment at most, then "Live: ...".
- Linux, KDE Plasma, live screen: change the shown screen's resolution, refresh rate and rotation: the picture goes on, the page names the new size, and neither "Live capture ended" nor a lasting "paused" appears. (A rotated screen is shown on its side on sway; note what Plasma does.)
- Linux, KDE Plasma, live screen: disable or unplug the shown screen: "Live capture ended: the screen cast was ended by the desktop.", and Start works again when the screen is back.
- Linux, live screen: `systemctl --user restart wireplumber`: the picture comes back by itself; if that takes more than three seconds the page says meanwhile that the desktop has paused the cast.
- Linux, live screen: `kill -STOP` of `xdg-desktop-portal-kde`, then Stop output: "Output stopped" and the lens off after about two seconds. After `kill -CONT`, Start works.
- Linux, live screen with HDR or ten bits a colour on the shown screen: the live screen starts and its colours are right.
- Linux viewer, with the lens output chosen: change *Lens pitch*, then click the empty card beside *Reset lens numbers*: the number stays. Tab from the top: the page scrolls when the focus reaches *View logs*.
- Linux, without a runtime folder (`env -u XDG_RUNTIME_DIR`) and with `XDG_DATA_HOME` in a folder more than about 90 characters deep: start the viewer, open a picture from the file manager: it arrives in the window that is open, and `any2odyssey --quit` closes it.
- Linux, a real terminal: start `./install.sh --yes --no-odyssey` in two terminals at the same moment and press Ctrl+C in the one that says it waits: it ends at once, and the other finishes with Done.
- Windows, the G90XF PC: start Narrator (Ctrl+Win+Enter) with the controls open and Tab through the page: "About, button" comes first, then "Depth strength, slider" and "Capture, combo box" with their values. Switch 3D on and off once with Narrator running: the tree is new code in the window that also owns the tray icon and the shortcuts.
- Windows: click the empty card to the right of *Convert automatically when this app opens*: nothing is ticked. Click the text: it is. *Seen in a window:* clicks 20, 60 and 150 pixels to the right of the text tick nothing; a click on the text ticks the box and `settings.json` has it.
- Either edition: press Ctrl+Plus ten times in the default window: the page grows to 1.7 times and stops, with the title bar whole; Ctrl+0 brings it back. *Seen in a window* on Windows.
- Either edition, both themes: open a drop-down list and read the selected entry; move the pointer over the primary button and read it. *Seen in a window* on Windows, measured in the program's own pictures: the selected entry 7.2:1 in the light theme and 5.1:1 in the dark one, the primary button under the pointer 5.7:1 or more in the light theme and 4.7:1 in the dark one.
- Windows: update from 0.9.5 and open About: "Replaced 0.9.5", in three parts. *Seen in a window* with a record written by hand beside the program (`"PreviousVersion": "0.9.5.0"`), from the title bar's button, from About of the tray menu and from About of the right-click menu (in a build with this round's correction: 0.9.8 opens no such menu on the Controls page). Not seen: the record of a real update.
- Windows, where the Desktop is on a share: make the share read-only and uninstall from Settings > Apps: the window names the shortcut that was left, and Any2Odyssey is gone from Installed apps.

New in 0.9.5:

- Windows, the G90XF PC, the ordinary case: run `Any2Odyssey-0.9.5-windows-x64-setup.exe` (not as administrator), choose Full converter. The page says "Checking this PC before the installation", then the elevation prompt comes without a question, and the installation completes as an upgrade; 3D conversion works. This is the one path the tests could not run to its end.
- Windows, the same PC, as an administrator account that is not the one that installed the worker (or with `UninstallWorker.owner` naming another account): the setup program, Full converter. A box shows the note about the other account's worker with **Continue with the installation?**, and the elevation prompt comes only after Yes. With No the page says "The installation was not started. Nothing was changed." and nothing has changed.
- Windows: during a full installation through the setup program, end the hidden `powershell.exe` that is not elevated in Task Manager: the page says that the installation may be incomplete; running the setup program again completes it.
- Windows, with the controls hidden in the tray: restore the window with something other than Any2Odyssey (a window-layout tool such as PowerToys or PersistentWindows, or `Win+M` followed by `Win+Shift+M` if that reaches it). Either nothing happens or the controls come back drawn, with their taskbar button, and Close works. *Seen in a window,* restored from another process in three ways (`ShowWindowAsync` with `SW_RESTORE` and with `SW_SHOWNORMAL`, a posted `SC_RESTORE`): the controls are drawn, no longer a tool window (the style that takes the taskbar button away), answer a click, and Close sends them to the tray again. Not seen: the taskbar itself and a window-layout tool.
- Windows: minimize to the tray and double-click the tray icon at once, several times: the controls come back every time and answer. *Seen in a window* for *Show controls* of the tray menu straight after *Minimize to tray*, forty times without a pause, and for `--show`. Not seen: the icon and a double click on it (the desktop the window ran on has no notification area).
- Windows, two monitors with different scaling: drag the controls from one to the other and back a few dozen times and watch the handle and GDI counts in Task Manager: they do not grow. *Seen in a window* for sixty changes between 100 and 150 % by the message: 560 handles before and 553 after, 61 GDI objects throughout. Not seen: two monitors and a drag by hand.
- Windows: extract the ZIP to `Downloads\Any2Odyssey [test]` and run `Install-GUI.cmd` there (it installs, as before); rename the folder to `Any2Odyssey [test` and run it again (it says to move the package and installs nothing).
- Windows: `Any2Odyssey.exe --export-guide` into a folder of about 250 characters with a name that ends in a dot: Explorer shows the file without the dot and opens it.
- Linux, Ubuntu 26.04 with the monitor and the Windows drive mounted: `./install.sh` ends with "setup: done" (it ended with "'wineserver' is needed" in 0.9.4), the lens helper works, and switching the output off and on again restarts Leia's service.
- Linux, Debian 13 or Ubuntu 25.10 on a real desktop: 2D to 3D with the distribution's `libonnxruntime-dev` 1.21 converts a picture.
- Linux, an NVIDIA GPU with the distribution's ONNX Runtime and CUDA packages (Arch: `onnxruntime-cuda` and `cudnn`): depth takes milliseconds, as before.
- Linux, a system with a running udev other than Arch (Fedora, Ubuntu): the udev step installs the rules, `/dev/odyssey-lens` appears for the user at the seat, and a second run says "already installed".
- Linux, openSUSE Tumbleweed or AlmaLinux 10 on a real desktop: `./install.sh`, then the viewer from the application menu.
- Linux: in two terminals, `./install.sh --yes --no-odyssey` at the same moment over an installation: the second prints "another installation or removal is at work in ...; waiting for it to finish", both end with Done, and `~/.local/share/any2odyssey/wine-lens-setup` (or `wine-lens`) has `setup.sh` and no folder `wine-lens` inside.

New in 0.9.4:

- Windows, the G90XF PC with an older Any2Odyssey installed: run `Any2Odyssey-0.9.4-windows-x64-setup.exe` (not as administrator), choose Full converter. The elevation prompt comes to the front and names Windows PowerShell; after it the wizard finishes, Installed apps has one Any2Odyssey entry at 0.9.4, the About page shows an upgrade, and 3D conversion works. Settings and logs are as before.
- Windows: run the setup program again and decline the elevation prompt: the Preparing to Install page says "Elevation was declined. Nothing was installed."; Back, then Install again, and approve: it completes as a repair.
- Windows, a standard account: the setup program with Full converter, answering the prompt with an administrator's name and password, is refused with the sentence about a different Windows account and installs nothing. On a PC without a full installation, Desktop controls only then installs for that account.
- Windows: a copy of the setup program downloaded with a browser: SmartScreen's "Windows protected your PC" has More info and Run anyway, as the README says. Note what it shows.
- Windows, with 0.9.4 installed: run the 0.9.4 ZIP's `Install.cmd` (a repair), then uninstall from Settings > Apps: one entry, and everything is removed. Install again with the setup program.
- Windows, once a newer release exists: the older release's setup program over the newer installation asks whether to install the older version; No changes nothing, Yes installs it.
- Windows: set Read-only on the desktop shortcut, then uninstall from Settings > Apps: the shortcut is gone.
- Windows: with `ANY2ODYSSEY_DATA_DIR` set to a folder whose path has 245 characters, start the controls, change a setting and Apply: no error, and `settings.json` in that folder has the change. *Seen in a window.*
- Linux, Ubuntu 24.04 (a real installation, not a container): `./install.sh` from the archive installs the packages the release has and names `libonnxruntime-dev` apart; the program starts in an X11 session and in a Wayland one.
- Linux, Ubuntu 24.04 with the Windows drive mounted: the Odyssey 3D step builds (it stopped at `wtsshim.c` up to 0.9.3) and ends with "setup: done"; the lens helper works.
- Linux, Fedora: `setup.sh` finds Wine's libraries and completes; `LANG=fr_FR.UTF-8 x86_64-w64-mingw32-objdump -p <wine>/cfgmgr32.dll | grep -c 'Ordinal/Name Pointer'` shows whether that objdump is translated (0 means it is, and 0.9.3 was affected there).
- Linux: with `LC_ALL=xx_XX.UTF-8` (a locale the system does not have), start the Odyssey 3D output: no "eye tracking could not be started" when it did start. Stop the tracker's camera, start again: the note appears and the picture stays.
- Linux: interrupt `any2odyssey --install-depth-model` with Ctrl+C twice, then let it finish: the model folder holds the model and no `.depth-download-*` file.
- Linux, KDE Plasma with kdialog: Open, pick a file: it opens at once. Open, then close the viewer while the dialog is up: no kdialog process is left.
- Linux: start two `./install.sh --no-deps --no-udev --no-model --no-odyssey` at the same moment: afterwards `any2odyssey --help` works.

New in 0.9.3:

- Linux viewer on KDE Plasma (Wayland), anaglyph or side-by-side output: start the output, make it full screen on the display the controls are on (they are now covered), then press F11 and Esc: it leaves full screen and closes. Again with the controls minimized. Up to 0.9.2 the output window stopped answering while the controls could not be seen (checked under sway only).
- Linux viewer on KDE Plasma (Wayland): with the controls behind another window or minimized, run `any2odyssey --show`: the task bar entry asks for attention (a Wayland window cannot raise itself). Moving the pointer over the controls for a while does not keep a processor core busy (`top`).
- Linux viewer on KDE Plasma (Wayland): the task bar and the window switcher show the Any2Odyssey icon for the controls and the output window, grouped; drop an image file from Dolphin on the controls.
- Linux, an installation of 0.9.0 or 0.9.1 made the usual way whose Odyssey 3D setup had run (also one since updated to 0.9.2): install 0.9.3 with `./install.sh --yes`. It ends with "setup: done", the lens helper works, `~/.local/share/any2odyssey/wine-lens/` no longer holds `setup.sh`, `src` or `tests`, and the settings and the depth model are still there.
- Linux: with the Odyssey 3D prepared, run `XDG_DATA_HOME=/tmp/elsewhere ./install.sh --no-odyssey`: `~/.local/share/any2odyssey/wine-lens/prefix` is still there.
- Arch package with an installation from the release archive still in `~/.local`: `any2odyssey-setup` names it and prints the command that removes it; after that command the menu starts `/usr/bin/any2odyssey`, and the settings and the depth model are still there.
- Linux: `any2odyssey --toggle` prints that it switches the Windows conversion and exits 1; a first start and `any2odyssey --quit` leave no `settings.json` in a new data folder.
- Windows: from a console started as administrator, run `installer/tests/test-uninstall-helper-elevated.ps1` (it leaves itself out otherwise): it passes.
- Windows: set the read-only attribute on the files in `C:\Program Files\Any2Odyssey`, then uninstall from Settings > Apps: everything is removed and the entry is gone.

New in 0.9.2:

- Linux, the usual installation (no `--prefix`): `./install.sh --yes` with the monitor awake and the Windows drive mounted ends with "setup: done", and `~/.local/share/any2odyssey/any2odyssey-lens-helper` works. Run `./install.sh --no-odyssey` again: the lens still works and the Wine prefix is still there. `./install.sh --uninstall`: `settings.json` and the depth model are still in the data folder.
- Linux: `sudo ./install.sh` (as root with your own home folder) stops with "not installing to ... as root" and writes nothing.
- Linux: after `install.sh --uninstall`, `grep any2odyssey ~/.local/share/applications/mimeinfo.cache` prints nothing; the installed `README.md` shows its screenshots in a Markdown viewer.
- Arch package: `/usr/share/any2odyssey/install.sh --uninstall`, as yourself and with sudo, prints "remove it with the package manager" and leaves `/usr` alone.
- Linux, mirrored outputs with the Odyssey: `start-stack.sh` runs, `edid.pairs` names the Odyssey's connector for the slot and `odyssey.place` is filled.
- Linux, live screen through the weaver: pictures per second and depth rate are what 0.9.1 gave (large allocations are now mapped one by one). Quit with the output on: the lens goes off, the process is gone within about 3 s and an immediate start works.
- Linux viewer, real zenity and kdialog: open a dialog, press Open... again; the old window goes and a new one appears.
- Windows: kill the controls while converting, start them again with automatic conversion, press Apply within 8 s of "Trying once more": the line counts down and 3D comes on.
- Windows: with `companion.log` over 4 MiB, start Any2Odyssey: `companion.old.log` appears and both processes keep logging. *Seen in a window* for the controls; the worker's part is still to do.

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
8. Quit then immediate relaunch, `--toggle` / `--show` / `--quit`, an installer upgrade over a running instance: no "Control events unavailable" in the log. Run `Install.cmd` the moment Any2Odyssey has been started: it closes the app instead of stopping with "Could not request a clean shutdown". *Seen in a window* for twelve rounds of `--quit` and a start straight after it, a second launch, `--show`, `--toggle` and two starts at the same moment: one instance each time and no such line. Not seen: the installer, and a toggle that starts a conversion.
9. View logs and the release-page button open Explorer and the browser; View logs still opens the data folder with an empty `Any2Odyssey.cmd` placed in `%LOCALAPPDATA%`.

The workflows run only on GitHub. The pull request that brings these changes is the first run of the pinned Arch container's key set-up and of the dependency job; the first tag after it is the first run of the attestation job and of the Linux gates and the dependency job in `release.yml`. Both need watching.

## UI sweep (no display needed)

`crates/app/src/ui/layout_tests.rs` drives the real frame (`App::update`) through `egui::Context::run` without a window and reads back what the frame painted and its accessibility tree, the one a screen reader is given through AccessKit (UI Automation on Windows, AT-SPI on Linux). The `sweep` tests in it go over every page in every state that changes what is on it (the controls with each long status, both notices and a long window title; the viewer with each output and input format and with a path, a status and a lens complaint several windows wide; every Help topic; About with an update, a failed check and a deep installation folder) at 42 sizes: the default window, the smallest one (700x360) and the one that was the smallest up to 0.9.6 (700x580) at display scales 1 to 3, the window maximized on 1280x720, on 1920x1080 at 100, 125 and 150 % and on 4K at 100, 150 and 200 %, very wide, very tall, both sides of the two-column limit at both heights, the default window zoomed from 0.5 to 1.7 and the smallest at 1.2, and two windows lower than the viewport allows (700x300 and 700x240, which a program that sizes windows can make). They fail when text or a control is cut by the window's edge, when text is seen beyond the window's upper or lower edge (a page longer than the window scrolls, and what scrolls ends inside the frame), when the wheel does not bring the last line of a page into the smallest window, when two controls share a place, when a button or a checkbox reacts beyond what is drawn of it, when a button, checkbox, slider, drop-down box, text or number field has no name for assistive software, when Tab does not go through the title bar from left to right or leaves the focused control outside the window, when the zoom (Ctrl+Plus, Ctrl+Minus, Ctrl+0) goes beyond what the window's width can hold, when a right click on a page opens no menu, and when the window is not told its smallest size again after the display's scale has changed. `crates/core/src/theme.rs` measures the contrast of every pair of text and background colours of both themes against WCAG's 4.5:1.

Letters the built-in typefaces lack (a window title or a file name in Chinese, Japanese, Korean, Hebrew or Arabic) are drawn with a typeface of the system, read when such a letter is first shown (`crates/app/src/ui/system_fonts.rs`). Its tests give a frame whose only typeface has no Greek a file to find, with files that are missing, empty, cut short or without the letter in front of it, and check that nothing is looked for twice, that a sign alone sends for no file and that no more than eight files are ever added. `source_titles_in_other_scripts_are_drawn_with_typefaces_of_the_system` in `layout_tests.rs` asks the system itself (the Fonts folder of Windows, `fc-match` on Linux) for five such titles in the open source list; on a system without typefaces for them (a container) it asks for nothing. The sweep fails when one of the interface's own signs is not in the built-in typefaces.

What these cannot see is the picture itself (colours as painted, the ornaments, how the letters of a system typeface sit beside the built-in ones) and whether the tree reaches the system: for those, run the program with a screen reader or an inspector (Accessibility Insights or `inspect.exe` on Windows, Accerciser or `python-atspi` on Linux) and look at a list while it is open, at a selected entry, in both themes.

## UI previews

`Any2Odyssey --screenshot FILE [--dark] [--help-page]` opens the controls with default preferences in an isolated preview (no single-instance guard, no worker, no preference loading or legacy import, no preference saves, no window enumeration, hotkeys not registered), captures the window after a few frames and exits 0, or exits 1 when the image was not delivered or could not be saved. The PNGs under `docs/assets/` were produced this way on Linux; they show the desktop controls, not optical 3D.
