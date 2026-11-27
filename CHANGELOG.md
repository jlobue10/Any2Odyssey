# Changelog

Any2Odyssey follows [Semantic Versioning](https://semver.org/). Versions continue from Any2Odd 0.7.3, whose own changelog ships with its source archive on [itch.io](https://zhandragon.itch.io/any2odd).

## Unreleased

Global shortcuts for the Linux viewer, with the keys of the Odyssey 3D Hub as [odysseyvk-linux](https://github.com/the-cave-dweller/Odyssey3d-linux) (the-cave-dweller, MIT) uses them. **Not run on the monitor's own desktop, and no key has been pressed yet**: they were run in a headless KDE Plasma 6.8.2 session (xdg-desktop-portal 1.22.1), each shortcut set off through KDE's shortcut service. `docs/TESTING.md` has what is left to see.

### Added

- **Linux: global shortcuts.** **Global shortcuts** on the viewer's page (off until chosen; `GlobalShortcuts` in `viewer-settings.json`) asks the desktop's `GlobalShortcuts` portal for five shortcuts that work while another window has the keyboard: start or stop the output, less and more depth, less and more pop-out (five hundredths a press). The keys asked for are the Odyssey 3D Hub's, as in odysseyvk: Ctrl+Shift+1, 3 and 4, 5 and 6. The desktop decides: it may ask you to confirm them, and they are changed in its own shortcut settings (KDE Plasma 6: System Settings → Shortcuts). A desktop whose portal has none, or a declined dialog, clears the box and says why on the status line.
  - The desktop gives shortcuts only to a program it can name, and it names by itself only one started from its menu: started from a terminal, a script or another program, Any2Odyssey was refused by Plasma 6.8 ("An app id is required"). It now tells the portal who it is (`any2odyssey`, the name of its desktop file, through the portal's registry of programs outside a sandbox) on a connection of the shortcuts' own, since only the first call on a connection can. This needs `any2odyssey.desktop` installed, which `install.sh` and the package do; without it the status line says so.
  - Clearing the box closes the desktop's session, and so does every other end of the shortcuts. As first written the session stayed open for as long as the program ran: Plasma kept the five key combinations from every other program, and after the box was ticked again there were two sessions and every press counted twice (depth moved by ten hundredths).
  - A portal that leaves the bus (an update of its package, a crash) clears the box with a line; ticking it asks the new one.
  - Seen in the headless session, started as from a terminal and as from the menu: the desktop's dialog with the five shortcuts once and not at a later tick, the keys asked for bound, presses taken with the window minimized (depth, pop-out, and the output window opened and closed), the session closed and the shortcuts inactive in KDE's service after the box is cleared, one press one step after it is ticked again, none left after the program quits.
  - **The keys are asked for as the desktop takes them.** Asked for as `CTRL+SHIFT+4`, they were bound by Plasma 6.8 as Ctrl+Shift+4 and no press did anything: with Shift held the key types `$`, and Plasma looks for Ctrl+$. On the monitor's own PC, with presses made through uinput: bound as Ctrl+$ the press was taken, as Ctrl+Shift+4 or Ctrl+Shift+$ it was not. They are now asked for as `CTRL+exclam`, `numbersign`, `dollar`, `percent` and `asciicircum`, the signs of a US keyboard; on another layout they are set in the desktop's shortcut settings. **Not seen: what Plasma's portal makes of these names** (the PC's bindings were set right in Plasma itself, not through a new request). A desktop that already has the five shortcuts from an earlier build keeps its bindings: set them in System Settings → Shortcuts → Any2Odyssey.
  - **What a shortcut did is said on the screen**, since the page is behind another window when one is pressed: "Depth 55", "Pop-out 10", "3D output on" or "off" (or why it did not start). While the Odyssey 3D output shows a picture, the words for depth and pop-out are drawn into that picture for a second and a half, a dark plate with light letters low in its middle, the same in both views and so at the depth of the screen. (First they went to the desktop's own display only. On the monitor's own PC, Plasma put that on another screen than the Odyssey: measured there, and the person at the seat saw nothing.) Without that output, and for the output's own start and end, the desktop shows them: KDE Plasma on its display for such things (where the volume shows when its keys are pressed), another desktop as a notification that goes after a moment and is not kept. **Show what a shortcut did** under the box switches it off (`ShortcutFeedback` in `viewer-settings.json`; on unless switched off). The words in the picture have not been seen on the monitor yet: `docs/TESTING.md`.
  - After a press the page is drawn again for three seconds, so that its sliders show the new depth and pop-out when the window was covered at the press.
  - Known, not fixed: when only KDE's own part of the portal (`xdg-desktop-portal-kde`) is restarted, nothing tells the program; the box stays ticked and the shortcuts do nothing until it is cleared and ticked again.

## 1.0.7 (2026-11-22)

An eleventh audit release: seven pull requests (#196 to #202) from a round that put 1.0.4, 1.0.5 and 1.0.6 on the rigs. Those three were made in one day on the monitor's own Linux PC, from measurements of one film on one screen, and their notes say what they had not seen: another refresh rate, another desktop, a game, a video of 60 pictures a second. This round ran those: a producer of frames at any rate through the program's own capture and render loop, the Wine-side program under real Wine 11.22 and 11.6 with a stand-in for Leia's weaver, TensorRT on an RTX 5090 (in WSL), the set-up script of the depth runtime in root trees of nine distributions, and the published files of the three releases. Linux only, but for the version: nothing Windows runs has changed. **None of it has been seen on the monitor**; `docs/TESTING.md`, "New in 1.0.7", has the checks.

After updating, run `any2odyssey-setup` once (or `install.sh` from the archive), with no 3D picture showing: the program that shows the picture through Leia's weaver and one script of the Odyssey 3D set-up changed, and an update of the program does not replace them in the data folder.

**Linux, live screen: a screen that changes 60 times a second is taken as it comes again.** Since 1.0.4 the depth thread wakes the render loop when a map is done. That turn took no frame but was counted as a picture, so the next frame waited, and the loop then ran on a grid of 15 ms of its own beside the frames. A video or a game of 60 pictures a second was shown later and less evenly than 1.0.3 showed it, some frames twice and some never: at 1080 lines with a map of 3 ms, 16.4 ms after the frame came where 1.0.3 took 5.9, and 116 of 1042 frames twice; of 60 a second on a 120 Hz screen whose frames come a refresh early and late by turns (as a browser's do), 232 of 1040 frames were in no picture at all. Now a turn that takes no frame leaves the pace alone, and no second picture is begun between the frames of a screen whose last three frames came less than 25 ms apart in the mean: every frame is shown once, 4.4 ms after it came in that last case (1.0.3: 6.3), and a game of 40 or 45 a second 5 to 6 ms after where it was 12 to 13. A film is shown as evenly as 1.0.4 to 1.0.6 showed it. Measured with a stand-in for the depth model that takes a chosen time for a map. (#202)

**Linux, the Odyssey 3D output: no picture is left until it is never shown.** 1.0.6's rule for a film's pictures left a picture that turned up early for up to three refreshes, whatever the rate. Where pictures are three or four refreshes apart, the next one arrives within those three and takes the place of the one that was left. Under real Wine, with the rule: 72 of 2000 pictures of a video of 50 a second on a 144 Hz display were never handed on, 5 of 1238 of a game that wanders around 30 a second at 120 Hz; without the rule, none. A picture is now left for one refresh where the pictures are three apart, for two where they are four, and for three as before from five on, so a film of 24 a second at 120 Hz is shown at the very same refreshes as in 1.0.6 (compared turn by turn over ten minutes of eleven rates). After it: 2000 of 2000, 1238 of 1238. (#199)

**Linux, the depth runtime (TensorRT): a library that is cut short no longer ends the program.** A library file that a copy or a download left incomplete ends whatever opens it with "Bus error". #197 keeps the program from opening the two TensorRT libraries it opens itself; run on the real libraries, the program still ended when TensorRT then opened the others by itself (`libnvinfer_plugin`, the builder's resource: at every start of a live screen), and the same with cuDNN or one of CUDA's libraries cut short (at the first depth map). All of them are now looked at first: depth goes on without the part that is damaged (through the CUDA provider, or on the processor), and the line under *Depth detail* ends with "It is cut short:" and the file. A folder named in `ANY2ODYSSEY_TENSORRT_DIR` that holds a cut library is named there, not silently passed over for the installed one. (#197, #201)

### Fixed

Linux, the live screen:

- **A screen of 60 pictures a second**, as above. (#202)
- **`A2O_LIVE_TIMES` naming a pipe no longer holds up the picture.** With a pipe nobody reads, no picture was made at all and the program could not end the live screen; with a slow reader, 22 pictures a second of 60. The lines are now written by a thread of their own, and where it cannot keep up, lines are left out and counted in a line `x <n>`. (#202)
- **A second live screen on an output that was left on no longer keeps pixels of the first.** After a live screen, another input and a live screen again with the output left on, the new render loop took what the old one had drawn in the picture file for its own: with a screen of another shape, two thirds of the pixels of each picture still had the first screen's colour. (1.0.4, where the picture began to be made in that file.) (#199)

Linux, a live screen's depth with TensorRT (1.0.5, 1.0.6):

- **Cut-short libraries**, as above. (#197, #201)
- **Stopping the output while TensorRT makes its form of the model and starting it again no longer makes two at once.** Two were made for the same size into the same file, 31 and 26 seconds; the second start now waits for the first and opens what it made. (#201)
- **Leaving the program while TensorRT makes its form of the model no longer ends in "Segmentation fault".** TensorRT's thread does not survive what a program runs as it leaves; while one is at work the program now leaves at once. Shown with the tests' own program, which leaves as the real one does, not with the window's Quit. (#201)
- **A kept form of the model is no longer deleted when TensorRT cannot be opened at all** (a library missing or damaged, another TensorRT in the named folder). It is put aside and put back unless making a new one works. (#201)
- **The card's memory that the CUDA provider held is given back when TensorRT takes over**: at *Finest* the program held 5.5 GB of the card and holds 1.3; at *Finer* 3.2 and 1.2. (#201)
- **The program's own memory no longer grows with every stop and start of a live screen.** 1.0 GB after one stop, 4.7 after ten, 10.6 after a hundred, with nothing loaded: memory that the C library kept for threads that were gone. It is given back when the model is let go: 1.9 GB at the most over 60 stops. The card's memory was given back every time before, too. (#201)
- **TensorRT's libraries are tried from the installed runtime when the folder named in `ANY2ODYSSEY_TENSORRT_DIR` has none that can be opened.** (#197)
- **A video of 30 pictures a second is taken for one when a frame comes a refresh early.** A film's picture waits for its own depth map where the frames come 25 to 70 ms apart; at 120 Hz a frame of such a video that comes a refresh early comes exactly 25.0 ms after the one before and fell out by a hair half of the time: a fifth of the frames were not waited for, and some drawn twice. The lower bound is 22 ms now: 297 of 300 drawn once with their own map. A proposal of the round, taken as proposed. (#201)

Linux, the Odyssey 3D output and set-up (run `any2odyssey-setup` once to have these):

- **Pictures left and never shown**, as above. (#199)
- **`A2O_WEAVER_LATENCY_US` and `A2O_WEAVER_LATENCY_FRAMES` tell Leia's weaver only a whole number that was asked for.** Set but empty, or `abc`, told it 0 microseconds; `1e5` told it 1, `25ms` 25, and `A2O_WEAVER_LATENCY_FRAMES=` 0 frames. Anything but a whole number in range (to 200000 microseconds, to 16 frames) now counts as not set, and the first line of `A2O_WEAVER_STATS` says "(not told)" after it. (#199)
- **The removal of Leia's old log files looks below Leia's folder only.** The pattern was matched against the whole path: with a folder named `Log` anywhere above the Wine prefix, every `.txt` file of more than two weeks in Leia's folder was a log to it. (#199)

Linux, `depth-runtime/setup.sh`:

- **The CUDA libraries are looked for as a program finds them** (#196): the script asked `ldconfig -p`, which a user on Debian does not have on the PATH, took a line that only contained the name, and took a file in `/opt/cuda/lib64` that no program would find there. It now opens the libraries the way the program does, also from `LD_LIBRARY_PATH`, and asks for `libcurand.so.10` as well, which the CUDA provider needs. A library that is cut short, a `python3` without `ctypes` and a Python older than 3.8 are said in one line each. (#196, #200)
- **Given a folder that already holds the two wheels, it downloads nothing** and needs neither pip, a venv nor the network. (#200)
- **Room is asked for before anything is fetched** (8 GB in the data folder's file system, 7 with a folder of wheels), and pip's temporary files go to the data folder, not to `/tmp`, which is memory on most desktops and smaller than the larger wheel on some. (#200)
- **An upgrade works where the file system cannot exchange two folders in one step** (two renames, and it says so), **the hidden staging folder is removed after `PIPE` and `QUIT` too** and by the next run after a kill, a relative `XDG_DATA_HOME` counts as unset as it does for the program, a `depth-runtime` that is a link or a file is said before the download, and root is refused in another user's data folder. (#200)
- `install.sh --uninstall` says that the runtime stays (2.8 GB) and how to remove it; the README names the script. (#200)

### What held

- **1.0.4's films**: 23.976 to 50 pictures a second on screens of 60, 120 and 144 Hz with a third of the frames early or late: none less even than in 1.0.3, most far more (24 a second at 120 Hz: a spread of 1.1 ms from picture to picture where 1.0.3 had 9.0). A frame that is the same as the one before is told right in 60,000 random pairs of every format, row length and turn; the halving for the depth model equals a reference in whole numbers in every pixel of 64 sizes on 1 to 32 cores.
- **TensorRT on an RTX 5090 (in WSL)**: `depth-runtime/setup.sh` ran at the first try with CUDA 13 and cuDNN 9 from NVIDIA's wheels in `LD_LIBRARY_PATH`. First form of the model 36 to 52 seconds, 54 MB each, opened again in 0.5 to 1.0 s; a map 2.7 ms for the CUDA provider's 6.8 at *Standard*, 9.3 for 31.2 at *Finest*. *Depth detail* changed six times while one was made: two made in all, never two at once. 200 stops and starts: the card's memory the same after each.
- **1.0.6's wait of a picture for its own map**: at 23.976, 24 and 25 pictures a second 96 to 98 % of the frames are drawn once with their own map at *Standard* and *Fine*; none left out, none later than the next frame; a map that takes three or ten times as long, a model that fails, a pause: nothing lost, even again within seconds.
- **The picture made in the file the output reads** (1.0.4), read by the real host under Wine at 1080, 1440 and 2160 lines as fast as it can be written: 5602 of 5603 pictures handed on whole, and the one mixture not taken for shown. The size changed, the writer ended in the middle of a picture, the sequence number past its wrap, the file removed: the host goes on or leaves as it should.
- **The published files of 1.0.4, 1.0.5 and 1.0.6**: eight each, every `.sha256`, every attestation against its tag, the contents against the tags; `depth-runtime/setup.sh` is in the archive and the Arch package from 1.0.5 on.
- **#196 against a program's own loader**: the same answer in all 16 cases (libraries in `LD_LIBRARY_PATH` only, in `/opt/cuda/lib64` only, CUDA 12 only, 32-bit ones), under sh, dash, BusyBox sh and bash.

### Corrections to the notes of 1.0.4 to 1.0.6

- 1.0.4, "a desktop, a game and a video of 60 a second are shown as they come": they were not; see above. And a source of 30 or 40 pictures a second that is not a film (a game held at that rate, a video call) is taken for one by both rules, in the program and in the Wine-side program: its pictures are evened like a film's: 7 ms later in the mean by the Wine-side program's rule, and with a quick depth model 14 ms later by the program's wait for the map. Nothing here can tell a game from a film by its timing; this is as it was made, and now said.
- 1.0.4, "55 of 165": through a desktop a screen of 144, 165 or 240 Hz arrives on a grid of 8 ms, as the same notes say two lines above, and is shown 61 times a second in steps of two and three refreshes mixed, as in 1.0.3. Screens of 90 and 100 Hz are shown 45 and 49 times a second, evenly.
- 1.0.4, "Windows is unchanged but for the version": true of every file but the program, whose code is 64 bytes shorter from 1.0.4 on (two files that changed are compiled for Windows too).
- 1.0.5, "meanwhile the maps are made as before": on the rig the CUDA provider's maps all but stand for 14 to 20 seconds while TensorRT makes its form of the model (one map every one to two and a half seconds), once for every size and detail. This may be WSL's doing; it is to be looked at on the monitor's PC.
- 1.0.5, how far TensorRT's maps differ from the CUDA provider's: over 60 pictures of films and desktops 0.0007 to 0.0011 of the range in the mean (the notes: 0.0004, from eight photographs) and 0.0025 to 0.0035 at 99 of 100 points (under 0.002). Nothing a picture shows.
- 1.0.6, `docs/TESTING.md`: not "every line" of `A2O_WEAVER_STATS` ends with `latency 25000`; every other line is the one that begins `shown for turns:`.

### Tests and releases

- The rule for a film's pictures in the Wine-side program is cut out of its source and run natively under the sanitizers (`contrib/wine-lens/tests/test-weaver-evenly.py`, in CI and in the release workflow): on 1.0.6's rule ten of its cases fail.
- New tests for: the picture drawn in its place through every change of lines, screen and sharpening (no test noticed either of 1.0.4's two guards taken out); the cut-short libraries; one form of the model at a time over a stop and a start; a model's failure letting go of a picture that waits; the log removal under a folder named `Log`; the set-up script's corrections, with stand-ins for the wheels (24 of them taken out by hand, each noticed).
- 1.0.6's wait of a picture for its own map is under no test that notices it taken out; the one test that looks at it is run alone and passes without it. Known.
- The saved files of 1.0.4, 1.0.5 and 1.0.6 are in the corpus of releases.
- `url` 2.5.9 for RUSTSEC-2026-0592 (a stack overflow in a function the program does not call), which had turned the dependency check red. (#198)

### Known, not fixed

- **A film is even only while nothing else on the screen changes at every refresh.** With the pointer moving over it (where the desktop draws the pointer into the frames), its pictures are 33 and 50 ms apart by turns, as in 1.0.3.
- **sway with the wlroots portal** sends 104 to 108 frames a second of a 120 or 144 Hz screen now that 125 are asked for, and 52 to 54 pictures a second are shown where 1.0.3 showed 62 (more evenly than there). KDE Plasma and GNOME were not run in this round.
- **Where a picture waits for its own map, the holding of a film's early frame (1.0.4) does nothing**: the pictures are written as the frames come, and the evening is left to the Wine-side program.
- **What the Wine-side program leaves a picture for is never given back**: after turns that ran over, or with a source whose clock runs ahead, a film's pictures can be shown up to three refreshes after they came for as long as the film runs. A film of 24 a second on a 165 Hz display is taken for a whole number of refreshes (it is 6.875) and is 17 ms later for no gain.
- `ANY2ODYSSEY_TENSORRT_DIR` naming a TensorRT of another major version: it is loaded, the provider cannot use it, and the installed one is not tried. A named folder that is empty or missing is passed over without a word. A folder of `depth-engines` that cannot be written has its form of the model made anew at every start, without a word.
- `depth-runtime/setup.sh` was not run with a `python3` of conda's in front on the PATH (its loader may find libraries the program's does not). pip prints nothing during the 4 GB download.

### Hardware verification

None. Everything above was run on rigs: stand-ins for the desktop's portal, for Leia's weaver and, for the timings of the live screen, for the depth model; TensorRT and the CUDA provider on a real card through WSL. `docs/TESTING.md`, "New in 1.0.7", lists what is to be seen on the G90XF, first of all: `any2odyssey-setup`, then the film of 1.0.6 again (nothing should differ), then a video of 60 pictures a second.

## 1.0.6 (2026-11-21)

Linux only, the live screen on the Odyssey 3D output: movement in a film hitched now and then. Measured on the Odyssey's own screen while it showed a film of 24 pictures a second in a browser (KDE Plasma 6.8.3 at 120 Hz, 2160 lines, *Finest*, TensorRT), before and after, from outside and with `A2O_LIVE_TIMES` and `A2O_WEAVER_STATS`.

After updating, run `any2odyssey-setup` once with no 3D picture showing: the program that shows the picture through Leia's weaver changed.

### Fixed

- A film's frame is shown once, with its own depth map, where the model answers in half the time from frame to frame or less. Every frame's picture was written 12.6 ms after the frame came, with the map of the frame before, and some frames in every second were then shown a second time, 14 ms later, with their own: 1199 frames made 1366 pictures in one minute, and what moves stood at another depth from picture to picture. A frame's picture now waits for the frame's map: one and a half times what the maps have been taking and 2 ms, and never so long that the picture is not made before the next frame. Measured after: 1385 of 1400 frames waited (13.7 ms in the middle, 19.2 ms at the most) and were drawn once with their own map, 25.8 ms after they came; the other 15 followed a frame that came out of turn. A map takes 15 ms on that screen, two and a half times what the model takes alone, with the GPU also weaving. A screen that shows no film waits only for maps of 4 ms and less, and a slower model changes nothing.
- The program that shows the picture leaves a film's picture that turns up sooner than the others for its turn. Pictures were written at points of a refresh up to 6 ms apart, in stretches, and whichever point that program looks at, a late one is shown a refresh later: for four refreshes where the one before had six. It applies where the last 32 pictures came a whole number of refreshes apart in the mean, three at the least (24, 30 or 40 pictures a second at 120 Hz), and holds a picture for three refreshes at the most; a desktop, a game and 60 pictures a second are shown as they come. `A2O_WEAVER_EVEN=0` switches it off, and `A2O_WEAVER_STATS` has a second line each second: for how many refreshes the pictures were shown, and how many turns a picture was left for a later one.
- With both, counted by that program over a minute of the film: 1321 of 1453 pictures were shown for five refreshes, 54 for four and 56 for six, 22 for fewer; 36 of the 60 seconds had nothing but fives. With the second change alone and frames still shown twice it was 1219 of 1602.

- Changing *Depth detail* while TensorRT makes its form of the model for a live screen no longer has it make two at once: the one being made is finished first (dropping it could not stop TensorRT or give back the GPU's memory), the maps go on through the CUDA provider meanwhile, and the detail chosen last is made after it. A TensorRT that cannot be used is still told of and not asked again, whichever detail it was asked for.
- `depth-runtime/setup.sh` unpacks the very bytes it has checked against the fixed SHA-256 (of wheels in a folder you name it takes a copy of its own first, which needs room for the larger one, 3.7 GB, in the data folder while it runs), puts a new runtime in the old one's place in one step, so that no moment has none, and clears the kept forms of the model only once the runtime is in place.

The pictures of a film are 13 ms later than they were, all of them. `A2O_LIVE_TIMES` has two more kinds of line: what a frame's picture did about the frame's map, and why a second picture of a frame was made.

## 1.0.5 (2026-11-21)

Linux only, and only with an NVIDIA GPU: a live screen's depth maps through TensorRT. Windows is unchanged: Samsung's engine makes the depth there, with models and a runtime of its own that Any2Odyssey does not touch.

### Added

- A live screen's depth maps are made by TensorRT, in half precision, where the ONNX Runtime the program loads has the TensorRT provider. Measured on an RTX 5090 with the program's own timing test (`live::bench`), a 3840 x 2160 screen, each map with its preparation: 1.2 ms for 2.9 at *Standard*, 2.2 for 5.7 at *Fine*, 3.7 for 13.5 at *Finer* and 6.2 for 27 at *Finest*. The maps differ from the CUDA provider's by 0.0004 of the range from far to near on average, and by less than 0.002 at 99 of 100 points (eight photographs at the four details). TensorRT makes a form of the model for each size of picture, which takes it from ten seconds to most of a minute, once: meanwhile the maps are made as before, the page says under *Depth detail* that it is being made, and it is kept in the data folder (`depth-engines`, some 55 MB a size) and opened from there in a fifth of a second. One that an update of TensorRT or another GPU has made unreadable is made anew. A still image's map is made as before. `ANY2ODYSSEY_TENSORRT=0` leaves TensorRT unused; where it cannot be used the page and the log say why, and the CUDA provider goes on.
- No distribution packages an ONNX Runtime with that provider so far. `depth-runtime/setup.sh` (in the archive beside `install.sh`; in the Arch package `/usr/share/any2odyssey/depth-runtime/setup.sh`; in the source `contrib/depth-runtime`) puts Microsoft's (1.32.0) and NVIDIA's TensorRT (10.16.1) into the data folder (`depth-runtime`, 2.8 GB of a 4 GB download, each file checked against a fixed SHA-256; it needs CUDA 13 and cuDNN 9 on the system), where the program uses a runtime before the system's. A runtime named in `ORT_DYLIB_PATH` comes before both, and `ANY2ODYSSEY_TENSORRT_DIR` names the folder of TensorRT's libraries for one that does not find them.

Not run on the monitor: the numbers are of the model alone, on a machine that showed nothing. What a live screen gains (how old the depth is that a picture is drawn with) is still to measure there with `A2O_LIVE_TIMES`.

## 1.0.4 (2026-11-21)

Linux only: the live screen and the Odyssey 3D output, from measuring 1.0.3 while it showed a film on the Odyssey's own screen (KDE Plasma 6.8.3, the screen at 120 Hz, 2160 lines, *Finest*), and some diagnostics. Windows is unchanged but for the version.

After updating, run `any2odyssey-setup` once (or `install.sh` from the archive), with no 3D picture showing: the program that shows the picture through Leia's weaver and one script of the Odyssey 3D set-up changed, and an update of the program does not replace them in the data folder. They were then run and measured on that screen; what was not is said at the end.

### Changed

Linux, the live screen:

- **A picture is seen when the screen draws it, not on a grid of 1/62 s.** A desktop was asked for 62 frames a second at the most. KDE Plasma draws the Odyssey's own screen at every refresh while the 3D picture lies on it, so its cast ran at that limit all the time and the screen was seen 62 times a second, whatever it showed and when. A film of 24 pictures a second reached the output 32.1 and 48.4 ms apart (two and three of those frames; 597 and 841 times in a minute) where its pictures are 41.7 ms apart: the unevenness of a film on a 60 Hz television, which the screen itself, at 120 Hz, does not have. A video of 60 a second lost or doubled a picture about twice a second the same way. The desktop is now asked for 125 a second, more than a screen of 120 Hz draws, so a frame comes at every refresh. A screen of more than 120 Hz is still seen on a grid, of 8 ms.
- **Of a screen that changes faster than pictures are made, every second or third frame is taken, as it arrives.** Pictures are made 66 times a second at the most. With more frames than that arriving, one always lay waiting when a turn began and was taken as it was, and how long it had waited drifted: of 120 frames a second every ninth picture came one frame after the one before it and the others two. The frames that come too soon after the one taken last are now let pass and the first that does not is taken at once, so the pictures are a whole number of frames apart (60 a second of 120, 55 of 165). Reasoned and tested on a clock of the test's own, not measured with a game.
- **A frame that is the same as the one before is told before it is copied.** Every frame was copied out of the stream's buffer (3.3 ms for 3840 x 2160) and compared afterwards: under the film, 62 a second were copied for 38 to be thrown away. A frame is now compared where it lies, and copied only when it is another picture.
- **The picture is made where the output reads it.** It was made in room of the render loop's own and copied into the file the output reads: at 2160 lines 66 MB a picture, 10 to 11 ms of the 23 a turn took. `live::bench::turn_timings` on the same machine, while the film ran: a turn at 2160 lines takes 11.2 ms where it took 22.9 (89 pictures a second possible where 44 were), at 1440 lines 5.8 where 9.3, at 1080 lines 4.0 where 6.1. The program also holds 66 MB less at 2160 lines. Where the file is not in memory the picture is made and written as before.

- **A film's frames are shown evenly.** They do not arrive evenly: with a film of 24 pictures a second in a browser on the Odyssey's own screen, a frame came 38 to 46 ms after the one before 56 to 74 % of the time, and otherwise a refresh early and a refresh late by turns. Where the last eight frames came in a steady run of 15 to 50 a second, a frame that comes early is now held until its time, 9 ms at the most. Measured in the running program over a minute of that film (`A2O_LIVE_TIMES`): 74 % of the frames arrived 38 to 46 ms apart and 88 % were shown so. A desktop, a game and a video of 60 a second are shown as they come.
- **The depth model sees a frame 22 ms sooner at *Finest*, and is given its next frame as soon as a map is done.** Before the model saw a frame of a large screen, the frame was halved on one processor core through numbers with a fraction: 23.8 ms at *Finest* on a 3840 x 2160 screen (12.3 at *Finer*, 6.1 at *Fine*, 2.5 at *Standard*). The same sums are now made in whole numbers on a few cores: 1.9, 1.3, 0.8 and 0.4 ms, the result the same colour or its neighbour. And a map that was done just after a film's frame had come left the model idle until the next frame: it made 14 maps a second for the film's 24 pictures and makes 23.5. A picture is drawn with the depth of the frame two before it, steadily, where it was one to three; with a finer look at where the 40 ms of a map at *Finest* go, more may be had. Where no frame of a film is about to come (a still screen, a desktop), a map is also drawn with at once and not at the screen's next change or 150 ms later.

Linux, the Odyssey 3D set-up (run `any2odyssey-setup` once after updating; it ends the stack, so not while a picture shows):

- **A start of Leia's stack removes the log files Leia's programs left more than two weeks ago.** They begin a file at every run and remove none; the eye tracker writes about 10 MB for every hour it follows a face.

Linux, the Odyssey 3D output:

- **Leia's weaver is told how long a picture takes to be seen: 25 ms.** The weaver places the two views for where the eyes will be when the picture it weaves is on the screen, and reckons with a time of its own choosing unless it is told one: 40 ms on the Odyssey at 120 Hz under Wine. Looked at on that screen while the head moved from side to side, with 60, 40, 32, 25 and 17 ms told to it in turn: the differences show, 25 ms was the best ("still not perfect"), 17 ms "maybe too fast". This is one screen's number, found by eye, on KDE Plasma at 120 Hz; `A2O_WEAVER_LATENCY_US=<microseconds>` in the environment of Any2Odyssey tells the weaver another.

### Diagnostics

- `A2O_WEAVER_STATS` now begins with the time Leia's weaver reckons with (`latency ... microseconds`) and says it again in each second's line, which also has the mean time of an upload, the shortest turn, and how many turns were half as long again or more (`late turns`; not to be trusted in a second whose shortest turn is far under a refresh). Measured at 2160 lines with a film: 120 weaves a second, an upload of 66 MB takes 4.5 ms in the mean and a turn with one 10 to 14 ms at the longest, so an upload on the weaving thread costs a refresh now and then and no more. `A2O_WEAVER_LATENCY_FRAMES=<n>` tells the weaver n frames instead of a time; right after being told 3 it went on saying 40000 microseconds.

- `A2O_LIVE_TIMES=<file>` in the environment of Any2Odyssey writes a line for every picture and every depth map of a live screen: when the frame arrived, was taken and was in the output's hands, which frame its depth came from, and how long each map took.

### Hardware verification

Seen and measured on the G90XF on 2026-11-21 (KDE Plasma 6.8.3, 120 Hz, a film in a browser on the Odyssey's own screen, 2160 lines, *Finest*): the numbers above, and "looking much better" after the depth model's input was made faster. Not seen: anything at another refresh rate, on another desktop, with a game or a video of 60 a second. `docs/TESTING.md`, "New in 1.0.4", has the checks.

## 1.0.3 (2026-11-21)

One change: the pull request that 1.0.2 left open (#184). On Linux, a film's black bars no longer take part of the depth of a live screen. It comes from the same round as 1.0.2 (audit round 24) and was kept out of that release because it "wants eyes on the monitor first". It is released here as it was proposed, so that it can be looked at on the monitor: **nothing of it has been seen there**, it changes what is shown for every film with black bars, and it rests on a few rules of thumb, which are given below. 1.0.2 is the release without it; nothing else differs between the two but the version. Windows is unchanged.

Coming from 1.0.2, nothing is to do after updating. Coming from 1.0.1 or older, run `any2odyssey-setup` once, as the notes of 1.0.2 say.

**Linux, live screen: a film's black bars lie at the far end of the depth.** The depth model does not know a black bar for nothing. Over 100 seconds of a film of 1280 × 534 on a screen of 1280 × 720 it has the bars at three quarters of the way from far to near in the mean, as a frame in front of the picture, and the nearest point of the whole map lies in a bar in 480 of 600 maps. The range from far to near is the map's, so the film between the bars is left three quarters of the depth in the middle, two fifths or less in every tenth map, and 27 % at the least: a wide shot of a town lies flat behind two bars that float in front. This is the same in every release up to 1.0.2. The depth thread now looks at the frame it gives the model for rows and columns that are black from end to end at its edges, and puts those rows and columns of the model's answer, and one more beside each, where the farthest point of the rest is. Both loops as the program runs them, fed 20 seconds of the film in real time with the model on a graphics card: the bars were shown at 0.68 of the way from far to near and are at 0.06; the film had 44 % of the range in the middle and has 97 %. How steady the picture is then lies between the same film with its bars as the model said them and the same film cropped to fill the screen (change of the whole picture from map to map: 4.9, 5.9 and 6.1 thousandths of the range). A picture without bars is told by its first row and its first column. (#184)

### Changed

Linux, the live screen:

- **Rows and columns that are black from end to end at the edges of the screen are put at the far end of the depth**, as above. The rules of thumb, so that they can be judged: a pixel is black at 24 of 255 or less; a bar is a hundredth of the side or more; the bars together are three quarters of the side at the most (a picture that is black all over has none and is treated as before). (#184)
- **A bar with something in it is a bar as high as the other.** A film's two bars are of one height. Where one of them reaches further and the same rows at the other edge are black in four pixels of five, that one is a bar as high, with what stands in it: subtitles, a player's controls, the pointer. Without this rule a bar ended where a subtitle began and the rest of it was the picture's again: with two lines of subtitles in the lower bar for two seconds in every four, the film had 98 % of the range while no subtitle stood and 81 % while one did, and its depth pumped with them (its share jumped by more than a tenth of the range 12 times in 463 maps). With the rule: 98 % and 99 %, no jump. What stands in a bar is then as far back as the bar, the subtitles too; whether that reads well needs eyes. (#184)

### Not covered

- Bars that are not black.
- Something in both bars at once: both are then as high as the one that is black from end to end for longer.
- Something in a bar at the side of an upright video: the bars at the sides are not taken to be of one width.
- A title on black: more than three quarters dark, taken for a dark picture, as before.
- A film with bars in a window that does not fill the screen: its bars are not at the screen's edges, and it is shown as before.

### Tests

- New tests, run with the rest: the black bars of a frame found, with the pointer in one and subtitles in the other, and a dark picture, a border and a picture black all over not taken for bars; the depth thread with a model that has the bars nearest; the map's edges put far. Each of eight parts of the change, taken out again by hand, fails one of them.
- The numbers above are from both loops of a live screen as the program runs them, with the capture fed by hand and the depth model on a graphics card, over 20 seconds of Tears of Steel (the Blender Foundation's, not in the repository) with and without subtitles in the lower bar. They were measured on the proposal's two commits; the three files that hold the live screen's depth are the same in this release, byte for byte.
- Every gate was run on the release's tree, on Windows and on Linux. The Windows program is 1.0.2's but for the version.

### Known, not fixed

- The items of 1.0.2 stand as they were, but for the one about a film's black bars, which this release answers.

### Hardware verification

Pending: nothing of this release was run with the monitor. `docs/TESTING.md` lists the checks under "New in 1.0.3", with 1.0.2 beside it if possible: a film with black bars above and below, full screen (the bars lie at the back, the film has more depth); the same with subtitles in the lower bar (the film's depth does not pump, and the subtitles lie at the back with the bar); a dark scene without bars (nothing of it is flattened); an upright video with black at both sides. The checks of 1.0.2 and of the releases before it are still to do as well.

## 1.0.2 (2026-11-21)

A tenth audit release: six pull requests (#179 to #183 and #185) from a round that ran what the round before it had reasoned. 1.0.1 was made by reading, on Linux alone, and its notes say so in several places. This round put it on the rigs: the depth model itself on the processor and on a graphics card over real film and recordings of a desktop, the Odyssey 3D set-up under real Wine in three versions, the live screen on KDE Plasma, GNOME, sway, labwc and COSMIC with their own portals, the Linux installer against real apt and real pacman in root trees of eight distributions, and the Windows side on a Windows PC for the first time since 0.9.9. Most of 1.0.1 held, the two corrections it had "reasoned from the code, not seen" among them. One of its claims did not hold and is taken back here. Nothing found was grave; three findings were of the middle kind, one of them new in 1.0.1 and two older. At the end all six were merged and run together, on Windows and on Linux, with every rig of the round.

After updating, run `any2odyssey-setup` once (or `install.sh` from the archive): three scripts of the Odyssey 3D set-up changed, and an update of the program does not replace them in the data folder.

**Linux, live screen: the range that 1.0.1 let follow the scene is each depth map's own again.** 1.0.1 measured each depth map against a range that follows the scene slowly, so that "the depth as a whole no longer steps when the nearest or the farthest thing in the picture changes a little". Run with the depth model over real pictures, it does the opposite on a desktop. The model's numbers move as a whole from one map to the next (on a desktop where only a counter changes, by about one hundredth of their span); measuring each map against its own lowest and highest number is rid of that, and a range that hardly moves is not. At *Depth steadiness* 60 and *Standard*, 1.0.0 against 1.0.1: the depth of the whole picture changes from map to map by 2.4 and 4.4 thousandths of the range on a desktop that stands still, by 2.8 and 3.9 while text scrolls; when a small window opens, the rest of the picture moves by 0.56 and 1.07 pixels between the two views of a 3840-wide view, and by 1.07 and 2.36 when a window is dragged in; after a hard cut in a film the new scene is nine tenths there after 3 maps and after 6 (22 at the most, more than three seconds on the processor), and is shown at as little as two thirds of its depth meanwhile. With this release every one of these numbers is 1.0.0's again. What is given up is the one case where 1.0.1 was better: where a window comes in over a film that stands still, the background steps back by 13.9 pixels at once as in 1.0.0, where 1.0.1 had 9.9 (0.9.9: 7.0). The following itself stays in the code with nothing to do. **This too has not been seen on the monitor**; `docs/TESTING.md` has the checks, side by side with 1.0.1 if possible. (#183)

**Linux, the Odyssey 3D set-up: a start of Leia's stack no longer leaves before Leia's session is there.** Under Wine 11.21 and 11.22 the process that is started as the session first is one of Wine's own, and the session appears a moment later; in between, nothing has the session's name. A start that left in that moment gave two faults, both seen under real Wine with stand-ins for Leia's programs. The older one: the first start after a login came without eye tracking ("Leia's service is not running"), in five cold starts of five. The one that is new in 1.0.1, whose longer look at Leia's log moved the moment: the lens output and the Odyssey 3D output asked for together started the stack twice, in four pairs of four, and the pair took 39 seconds where it takes 20. The start now waits up to three seconds for the session; a cold start takes a tenth of a second longer under Wine 11.22 and no longer under 11.6. Whether either fault showed on the monitor depends on when Leia's real service writes its line about the board, which the rig could not know. (#185)

**Linux, live screen: a damaged note of the remembered screen no longer keeps the live screen from starting.** The program keeps the desktop's token for the screen that was chosen, so that the next start asks nothing. With something in that file that is no token, every Start ended with "Restore token is not valid" on KDE Plasma and on GNOME; with zero bytes in it, which is what a power cut can leave, the program lost its connection to the session's D-Bus at the first Start and every later one failed, after a new launch too. Only *Choose another screen* got out of it, and nothing said so. Older than 1.0.1 (0.9.9 does the same). Such a token is now not sent, and a request that the desktop refuses as wrong while a token was given is made once more without it: the desktop's dialog comes as at a first start, and a good token is written. (#182)

### Fixed

Linux, the live screen:

- **Each depth map is measured against its own range again**, as above. (#183)
- **At a depth steadiness of 0 nothing of the range before is kept.** 1.0.1 kept half of it, against the page's own words ("How much of the previous depth map is kept"): after a cut, 15 to 21 % of the old depth was left with the first map, and up to three eighths of the picture lay at the nearest or the farthest. (With the range each map's own this line has nothing to do; it is the correction if the following range comes back.) (#183)
- **A damaged note of the remembered screen**, as above. (#182)

Linux, the Odyssey 3D set-up (run `any2odyssey-setup` once to have these):

- **A start waits for Leia's session**, as above. (#185)
- **A clock that was set back no longer makes every request a cold start.** A running stack counted as having the lens board only while the board's link in `/dev` was not newer than the note of the start. On a PC that also starts Windows, with Windows keeping local time, the clock is set back after the boot wherever local time is ahead of Greenwich, and the link stayed "newer" for hours: each request took 20 seconds where it takes a fifth of one. The note now holds the link's own time, and the two are compared for being the same. (#185)
- **A `wine` on the PATH that is a script of the user's own** ("Known, not fixed" in 1.0.1) is asked where its libraries are. With no other Wine on the machine the set-up stopped at "could not find Wine's x86_64-windows library folder"; with another one it took that one's libraries. The set-up now asks the Wine that runs, once a prefix is there, and notes the folder for the starts that follow. A `wine` that lies beside its libraries, a distribution's or WineHQ's, is asked nothing. (#185)

Linux and Windows, the program:

- **Something in the place of the log's rotation turn no longer stops the rotation for good.** 1.0.1 rotates the log in one process at a time and marks its turn with a file, `companion.rotating`. A folder or a dangling link of that name was never taken away, and the log grew without end; on Windows a folder there was not even looked at. What is old and in the way is now removed, and where it cannot be, the log is rotated without a turn. (#181)
- **Linux: a log that is a link to a file elsewhere no longer has that file made its owner's alone.** 1.0.1 makes the log 0600 "at the next line"; through a link, that changed the mode of the file the link names. A log made by the program is 0600 as before. (#181)
- **The guide's *Automatic start & recovery* says that a start which finds another renderer is made once more.** It said "never retried"; the program has made that second try since 0.9.1. (#181)

Windows:

- **The uninstaller's refusal stays on screen where the 64-bit PowerShell cannot be started.** Since 1.0.1 the uninstaller starts itself again in the 64-bit PowerShell when a 32-bit program runs it. Where that one is there and Windows does not start it (a policy, security software), the start's own error ended the script before the part that waits for a key: the window closed with nothing to read. It now says "Use 64-bit PowerShell to uninstall Any2Odyssey." with Windows' reason, and waits as before. (#179)
- **`--layout-probe` names the file it could not write**, as `--inspect` does. (#179)

### What held

Run for the first time, and found as the notes of 1.0.1 and 1.0.0 say:

- **`any2odyssey-setup` ends with "done" under real Wine** (11.22, wine-staging 11.22 and 11.6; a fresh prefix, and over a set-up made by 0.9.9 and by 1.0.0), and Wine answers the question that 1.0.1 added with yes. Each of 1.0.1's eight corrections of the set-up was seen doing what it says: a failed start is not taken for a running stack, the board's line has to be this start's, the libraries are those of the Wine that runs in eight layouts, a failed import stops the set-up with its reason and a second run repairs it, exactly the entries of the monitors that are gone are removed, and a camera bridge whose note is lost is found again and no other one is ever ended.
- **Quitting with a helper that does not leave** ("Reasoned from the code, not seen"): 90 quits through the window, nothing left behind, three seconds at the most; 1.0.0 showed the old fault in 7 of 67. **Full screen and the output window** (the same): 53 steps of eight sequences, all right; 1.0.0 fails six as the notes describe.
- **The log by several programs at once**: on Linux and on Windows, a hundred rounds of two, eight and ten programs with a log over 4 MiB lost no generation and no line; 1.0.0 lost the kept generation in 2 of 100 on Windows and in every round where a rename is slow.
- **The live screen on real desktops**, 0.9.9 against 1.0.1 on KDE Plasma, GNOME, sway and labwc: the picture is the desktop's own screenshot pixel for pixel in both, a second start restores the screen without a dialog, memory is level over ten minutes. No desktop sends rows with unused bytes after them, and none changes the fourth byte of a pixel alone, so the two corrections of the frame comparison (1.0.0, 1.0.1) are never at work there; they cost 0.5 to 0.7 ms a frame at 3840 × 2160. A 3840 × 2160 screen shown at 720 lines is the screenshot shrunk by an area average, exactly.
- **The depth model's start and stop**: 1,240 live screens on the processor and 195 on a graphics card, at gaps from nothing to three seconds, never two loads for one screen and never the model let go under one. The depth from the card is the processor's to within a twentieth of a percent of the range.
- **The pace**: 60 pictures a second in every run, with 1.0.0's every-pixel shrink costing what its notes say (0.9 to 2.0 ms on six threads; 2.6 on four, 4.8 on two, where the 99th picture of a hundred then takes longer than its 15 ms).
- **The Linux installer against real package managers** (Debian 12 and 13, Ubuntu 22.04, 24.04, 26.04 and the development branch, Arch, Fedora): every correction of 1.0.0 and 1.0.1 does what it says, in German as in English, and `install.sh` is unchanged in this release. The lock of 1.0.0: 152 runs killed at any moment never kept a later run out.
- **The published files of 1.0.0 and 1.0.1**: eight each, every checksum and every attestation right, and inside them what the tags say.
- **Windows**: the three questions about a display's colour give the same answers as in 1.0.0 and as Windows gives by three other roads; the package check passes on a PC with the full converter and a newer Any2Odyssey installed; the controls in a window of their own behave as in 0.9.9.

### Corrections to the notes of 1.0.1 and 1.0.0

- 1.0.1, "the depth as a whole no longer steps when the nearest or the farthest thing in the picture changes a little": measured, it stepped about twice as much on a desktop, and a cut took twice as long. Taken back, as above.
- 1.0.1, "about 36 MB" given back after *Finest* on a 3840 × 2160 screen: 13 to 26 MB were measured.
- 1.0.1, "Known, not fixed", the jump list on Windows: settled. Windows takes the list, and both entries were read back from it with their titles, "Turn 3D on / off" and "Open 3D controls".
- 1.0.0, "a large [change] is followed with the next map": with the depth model, at steadiness 60 a new scene is nine tenths there after 3 maps in the middle (5 at the most) where 0.9.9 took 5 (6), and at 95 after 4 (11) where 0.9.9 took 18 (49). Its price is as its notes say and now has a number: where something near comes into a picture that stands still, the background steps back by 13.9 pixels in one map where 0.9.9 had 7.0, and at steadiness 95 the edges of things that move are hardly calmer than at 60.

### Tests and releases

- **Corrections of 1.0.0 and 1.0.1 that no test noticed when they were taken out again have tests now**, or tests that can fail: the two `LC_ALL=C` in front of apt (without them a German environment gets the distribution's old Wine installed again), the installer's wait for its lock where there is no `timeout`, `install.ps1 -PackageOnly` (its test asserted nothing), *Restore overlay* kept until the output window is there, `--inspect` and `--layout-probe` on Windows, the rows of a frame taken without their gaps (#174), the depth thread's side of letting the model go, the failed import of the camera's entries, and the log made through a link. Each area took every correction in its files out by hand, about a hundred in all.
- **Every order of the supervisor's events is walked against the rule** for a start that meets another renderer (4,224 states); each of 1.0.1's two corrections there, taken out, fails within six events.
- **The saved files of 1.0.0 and 1.0.1 are in the corpus of releases** and are read back.
- **A test that showed a window no longer does so on the desktop of whoever runs the tests.** The tray's test makes a small window and asks for the foreground with it; it now runs on a desktop of its own, as the new overlay test does.
- **`scripts/apt-install.sh` fetches the package lists again before a download is tried again.** `apt-get update` ends well when a mirror cannot be reached, with the old lists; the download then asked three times for a file the mirror had replaced. Seen with real apt against a mirror that was out for 15 seconds and came back with a newer package.
- **Three tests that failed for reasons of their own are corrected**: 1.0.0's lock test under Debian's BusyBox shell, the uninstall flow's suite under a 32-bit PowerShell (it now refuses to run there: its fixture would have asked for real elevation), and a test of the instance socket that failed about once in a hundred runs on a busy machine.
- **All six were merged and run together before the release**: every gate on Windows and on Linux, and each of the round's six rigs on the merged tree.

### Known, not fixed

- Linux, live screen: where something near comes into a picture that stands still, the background steps back in one map (1.0.0's limit of a sixteenth, as above), and the depth of the whole picture still moves a little with the model's lowest and highest number. A way to calm both was tried on the round's maps outside the program and is not built in.
- Linux, live screen: the depth model reads a film's black bars as a frame in front of the picture, in every release alike. A proposal that puts the bars at the far end is an open pull request (#184) and not in this release: it wants eyes on the monitor first.
- Linux, live screen: whether the pace holds on a 3840 × 2160 screen where KDE Plasma sends 62 to 64 frames a second could not be run without a graphics card under the desktop.
- Linux, the Odyssey 3D set-up: a clock that is set back during a start makes that one start fail with "did not get the lens board ready"; the next one works. A `wine` script that is pointed at another Wine after the set-up needs the set-up run again.
- Linux: a helper that does not leave by itself stays when the program is ended by a signal (the program catches none).
- Windows and Linux: a second *Apply* inside a recovery's countdown waits for the old moment; a wait whose moment has passed reads "Trying once more in 1 seconds" and starts at once. A settings file over 16 MiB is refused with its name, and nothing says what to do with it.
- Linux, live screen: the page does not say that the remembered screen was not accepted when the desktop asks again.
- The items of 1.0.1 that are not named above stand as they were.

### Hardware verification

Pending: nothing of this release was run with the monitor, the camera, the lens, or Leia's and Samsung's software; the Odyssey 3D set-up ran under real Wine with stand-ins for Leia's programs. `docs/TESTING.md` lists the checks under "New in 1.0.2". First: `any2odyssey-setup` once, then the first Start output after a login (the picture comes with eye tracking, and the start takes no longer than it did), then Start output twice in a row (the second at once). Then the live screen on a desktop with windows, nothing moving but a clock: by the numbers the picture breathes in depth about half as much as in 1.0.1. The checks of 1.0.1 that are not replaced there, and those of 1.0.0 and 0.9.9, are still to do as well.

## 1.0.1 (2026-11-20)

A ninth audit release. 1.0.0 was the one release no audit round had read, so this round read its five pull requests first and then the whole program again, in six parts: the shared core, the live screen, the viewer and the controls, the Windows side, the Odyssey 3D set-up under Wine, and the installer with the workflows. Nothing found was grave. Seven findings were of the middle kind and about thirty were small; all but one are corrected here (the one needs a Windows PC to look at, and is under "Known, not fixed").

After updating, run `any2odyssey-setup` once (or `install.sh` from the archive): five files of the Odyssey 3D set-up changed, and an update of the program does not replace them in the data folder.

**Linux, live screen: the depth as a whole no longer steps when the nearest or the farthest thing in the picture changes a little.** The depth is measured against a range, from the farthest thing in the picture to the nearest, and that range was meant to follow the scene slowly, "so a near object entering does not push everything else back at once". It never did: each map came to that step already brought to 0 to 1 by its own farthest and nearest point, so the range it was measured against was always 0 to 1 and had nothing to follow. Something near that came into the picture moved everything else back in the next map; up to 0.9.9 the blend of old and new depth softened that, and since 1.0.0 only the sixteenth that a place may lag. The live screen is now given the model's own numbers, and the range follows them as it was written to. So that this does not bring a new fault with it, the range may be at most a quarter of the new picture's own span behind it at either end: without that limit, a cut to a scene with a fifth of the range before it would have been shown at a fifth of its depth, and would have needed about 118 maps (some 20 seconds on the processor) to get to nine tenths. With it, such a scene has four fifths of its depth with the first map. A small change is calmed; a large one is taken at once, all but that quarter. In numbers, at steadiness 60: where the nearest point rises by a third of the range, a place in the background moves from 0.333 to 0.332 where it moved to 0.300; where the nearest point rises tenfold, it moves to 0.097 where it moved to 0.088, which is the large change taken at once. A screen that stands still comes to rest as fast as before (about 20 steps of 150 ms at steadiness 60). **None of this has been seen on the monitor**: it is the first thing to look at, and `docs/TESTING.md` says how.

**Linux, the Odyssey 3D set-up: a start of Leia's stack that failed is no longer taken for a running stack.** A start leaves a note when it has ended well, and a later request compares the note with the lens board's link. The note of an earlier start stayed while a new start was under way: when that start failed ("Leia's service did not get the lens board ready"), or the program that asked was ended while it waited, Leia's programs and Wine's server were up with the old note, and the next request found a running stack. The lens helper said "on" and the weaver started with a board the service never got. The note now goes before a start begins. And the line of Leia's log that says the board answered has to be one written during this start: it was looked for in the whole of the newest log file, where the line of the run before can stand.

### Fixed

Linux, the live screen:

- **The depth's range follows the scene**, as above.
- **A stop and a start of the live screen close together no longer load the depth model twice.** The live screen that ended let go of the model after the new one had loaded it.
- **The memory of a finer depth detail is given back when a coarser one is chosen.** Three buffers of the model's picture kept the size of the finest detail that had been chosen: about 36 MB on a 3840 × 2160 screen after *Finest*, until the output was stopped.
- **A frame that differs from the last one only in the byte of each pixel that holds no colour is not a new picture.** 1.0.0 left the unused bytes after the rows out of that comparison; the fourth byte of a pixel in the formats without transparency is as little the desktop's to keep the same. No desktop that was run in the rounds changes it.
- **A depth map whose numbers lie further apart than a number can say is refused** and no longer stops the program. The model does not make one; the step can now be given numbers of any size.

Linux, the Odyssey 3D set-up (run `any2odyssey-setup` once to have these):

- **A failed start is not taken for a running stack, and the board's line has to be this start's**, as above.
- **Wine's libraries are taken from the Wine that runs.** The set-up and each start took them from the first of a list of usual folders. With two Wines on one machine (WineHQ's under `/opt` beside a distribution's, or a build of one's own) the version that was checked was one Wine's and the libraries that were listed, copied and linked against the other's, which is how 0.9.9 described a prefix in which nothing started. The folder beside the `wine` on the PATH is now looked at first. A `wine` that is a script of the user's own is not read, and the list is used as before.
- **The set-up stops with a reason when Wine does not take its registry entries.** A failed import of the lens board's entries ended the set-up without a word; a failed registration of Leia's eye tracker ended it before the line that was to say so; and a failed import of the camera's entries was let pass, so that the set-up said "done" and the eye tracker never got a camera. After the camera's import the set-up now asks Wine whether the entries are there.
- **A monitor that was unplugged no longer stays in the Wine prefix.** The entries of monitors that were there at an earlier start and not at this one were left, and Leia's service was shown a monitor that is gone. (A monitor that was unplugged before the last start under 1.0.0 is not known any more, and its entries stay.)
- **Eye tracking finds its camera bridge again when the note of the bridge's process number is lost.** The note is in the session's folder, which goes at logout. A bridge that lived on was known to nobody: every start then made a second one, which could not have the port ("is port 47816 taken?"), until the next boot. The bridge is now also looked for among the user's own processes, by its program file.
- **The message about a Wine that is too old names WineHQ's `winehq-devel` and `winehq-staging`**, the packages that put a `wine` on the PATH, and so do `INSTALL.txt` and `docs/LENS-SWITCH.md`. They named `wine-devel` and `wine-staging`, which install under `/opt` alone.

Linux, installing:

- **On Arch with an NVIDIA card, `any2odyssey-setup --yes` installs what is missing where `onnxruntime-cpu` is installed.** It asked pacman for `onnxruntime-cuda` in the same request as everything else; the two packages cannot be installed together, pacman's question whether to remove the one is answered with no where nobody is asked, and the whole request was dropped: none of the missing packages was installed, at every run. The installed runtime now stays, and a note gives the command for the other.
- **Removing an archive installation beside the package no longer removes the Wine side by default.** The package's own note sends a user with both to `install.sh --uninstall`, saying that the settings and the depth model stay; that run then asked whether to remove the Wine side, about 1 GB, with yes as the answer to Enter and to `--yes`, and the Wine side is in the data folder that the packaged program uses too. Where a package is installed as well, the answer is no unless it is given; `any2odyssey-setup --uninstall` is as before.
- **On Debian and Ubuntu a Wine that the set-up refuses is not installed.** A missing `wine` was installed from the distribution, which has Wine 8 to 10 where 11.6 is needed: the Odyssey 3D step then stopped at "this Wine is too old", with a package in the way of WineHQ's. The installer now asks apt for the version it would install first, names WineHQ's packages where that is too old, and leaves the Odyssey 3D step out with one line.
- **A Wine side built from other sources is noticed after an update.** The note "made by another version" compared the eight scripts that are copied. A release that changes only the source of a library the set-up compiles left the old library in the prefix and said nothing. The set-up now notes what it was made from, and the installer compares that. A Wine side made by 1.0.0 or earlier has no such note and is compared as before, and with the sources 1.0.0 shipped.
- **The lens board is waited for after its udev rules are installed.** With nothing slow between the two steps (the depth model there already), the Odyssey 3D step looked for `/dev/odyssey-lens` before udev had made it, and was left out.
- **No icon cache is made in `~/.local/share/icons/hicolor` where there was none.** GTK takes such a cache for the whole truth, and icons that other programs put there later without renewing it were not found.

Linux, the program:

- **A named pipe where `companion.log` belongs no longer holds the program at its first line of the log.** The log was the one file of the data folder still opened the waiting way: the controls stood still, `--quit` did nothing, and `--self-test` never ended. The log is also made for its owner alone (0600) like the other files there; one that is there already is made so at the next line.
- **The saved token of the shown screen is read as the settings are**: not waited on where it is a pipe, and not read whole whatever its size.
- **Quitting no longer leaves a helper behind that did not leave by itself.** After a quit request the program gave the window three seconds to close and then ended the process, also when the window had closed long before and the program was waiting for the lens helper or the weaver to leave, which it ends after three seconds of its own that begin later. A helper that hung was left with its window and the lens on, under a line in the log that the window had not closed. The process is now ended that way only while the window has not closed, and the wait for the helpers has an end of its own (about five seconds). Reasoned from the code, not seen.
- **Full screen is forgotten when the output window goes.** With the output in full screen, a change of *Output format* to Odyssey 3D and back made a new window, not full screen, that the program took for full screen: with Lens output the lens was switched on over a window, the F11 hint was missing, the first F11 did nothing and the first Esc did not close. Reasoned from the code, not seen.
- **A picture is opened by what it is, not by its name.** A PNG, JPEG or BMP without an ending, with another one's, or with one more behind it (`.png.part`) was refused, though the desktop offers the program for such files by their content. The formats that are opened are the same three.
- **`--export-guide` writes where the path leads.** It replaced a symbolic link with a file and a file that may not be written with one that may, where saving a picture does neither. It now writes the link's target, refuses a file that may not be written (exit code 1), and keeps the permissions of a file it replaces.
- ***View logs* no longer holds the window for a quarter of a second** or more while the file manager is started.
- **A picture that is replaced by a pipe at the moment it is opened** can no longer leave the viewer at "Opening" for good, and a link or a pipe put in the place of a file that is being saved over is neither followed nor waited on: what was opened is asked, not the name.

Windows and Linux, the program:

- **Unticking *recover* during the wait for another renderer no longer turns 3D off.** Where a start meets another renderer it waits eight seconds and tries once more, with recovery on or off. That wait was counted as a recovery under way, which is what unticking the box ends: "Automatic recovery cancelled", and the second try was never made.
- **One wait for another renderer per start, as the rule says.** A recovery that was counting down was taken for that wait's second try, and a start in that time that met another renderer waited once more where it was to turn 3D off with its advice.
- **Two programs that start together with a log over 4 MiB no longer lose the generation that was kept.** Both found the log large; the second moved the new file of one line over `companion.old.log`. One of them now does it at a time (`companion.rotating` is there for as long).
- **Replacing settings that could not be read no longer copies a file of any size.** The copy that is kept of the old file was made on the window's thread whatever its size; a file over 16 MiB is now refused with its name, and left.

Windows:

- ***Restore overlay* asked for while the conversion is still starting is done when the 3D picture comes.** The request was taken and dropped while Samsung's window was not there yet, under a line in the log that it had been asked for.
- **The uninstall runs when a 32-bit program starts it.** Windows gives such a program the 32-bit PowerShell, and the uninstall stopped at "Use 64-bit PowerShell to uninstall Any2Odyssey." It now starts itself again in the 64-bit one.
- **`--layout-probe` and `--inspect` say what failed**, the first without ending with exit code 101 where nobody reads its error output, the second at all.
- **Three questions to Windows about a display's colour are asked with the whole of what Windows fills in**, not with its first part alone. Nothing was seen to go wrong; the same was corrected for another question in an earlier release.
- **`scripts/verify-package.ps1` no longer depends on what is installed on the PC.** On a PC with the full converter or a newer Any2Odyssey it called a correct package wrong. `install.ps1` has a switch `-PackageOnly` for it.

### Tests and releases

- **The workflows' `apt-get` steps have a time limit and three tries.** The first run of the checks for 1.0.0's merge was cancelled after 45 minutes, in which one such step had taken 17 minutes and another had not ended: apt-get gives up on a mirror that does not answer, not on one that answers slowly. `scripts/apt-install.sh` fetches with a limit, tries again what was stopped, and fails with a line that says so.
- **Five tests that could not fail can.** A test that runs itself again in a process of its own passed when that process ran nothing; two tests of how files are written did not test what their comments said; the installer's test of the udev rules looked at a folder the installer never touches; and the workflows' check of the desktop entry had `|| true` behind it.
- **New tests for what had none**: `tools/mkboard.py` run as a program, `setup.sh` run from its first line to its last against stand-ins for Wine, and the cases above, each with a test that fails without its correction where one could be written.
- **Run before the release**, on the merged tree, on Linux: the formatter, clippy and the tests of the three Linux packages, the Windows packages compiled and linted for Windows, and the script tests under `sh` and BusyBox. The Windows tests, the installer's suites under both PowerShells and dash ran in the pull request's checks, not before it: of what is new, the PowerShell had been read and not run until then.

### Known, not fixed

- Windows: whether the title of the jump list's entry is given to Windows in a form it takes (`crates/windows/src/taskbar.rs`) could not be settled without a PC; if not, the jump list has no entries, and nothing says so.
- Linux, the Odyssey 3D set-up: with a `wine` on the PATH that is a script of the user's own, Wine's libraries are still taken from the list of usual folders.
- Linux, opening a picture and writing over a file: the check of what was opened is made on Linux; on Windows and elsewhere the name is still looked at first.
- The items of 1.0.0 stand as they were.

### Hardware verification

Pending: nothing of this release was run with the monitor, the camera, the lens, Wine or Leia's and Samsung's software; the Odyssey 3D set-up's scripts were run against stand-ins only. `docs/TESTING.md` lists the checks under "New in 1.0.1". First: `any2odyssey-setup` once, which has to end with "done" (it now asks Wine a question it did not ask before, and stops on a no), and two cold starts of the Odyssey 3D output. Then the live screen with something small and near coming into the picture, and with a hard cut between two scenes. The checks of 1.0.0 and 0.9.9 are still to do as well.

## 1.0.0 (2026-11-20)

Five pull requests (#172 to #176) that did not come from an audit round of this project: one changes how the depth of the Linux live screen follows what moves and how a large screen is shrunk for the picture, and four close gaps that fixes of earlier releases had left. Each was read against the code it changes; all five were merged and run together, on Windows and on Linux. One of the five (#176) failed the Arch package as it came and was corrected before it went in. There was no audit round for this release.

This is the first release numbered 1.0. The number changes nothing about what has been seen on the monitor and what has not: that is in the README's status line and in `docs/TESTING.md`, as before.

**Linux, live screen: the depth of something that moves no longer trails behind it.** Each new depth map is blended into the one before it, by as much as *Depth steadiness* says. Where something had moved, that left the old depth standing for a while: at the default steadiness (60) a place that went from near to far kept 60 % of its old depth after the next map, and at 95 the depth had not reached a new scene after five seconds (a "Known, not fixed" item of 0.9.9). What is kept of the old depth is now at most a sixteenth of the range. A small change is calmed as before; a large one is followed with the next map. The other side of it: a place whose depth the model itself puts near in one map and far in the next was calmed by the blend, and is not any more once the two are more than about a tenth of the range apart. Whether that shows on a real picture has not been seen; it is the first thing to look at on the monitor. (#172)

**Linux, live screen: a screen shown at less than half its size is shrunk with every pixel counted.** For the picture, four pixels of the screen were read for each pixel of the view, however many it stood for: four of seven where a 5120 × 1440 screen is shown at 1080 lines, four of nine where a 3840 × 2160 screen is shown at 720. Thin lines and small text came and went as they moved across the pixels that were read. The depth model's picture has been made with every pixel since 0.9.8; the picture that is shown is now made the same way. It costs time where it applies: on the development PC, 1.8 ms where it was 0.6 for a 3840 × 2160 screen at 720 lines, and 1.7 ms where it was 0.6 for a 5120 × 1440 screen at 1080 lines (six threads; about 3 ms on four of its cores). The pull request's own measurement, on a slower machine, was 11.1 ms against 2.1 and 7.9 ms against 2.0. The loop has 15 ms for a picture. A screen shown at half its size or more (3840 × 2160 at 1080 lines, 1920 × 1080 at any) is made as before. (#172)

### Fixed

Linux, the live screen:

- **The depth follows what moves**, and **a screen shown at less than half its size keeps its thin lines**, as above. (#172)
- **A frame that differs from the last one only in the unused bytes after its rows is not a new picture.** 0.9.9 stopped taking a frame that is the picture already shown for a new one, by comparing the frame's bytes; where a desktop sends rows with unused bytes after them, those were compared too, and what they hold is not the desktop's to keep the same. The rows are now taken without them before the comparison. No desktop that was run in the rounds sends such rows. (#174)

Linux, the settings:

- **A named pipe where a saved file belongs is refused by what was opened, not by its name.** 0.9.9 refused to read one, by looking at the name and then opening it: a pipe put there between the two was opened all the same, and the program waited for a writer that never comes. The step that replaces damaged settings had no such look at all. A saved file is now opened without waiting and what was opened is looked at, for reading and for the replacement; both say that it is not a regular file. (#175)

Linux, the Odyssey 3D set-up:

- **The set-up stops with a message when the lens board's USB descriptors are cut short or are no descriptors.** `tools/mkboard.py` wrote `board.h` and `device.reg` from whatever it had read; with fewer bytes than the board's descriptors have, the Wine side was built with what was there. Both files are now left as they were. (#173)

Linux, installing:

- **`install.sh` stops when it cannot get its turn.** A run that found another one at work in the same prefix waited for it, and after five minutes went on beside it with a note; where there is no `timeout` it went on when the wait failed for any reason. Two runs at once is what the lock is there to prevent (0.9.4: half an installation, or a second copy of the Wine set-up inside the first). It now ends with "could not acquire the busy lock" and changes nothing; the run can be started again. (#176)

### Tests and releases

- **The link check of the packaged documents follows symbolic links, and a link has to end inside the package.** A link to a file that is beside the documents on the machine that builds the package, and not in it, passed. The Arch package's license files are links from the documents' folder to `usr/share/licenses`, on purpose: the check takes the package's folder as a second argument for that, and `PKGBUILD` and the workflows give it. (#176; as it came, the check refused those links and the package was not built, and with BusyBox's `realpath` it reported every link.)
- **The lock of `install.sh` is tried with a lock that is really held**, by the test itself as another run holds it, and only the five minutes are stood in for. (#176)
- **The shared check of a captured frame's size no longer counts past the largest number**: a frame whose rows cannot all be there is refused for that, not by luck. No desktop can send one; the buffer's own size is looked at before. (#172)
- **All five were merged and run together before the release**: every gate on Windows and on Linux.

### Known, not fixed

- Linux, live screen: the pictures between two depth maps are still made with the older map, which is about one and a half times as old as the model takes for a map (0.2 s at *Standard* on the processor, a few hundredths on a graphics card). #172 changes what a new map does when it arrives, not when it arrives.
- Linux, live screen: what #172 gives up, as above: a depth that changes by more than about a tenth of the range from one map to the next is not calmed at any steadiness.
- The items of 0.9.9 that are not named above stand as they were.

### Hardware verification

Pending: nothing of this release was run with the monitor, the camera, the lens or Leia's and Samsung's software. `docs/TESTING.md` lists the checks under "New in 1.0.0". Most in need of a real run, and the only way to judge it: the live screen with something moving on the shown screen, at *Depth steadiness* 60 and 95 (the depth should be where the moving thing is, with no shape left behind), and the same screen with nothing moving (edges between near and far should be as calm as in 0.9.9). The checks of 0.9.9 are still to do as well.

## 0.9.9 (2026-11-19)

An eighth audit release: seven pull requests (#163 to #169) from a round that began with 0.9.8, the first release in a long while that no round had looked at. It found a defect of 0.9.8 that every user of the live screen has, three times over and independently; ran the depth model on a real graphics card for the first time; took the Odyssey 3D set-up across upgrades of Wine with fourteen real Wine versions; ran the Windows controls in a window of their own for the first time since 0.9.1; and ran the Linux edition on the desktops that were left. At the end all seven were merged and run together, on Windows and on Linux.

After updating, run `any2odyssey-setup` once (or `install.sh` from the archive): three scripts of the Odyssey 3D set-up changed, and an update of the program does not replace them in the data folder. From this release on the installer says so when they are another version's.

**Linux, live screen: 0.9.8 computed the depth at twice the depth detail that was chosen.** 0.9.8 hands a large screen to the depth thread at twice the model's size, to be halved there, and the thread still named the frame's own long side as the detail: nothing was halved. On every screen 1036 pixels wide or more *Standard* was a model picture of 1036 × 588 where 518 × 294 was meant, and on a 3840 × 2160 screen *Fine*, *Finer* and *Finest* were one and the same. On the processor a depth map took 1.1 s where it now takes 0.18 s (1920 × 1080), and 0.9.8 made 0.6 maps a second where 0.9.7 made 5; on an RTX 5090, 26 ms against 14. The picture never waits for the depth, so each picture was shifted by the shape of a much earlier one: around anything that moved, the depth trailed. The steadiness 0.9.8 was made for is kept, and is in fact better than what 0.9.8 shipped: its notes' "0.8 %" is what this release measures (0.87 % on a 5120 × 1440 desktop; 0.9.8 as released: 1.12 %). (#164, found independently by #167 and #169)

**Linux, an NVIDIA graphics card: the page says where the depth is computed, and a card that cannot be used no longer ends 2D to 3D.** Where the card's provider did not start, 0.9.8 either went to the processor without a word (no cuDNN, CUDA's libraries missing or of another version), or 2D to 3D did not work at all (cuDNN of another major version, no card visible, the card's memory refused). In every one of these the depth is now computed on the processor, and one line under *Depth detail* reads "Depth is computed on the NVIDIA GPU." or says that the processor is used and why. (#164)

**Linux, the Odyssey 3D: an upgrade of Wine no longer leaves the output dead.** Three cases, each seen with real Wine and stand-ins for Leia's programs: Wine upgraded while the output had been used in the session (dead until the next login); a set-up made under Wine 11.10 to 11.17 and Wine then 11.22 (nothing answered, and running the set-up again did not help); and a first set-up that was interrupted (never repaired by a later run). The fix of 0.9.8 itself holds across 11.21 to 11.22, to wine-staging, and back. (#168)

**Linux, KDE Plasma, live screen: frames are taken as they arrive.** Since 0.9.7 Plasma sends 62 to 64 frames a second, a little faster than the program's loop turned: each frame was taken later than the one before until one was left out, and it began again. (#169)

### Fixed

Linux, the live screen:

- **The depth model is shown a large screen at the chosen depth detail**, as above. If the depth looked better in 0.9.8 than it does now at *Standard*, that was a finer detail under the name of *Standard*: choose *Fine* or *Finer*. (#164, #167, #169)
- **Frames are taken as they arrive when the desktop sends 62 to 64 a second.** At 62.5 and 64 frames a second a frame was 13 to 17 ms old when its picture was in the frame file and 24 to 164 frames were left out in 40 seconds; it is 4 to 6 ms now with 1 to 10 left out, as at 60. The loop's time is 15 ms, and it is counted from when a turn was due, so one late turn is worked off within three or four pictures and not over a dozen. (#169)
- **A frame that is the picture already shown is not a new picture.** A desktop sends a frame for every time it has drawn the screen, changed or not: GNOME while only the pointer moves and the pointer is not drawn in. Each was shrunk, given to the depth model and woven again, and the page read "5 pictures a second" over a still screen. It reads "not changing" now; a frame that differs in one pixel is still a new picture. (A "Known, not fixed" item of 0.9.7.) (#163)
- **niri, and any desktop that gives up a cast the same way: Start no longer waits for ever at "Choose the screen".** The desktop closed the session without an answer; the page now ends within about three seconds with a line that says so. (#166)
- **Where a desktop has no screen cast, the page says what is missing**: an X11 session, a portal without a backend for the desktop, no portal, no session bus. Each was the portal library's own sentence before. A picker that is closed on Hyprland's portal backend reads "no screen was chosen", not "Invalid session". (#166)
- **The page says why no picture has come when the desktop sends only buffers that hold none**, with their number and the reason. (A "Known, not fixed" item of 0.9.7.) (#163)
- **Every desktop but KDE: the page warns that the Odyssey's own screen must not be the one shown**, because only KWin can keep the 3D picture out of a capture. The warning was never given outside KDE. (#166)

Linux, 2D to 3D on an NVIDIA graphics card:

- **A card that cannot be used is left for the processor, and the page says which of the two computes the depth**, as above; a run that fails on the card in the middle of a live screen goes on with the processor. (#164)
- **The card's memory is given back.** The program kept what the largest picture it had ever worked on had needed: 6.9 GB after one square picture at *Finest*, 2.9 GB after a stopped live screen. It is about 0.8 GB after a still image now, and the model is let go when the last live screen ends (it is loaded again at the next, in about a second on the card). On the processor the program's own memory went from 1352 MB to 393 MB after one still at *Finest*. (#164)

Linux, the Odyssey 3D set-up and its Wine side:

- **An upgrade of Wine while the output had been used in the session**: the old Wine's server turned every new program away, the stack still counted as running, and every request failed until the next login. The first request after the upgrade is now one cold start. (#168)
- **A set-up made under Wine 11.10, 11.14 or 11.17, and Wine then upgraded to 11.22**: Wine's own update of the prefix never ended, with one of the set-up's stand-ins in its way, and `setup.sh` run again stopped after two minutes without a word. Wine is now let update the prefix by itself first, at the first start after an upgrade and in `setup.sh` over an existing prefix. The first start after an upgrade of Wine takes about 36 seconds for it, where it took 22 to 24. (#168)
- **`setup.sh` refuses a Wine older than 11.6**, on which the set-up was made without a complaint and every request then failed after 80 seconds. Debian 12 and 13 and Ubuntu 24.04 to 26.04 ship Wine 8.0 to 10.0. (#168)
- **A first set-up that did not get through** (Ctrl+C, a full disk) is made anew by the next run; a set-up whose stand-ins no longer fit the installed Wine says "Wine has been upgraded since the Odyssey 3D set-up was made … Run the set-up again" within three seconds. (#168)
- **An update of the program says when the Wine side still runs another version's scripts**: `install.sh` and `any2odyssey-setup` print a note. (A "Known, not fixed" item of 0.9.8.) (#168)
- **The set-up compiles with the mingw-w64 headers of Ubuntu 22.04.** (#168)
- `setup.sh --help` prints how to use it; it took `--help` for the Windows drive. (#163)
- **Another program can no longer shorten the frame file by accident**, which ended the viewer with signal 7: the file is read-only once the viewer has its own handle. Wine 11.22 and wine-staging 11.22 read it as before. (A "Known, not fixed" item of 0.9.7.) (#163)

The viewer and the controls:

- **A right click opens the menu again on the Controls page** (Windows, lost in 0.9.7) and on the viewer (Linux). The page's drag area lay over the area the menu hangs on. (#165)
- **Windows: after a change of the display's scale the window's smallest size is that of the new scale.** A window made at 100 % and then at 200 % could be made 700 × 360 pixels small, half of what it needs. (#165)
- **A choice of the viewer made just before the controls are minimized, hidden or turned to another page is written.** It stayed unwritten for as long as the viewer's page was not drawn. (#167)
- **One value of the wrong kind in `viewer-settings.json` no longer makes every choice the default**; the values are read one by one. (#167)
- **Linux: a named pipe where `viewer-settings.json` or `lens.json` belongs no longer leaves the program without a window.** (#167)
- **zenity: Save preview PNG… opens in the current folder**, not at the root of the file system, where nobody but root can save. (#166)
- **An existing picture in a folder that takes no new file can be saved over again** with `--screenshot` and Save preview PNG…, as up to 0.9.5. (A "Known, not fixed" item of 0.9.7.) (#163)

Windows, uninstalling:

- **A shortcut the uninstaller leaves because it cannot read it is named**, and the window waits for a key. (A "Known, not fixed" item of 0.9.7.) (#163)

### Corrections to the notes of 0.9.8

- "The viewer keeps its choices … Both editions": the Windows edition has no viewer page and writes no `viewer-settings.json`. What Windows keeps, as before, is `settings.json` and `ui-settings.json`.
- "At a cost of two to four milliseconds a depth map" was the shrink alone, on six threads. On one thread it is 17 to 27 ms for a 3840 × 2160 screen, and the halving for the model adds 3 ms at *Standard* and up to 25 ms at *Finest*.
- "It is 0.8 % now": that is what this release measures. 0.9.8 as released gave 1.12 % on the same kind of desktop.

### Tests and releases

- **The published programs no longer hold the build machine's folders** (what `makepkg` warned of), and a check in CI and at a release looks for them in the Windows program, the archive's and the Arch package's. (#163)
- **Ten of 39 changes of 0.9.8, taken out again by hand, were noticed by no test**; each has one now. One of them matters on the monitor: the start script's test was fed a file with other line ends than the set-up writes, so the lines 0.9.8 added were never run as they run there. (#167)
- **The Windows controls were run in their own window**, on a desktop that is never shown, with a build that has no conversion engine: Chinese, Japanese and Korean window titles in *Source window*, the 360-point window, a click made while the pointer moves, every way of ending the program, 22 damaged settings files, and an hour in the tray (under 0.1 % of a core, no file written, nothing growing). `docs/TESTING.md` marks what was seen there. (#165)
- **The live screen has a rig that times it**: from the desktop's frame to the frame file a picture is 4 to 6 ms old at 1920 × 1080 and 8 ms at 3840 × 2160; a reader on the Wine side that stalls or stops never holds the program up. (#169)
- **The Linux edition was run on labwc, COSMIC and niri, and in X11 sessions.** Hyprland and gamescope could not be started without a graphics card. `docs/PLATFORMS.md` no longer says that a live screen works on X11: no X11 session that could be put together has a screen cast. (#166)
- **All seven pull requests were merged and run together before the release**: every gate on Windows and on Linux, the Wine rig, and the timing rig, which showed that the comparison of each frame with the one before costs nothing that can be measured.

### Known, not fixed

- Linux, live screen: the depth map a picture is made with is about one and a half times as old as the model takes for a map (0.2 s at *Standard* on the processor, a few hundredths on a graphics card), because the picture never waits for it; at *Depth steadiness* 95 the depth had not reached a new scene after five seconds. A still screen's depth is eased in at six steps a second, which on a graphics card is slower than a moving screen's.
- Linux, an NVIDIA card: one processor core is busy for as long as a map is computed on the card; the first map ever made with a set of NVIDIA's libraries can take from 9 seconds to two minutes (later starts 1 to 4 s); a card that was left for the processor is tried again only at the next start of a live screen or of the program.
- Linux, the Odyssey 3D: why the stand-in stops Wine's update of an older prefix is not known, the fix goes around it; the Arch package says nothing after `pacman -U` (`any2odyssey-setup` does); the eye tracker's pause stands as it was, with its cause now found in Wine's server (a socket is given up for good after one refused datagram).
- Linux, live screen: the window is drawn once a second while the live screen is on, also hidden; the page cannot tell that the Wine side has stopped reading; 2160 lines with the depth model on the processor are short of 60 pictures a second.
- Linux: COSMIC's portal backend ended most casts by itself in the rig's nested session (not known on real hardware); nothing has been seen on Hyprland or on gamescope.
- A change of the viewer's choices in its last second is lost when the program is ended by a signal or by its session ending; the same has always held for the lens numbers.
- Two title-bar buttons are both named "Minimize" to a screen reader on Linux: the one that hides the controls in the tray minimizes where there is no tray. Both stay.
- Hebrew and Arabic are drawn in the wrong order (it needs a library for the Unicode bidirectional algorithm, which the program does not have).
- Windows: the controls do nothing of their own at a logoff with 3D on; a `settings.json` saved as UTF-16 is taken for damaged until *Apply*.
- The items of 0.9.8 and 0.9.7 that are not named above stand as they were.

### Hardware verification

Pending: nothing of this release was run with the monitor, the camera, the lens or Leia's and Samsung's software. The graphics card was a real one, reached through WSL and the Windows driver; Wine was real, with stand-ins for Leia's programs; the desktops ran without a screen. `docs/TESTING.md` lists the checks under "New in 0.9.9". Most in need of a real run: the Odyssey 3D set-up's start after `any2odyssey-setup` on the G90XF under Linux, because `setup.sh`, `start-stack.sh` and `ensure-stack.sh` changed and they are the ones seen working there; the live screen at *Standard* with something moving (the depth should follow sooner than in 0.9.8, and the page names the NVIDIA GPU); and a right click on the Windows Controls page.

## 0.9.8 (2026-11-18)

Two fixes and one addition, from an evening with 0.9.7 on the monitor's own machine.

After updating, run `any2odyssey-setup` once (or `install.sh` from the archive): a package or archive update does not replace the scripts of the Odyssey 3D set-up in the data folder, and the set-up does.

### Fixed

- **Linux, the Odyssey 3D: an upgrade of Wine no longer breaks the output until the set-up is run again.** At its first program after an upgrade Wine updates the prefix, and that registers Wine's own toast library again for two of the three classes the set-up's stand-in answers for. Wine's has no notifier yet, and Leia's session (`SRSession.exe`) ended within a second of every start. With no session nothing counted as running: every Start was a cold start of half a minute that ended the stack the Start before it had brought up. Seen on the monitor's machine after Wine 11.21 became 11.22. `start-stack.sh` now writes the stand-in's three entries at every start, with the monitors' entries it writes there anyway. An installation that is in this state is repaired by `any2odyssey-setup` (or `setup.sh`), which also brings the new script.
- **Linux, live screen: the depth of a large screen is steadier.** The screen is shrunk for the depth model, and that read four pixels for each of the model's, however many it stood for: all of them down to half size, one in fourteen of a 3840 × 2160 screen at standard depth detail. Since 0.9.7 a screen on KDE Plasma arrives at its own size (0.9.6 and before were sent 1920 × 1080), so small text and thin lines came and went in the model's picture with every pixel they moved, and the depth with them. Measured on a 5120 × 1440 desktop with text: moved by one pixel, the depth map changed by 6.7 % of its range on average, where the way of 0.9.1 gave 1.5 %; it is 0.8 % now. Every pixel of the screen is counted, at a cost of two to four milliseconds a depth map. On a 3840 × 2160 screen with little small detail the three were the same (0.2 to 0.3 %).

### Added

- **The viewer keeps its choices between runs**: what the image is and how it is shown, depth strength and pop-out, depth detail, and a live screen's sharpness, depth steadiness, lines and pointer. Every launch began at the defaults, a live screen at standard depth detail and 1080 lines. They are written to `viewer-settings.json` in the data folder a second after the last change and when the program ends; a value in the file that the page could not have chosen counts as the default for that choice. The output is not started by it. Both editions.

### Known, not fixed

- Linux, live screen: a window that takes a small part of the screen has a small part of the depth model's picture, and its depth is coarse (edges inside it bend). *Depth detail* at Fine or Finer gives it more.

- Linux, the Odyssey 3D: after an upgrade of Wine the set-up's copies of five of Wine's own libraries (the ones its stand-ins pass calls on to) stay those of the Wine before it, until the set-up is run again. Seen working across 11.21 to 11.22; "run it again after a Wine upgrade" still stands.
- An update of the program does not update the Odyssey 3D set-up's scripts in the data folder; the set-up has to be run again for that.
- The items of 0.9.7 stand as they were.

### Hardware verification

This one was made on the hardware: Wine 11.22, the G90XF and Leia's own programs. With the entries as Wine's update left them `SRSession.exe` ended at each of its starts with an access violation after `toast_manager_factory_QueryInterface ... E_NOINTERFACE`; with the three entries written by the script's new lines it stayed. The depth map's steadiness was measured with the model on this machine's GPU and screenshots of its two screens; the live picture with it was looked at on the G90XF's own screen (a full-screen picture looked right). The saved choices were run in tests only, not yet through the window. Not yet seen: the picture itself after a start with the new script, and the tests under a shell other than this machine's (CI's `sh` is dash).

## 0.9.7 (2026-11-18)

A seventh audit release: six pull requests (#154 to #159) from a round that again ran things no round had run. The Linux edition was run on KDE Plasma 6.8 and on GNOME 51 themselves, without a screen, with their own compositors, portals and file dialogs. The live screen was fed by a stand-in for the desktop's portal and a stream made for the purpose: every format, row length, size, turn and mark a desktop may send. The Wine side of the Odyssey 3D output ran under real Wine for the first time outside the one machine it was written on. The eight published files of 0.9.6 were taken apart and the documents that ship with them read against the programs. The changes of 0.9.6 were attacked with counter-examples that were run, and the defects that earlier notes list as known and not fixed were reproduced and fixed where no hardware is needed. At the end all six were merged and run together, and two of them were run on each other's rigs, which found two more faults that neither showed alone.

**Linux, KDE Plasma: the live screen was taken at 1920 × 1080 whatever the screen was.** A 3840 × 2160 screen arrived shrunk and a turned one pressed into 16:9, because the program named a size and KWin gives a reader the size it names.

**Linux, sway and other wlroots desktops: a rotated or mirrored screen is shown upright in the live screen.** It was shown on its side.

**Both editions: window titles and file names in Chinese, Japanese and Korean are drawn in their letters**, with a typeface of the system. They were drawn as boxes. Hebrew and Arabic get their letters too, still in the wrong order.

**The picture that 0.9.6 copies into the frame file's memory arrives whole under Wine.** That was the open question of 0.9.6; under real Wine 11.21 and wine-staging 11.21, of 273,884 test pictures sent, none that the Wine side kept was torn, of the wrong size or out of order, and 594 slider steps of the real program at 3840 × 2160 were the same byte for byte on both sides. Leia's own programs were stand-ins.

### Fixed

Linux, the live screen:

- **A screen is taken at its own size on KDE Plasma.** The program wished for 1920 × 1080 inside a range of sizes, and KWin 6.8 offers every size from 200 × 200 to 10000 × 10000 and resizes the cast to the reader's wish: the page read "Live: a 1920 × 1080 screen" for a 3840 × 2160 and a 2560 × 1440 screen, and a screen turned on its side arrived pressed into 1920 × 1080. No size is wished for now, and the page reads the screen's own. On Plasma the program therefore handles up to four times the pixels it did, so the pictures a second there may differ from 0.9.6; after the update, log out and in once, because KWin keeps the size 0.9.6 asked for until then. GNOME and sway always sent the screen's size. (#159)
- **A rotated or mirrored screen is shown upright on sway.** The stream says how the frame is turned and the program did not read it: all seven turns and flips other than none were shown turned. They are turned back now, and on sway the picture equals the compositor's own screenshot pixel for pixel for all eight, at scales 1.25 to 2, on two screens and with ten bits a colour. KDE and GNOME send no such note and hand out a turned screen upright; nothing is turned there, and their pictures are the same with and without this change. (#157)
- **A frame that the desktop marks as no picture is not shown.** A buffer marked as corrupted, or of size nothing, was read like any other and shown: stale or unfinished contents for one picture. xdg-desktop-portal-wlr marks a frame it could not complete, KWin a pointer-only update, mutter a frame it could not record. A buffer of size nothing used to be read as the whole buffer; it is no picture now. (#157)
- **GNOME: Start works after the portal's backend was restarted.** The first and second Start read "Live capture ended: the desktop's screen-cast portal refused: ... Unavailable cursor mode 1." The pointer is asked for by name only where it is to be drawn in. (#159)
- **The memory of a larger screen's pictures is given back when the screen is smaller again, and that of more lines when fewer are chosen.** A session at 1920 × 1080 that had shown 7680 × 4320 for a moment stayed at 438 MB until the output was stopped; it is back at 201 MB within a second. After 2160 lines of detail and back to 1080 it kept 71 MB; it does not. (A "Known, not fixed" item of 0.9.6.) (#157)
- **A screen faster than 60 Hz is no longer copied picture by picture.** The program named its limit in a field that desktops answer with nothing; the field they keep to was not named. A 120 Hz screen on sway delivered about 90 pictures a second and a 120 Hz screen on Plasma about 110, each copied and most thrown away. The desktop is now asked for 62 a second at the most: not 60, because mutter 51 then delivers 48 (measured; from 61 up it delivers 60), and KWin delivers exactly what is asked. A stream that names one fixed rate above sixty was refused with "no more input formats" and is taken. (#157)
- **A screen cast with a side longer than 16384 pixels ends with a line that says so**: "Live capture ended: the screen cast is 17000 × 17000 pixels, and no more than 16384 a side can be shown." The size the program used to name carried that limit along, with the unspecific "no more input formats". (#157, #159)

Linux, the Odyssey 3D set-up and its Wine side:

- **Wine no longer ends Leia's service in the middle of its own start.** Wine ends a prefix's services three seconds after the last ordinary program has left, and a service does not count as one; `start-stack.sh` pauses 6, 8 and 2 seconds between short programs. Where the service reported itself running early enough, Wine stopped it, with the USB driver, during the start, and every later request was a cold start of half a minute. Wine's server is started with a minute of patience first. Seen with a stand-in for the service only: on the one machine with the monitor the start evidently wins by timing. (#158)
- **A Wine server that was killed or crashed no longer leaves the output broken until the next login.** Its programs stay in the list of processes and can do nothing; they were found by name and taken for a running stack: eye tracking said "on" with no tracker, every stop took twelve seconds, and the output did not come back. The stack now counts as running only with a server of its prefix, and a start ends what the dead server left behind (Wine's programs of this prefix, nothing else). (#158)
- **The output starts where the frame file's folder has letters outside Western European ones.** The Wine-side program opened the file by a path that had gone through the prefix's 8-bit code page: under a runtime or home folder with Cyrillic or Japanese letters the page read "cannot open the frame file". (#158)
- The try-out the set-up prints, `sleep 20 | any2odyssey-lens-helper`, showed the lens for an instant on a first start, which itself takes longer than that; it is `sleep 60` in the set-up, the README and `docs/LENS-SWITCH.md`. `start-stack.sh` no longer leaves a copy of its shell beside Leia's session for as long as that runs. (#158)

Linux, the viewer:

- **One instance again where `XDG_RUNTIME_DIR` names something that cannot be used** (a file, a folder that cannot be written, another user's). Every launch became a window of its own and `--quit` found nothing. The instance socket is then tried in the data folder, for listening and for handing over. (A "Known, not fixed" item of 0.9.6.) (#156)
- **An ONNX Runtime library file that is cut short is refused with a message** that names the file and its two lengths. The viewer ended with "Bus error" inside the system's loader when 2D to 3D was chosen. (A "Known, not fixed" item of 0.9.6.) (#156)
- **KDE without zenity: Save preview PNG… opens in the current folder with the name filled in.** kdialog read the bare file name as an address, opened at "http: any2odyssey-preview.png" and left the Name box empty. (#159)

Linux, installing:

- **On Arch, `--yes` answers pacman's own question too.** The installer's questions were answered and pacman's "Proceed with installation? [Y/n]" was not: run from a script or another installer, with nobody to answer, it installed no packages after `--yes`. apt-get, dnf and zypper were always told not to ask. `any2odyssey-setup --yes` is the same script. (#154)
- The Arch package names `python` among its optional dependencies (two tools of the Odyssey 3D set-up are Python scripts; `namcap` reported it as an error). (#154)

Both editions, the controls and the viewer:

- **Titles and names in Chinese, Japanese and Korean are drawn in their letters.** The program's own typefaces do not have them, and the Windows source list and the viewer's path box and status line showed boxes. The first time such a letter is shown, a typeface of the system is looked for on a thread of its own (on Windows in the Fonts folder: Yu Gothic, Microsoft YaHei or JhengHei, Malgun Gothic, in the order of the display language; on Linux what fontconfig names, else Noto Sans CJK) and added: nothing is read at start, each file once, eight at the most, and the program grows by the size of the file, 14 to 20 MB. Hebrew and Arabic are given their letters the same way. (A "Known, not fixed" item of 0.9.6.) (#156)
- **A click made while the pointer still moves does not shift the page.** The interface library lets a page be dragged with the mouse like with a finger, and a press on a button counted as the start of a drag: the page moved by what the pointer travelled in that moment, some six points for a hand that is slowing down, on any page longer than its window, in every release. A page is dragged by a finger only now; the wheel, the scroll bar and Tab are as they were, and the page-sized invisible Tab stop that the drag area was is gone. (#155)
- **The window can be made 360 points high instead of 580**, and every page scrolls below its full height. 580 was more than the whole of a 1920 × 1080 screen at 200 % or a 1366 × 768 one at 150 %: the window's lower edge lay outside the screen, with its resize handles. The smallest width is 700 as before. (A "Known, not fixed" item of 0.9.6.) (#156)
- **Tab scrolls to a focused control whose last three points lie under the window's frame.** In the default window *Reset lens numbers* was left there. (#155)

Windows:

- **Uninstalling removes a shortcut that is hidden or marked as a system file.** 0.9.6 left it behind as one that "could not be removed", pointing at a program that was gone. (#155)

### Documents and the release page

- **The release pages of 0.9.5 and 0.9.6 named the wrong requirements**: "needs glibc 2.39 or newer" and "ONNX Runtime 1.22 or newer", where the program has needed glibc 2.35 and ONNX Runtime 1.17 since 0.9.5. Users of Debian 12, Ubuntu 22.04 and Debian 13 were told the archive, or its 2D to 3D, was not for them. Both pages were corrected by hand, the text the release workflow writes is corrected, and `scripts/check-doc-numbers.sh` now compares these two numbers in the README, `INSTALL.txt`, `docs/PLATFORMS.md` and the release text with the installer and the build. (#154)
- **The third-party lists name the Rust standard library**, which is in every program and in no list of crates, with the crates it is built from; and the Linux list names the few functions of PipeWire's headers that are compiled into the program. A crate that a list names twice is noticed. (#154, #156)
- `docs/PLATFORMS.md` names the Windows setup program among what is distributed, four files with provenance where it said three, and says that GNOME's output window has a title bar (it said it had none). `docs/LENS-SWITCH.md` says that the archive and the Arch package carry the Wine helper's script and source (it said no package contains it) and that the set-up needs `pgrep`. `contrib/wine-lens/README.md` named its license at a path that exists in no package. (#154, #159)
- The notes of 0.9.6 say that the accessibility part added "about 0.4 MB". That is the Windows program; the Linux program grew by 1.8 MB in the Arch package and 2.3 MB in the archive.

### Tests and releases

- **The eight published files of 0.9.6 are what the release says they are.** Every checksum and every attestation holds; the ZIP, the archive and the Arch package hold the tag's files and nothing else; the setup program, decoded and not run, holds the ZIP's 27 files; every crate whose paths are in the programs is in the lists. The archive was installed, started, updated from 0.9.5 and 0.9.3 and removed in a throwaway Arch tree, and the published Linux programs are on the accessibility bus with their controls named. (#154)
- **The licenses of the dependencies are checked** in CI and at a release: `deny.toml` lists the nine a dependency may have, and one that arrives under another fails the check. (`cargo deny check licenses` reported 383 errors before, and was not run.) (#156)
- **The changes of 0.9.6 were reviewed by running counter-examples.** The frame file's memory held against a reader that follows the Wine side's protocol at 280 pictures a second, on a full memory disk and at every size; 180,000 resizes are the same byte for byte as the routine they replaced; the instance socket gives one instance at every path length. Each fix of 0.9.6 was taken out again: eight were not noticed by any test and have one now. (#155)
- **The live screen has a stand-in portal and a stream producer** (kept with the round's scripts, not in the repository): 131 cases of formats, sizes, row lengths, turns, marks and paces, run under AddressSanitizer too, with nothing read outside a buffer. Every offered format arrives with every pixel as sent, the ten-bit ones included. (#157)
- **Round 20's fixes of the live screen hold on KDE Plasma and GNOME**: a screen that is switched off or removed ends the cast with its line, Stop returns within about two seconds while the portal stands still, and Start works again. The script that lets the Odyssey show its own screen live loads in KWin 6.8 and hides the output window from the cast. (#159)
- `start-stack.sh` and `ensure-stack.sh` have a test with stand-ins for Wine, and the guide in the Linux archive is compared with what its program exports at a release too. (#158, #154)
- **All six pull requests were merged and run together before the release**, and two were run on each other's rigs. That found what no single one showed: a test of one failed against a script change of another although they merged without a conflict; two changes to the same request for a stream had to be made one; a limit of exactly sixty pictures a second cost a fifth of them on GNOME; and the upper size limit had gone with the wished size.

### Known, not fixed

- Hebrew and Arabic are drawn in the wrong order, and Arabic unjoined: the interface library places letters from the left in the order they are stored. The same holds for Thai and the Indian scripts as far as they need shaping.
- A typeface of the system stays in memory once it was needed (14 to 20 MB), and on a Linux system whose fontconfig is not set up for a language, Chinese characters are drawn in the Korean shapes of Noto Sans CJK, as in every other program there.
- A mouse can no longer drag a page by its background; a touch screen's page no longer glides on after the finger is lifted.
- Linux, KDE Plasma, live screen: a change of the shown screen's rotation, scale or resolution while live keeps the old size and shape until Stop and Start; KWin tells the reader nothing.
- Linux, GNOME, live screen: with the pointer not drawn in, moving only the pointer still brings pictures, so the page reads "5 pictures a second" where it would read "not changing".
- Linux, live screen: a desktop that sends only buffers that cannot be read leaves the page at "Waiting for its first picture" without a reason. None is known to.
- Linux, live screen at 2160 lines with sharpening: still below 60 pictures a second on the processor alone.
- Linux, Wayland: `--show` cannot raise the controls (they ask for attention); GNOME's output window has the windowing library's plain title bar, not the desktop's own.
- Linux, the Odyssey 3D: the eye tracker's pause. With a stand-in tracker under Wine 11.21 a socket hears nothing more after the first refused datagram when the set-up's own library is in Wine's server. It was left as it is because this arrangement is the one seen working on the monitor; if tracking does not come back after a second start in one login, this is where to look.
- Linux: if another program of the same user shortens the frame file while the output is on, the viewer ends (signal 7).
- Windows: a shortcut that another program holds open without sharing stays after an uninstall and is not named.
- `--screenshot FILE` into a folder that cannot be written is refused where 0.9.5 overwrote an existing file in place.
- Both Linux programs hold the build machines' paths (`makepkg` warns of it); `setup.sh --help` takes `--help` for the Windows drive; two title-bar buttons are both named "Minimize" to a screen reader on Linux.
- The items of 0.9.6 that are not named above stand as they were.

### Hardware verification

Pending: these changes were made and tested without the monitor, the camera, the lens or Leia's and Samsung's software; KDE Plasma and GNOME ran without a screen or a GPU, Wine ran with stand-ins for Leia's service, tracker and weaver, and the Windows controls were drawn in tests and not in a window. `docs/TESTING.md`, "Pending hardware verification", lists the checks under "New in 0.9.7". Most in need of a real run: the Odyssey 3D set-up's start, stop and restart on the G90XF under Linux, because three of its scripts changed and they are the ones that were seen working there; the live screen on KDE Plasma, which now arrives at the screen's own size (the size named on the page, and the pictures a second on the 165 Hz Odyssey, where about sixty is expected); and a Japanese, Chinese or Korean window title in the Windows source list.

## 0.9.6 (2026-11-17)

A sixth audit release: six pull requests (#146 to #151) from a round whose methods were again new. The Linux live screen was run for real under a headless desktop with PipeWire and the screen-cast portal, and the session was changed under it: modes, screens taken away, the portal and PipeWire stopped and restarted. The Linux program was run about 2,200 times with the operating system failing underneath it, one system call at a time, and on full, read-only and missing folders. Every page of the interface was drawn at every window size, display scale and zoom, walked with the keyboard, and read through the accessibility tree, on Linux and on a desktop of Windows that is never shown. The saved files of all fourteen releases were made with each release's own code and read by every other. The six Linux releases were measured against each other with one script, and the hot paths that were left were profiled. A second account and a hostile server played the attacker against the running program, and found nothing. And the changes of 0.9.5 were attacked with counter-examples that were run. Built and tested in CI; not yet run on the monitor, see Hardware verification.

**Linux: a click on the empty card beside *Reset lens numbers* reset the lens numbers.** The viewer's hand-drawn buttons reacted across the whole width of their card.

**Linux, live screen: a screen that was unplugged or switched off was shown as "not changing" for ever, and Stop output did nothing while the desktop's portal did not answer.**

**Linux, Odyssey 3D output: a still picture follows a slider about five times faster, and each live picture is handed over in less than half the time.** The pictures are the same byte for byte.

**Both editions: screen readers are given the controls.** They were given an empty window.

### Fixed

Linux, the live screen:

- **A live screen whose screen is taken away ends, instead of reading "not changing" over a frozen picture.** When the shown screen goes, PipeWire only pauses the stream and removes the screen's node; the program took an error or a lost connection for the end and nothing else. After the shown output was unplugged or switched off, or the portal's backend ended, the page read "Live: a 1920 × 1080 screen, not changing, so nothing is redrawn" for as long as the output was left on. With xdg-desktop-portal-wlr 0.8.4 on sway the same came of a change of the shown output's mode. The page now reads "Live capture ended: the screen cast was ended by the desktop." about a quarter of a second later, and Start works again. (#150)
- **Stop output no longer freezes the window while the portal does not answer.** With xdg-desktop-portal or its backend standing still, Stop output had no effect, the window was not redrawn, and `--quit` from a second launch was accepted and not acted on. The portal's answer is now waited for 1.5 seconds at the longest. (#150)
- **The page says when a live screen waits for its first picture, and when the desktop has paused the cast.** It read "Choose the screen to show in 3D in the desktop's dialog" with no dialog open, on a desktop that sends the first picture only when something moves, and "not changing" for a cast the desktop had paused. (#150)
- **A screen that the desktop renders with ten bits a colour can be shown.** The page read "Live capture ended: no more input formats." at every start. The four formats of ten bits a colour are accepted and written as a byte a colour. Run on sway only. (#150)

Linux, the viewer:

- **A button reacts where it is drawn.** In the default window *Reset lens numbers* was 158 points of button in 440 points that reacted, and so were *Save preview PNG…* and *Choose another screen*: a click on the blank card, to leave a number field for instance, reset the calibration, and it was saved a second later. (#148)
- **One instance again where the path of the instance socket is long.** Without a runtime folder the socket lies in the data folder, and under a deep home folder or `XDG_DATA_HOME` its path passed the 107 bytes a socket's address holds: every launch became a window of its own and `--quit` found nothing running. (#147)
- `--inspect`, `--layout-probe`, `--test-capture-exclusion`, `--test-offscreen` and `--worker`, which only say that they run on Windows, exit 1 when nobody reads what they say; they ended with a panic message and exit code 101. (#147)

Linux, installing:

- **Ctrl+C ends an installer that waits for another one.** The wait that 0.9.5 added ran in a process group of its own: under bash the installer waited on and then installed after the Ctrl+C, and under dash it ended only when the wait was over. (#149)
- **Alpine with the `gcompat` package is refused like Alpine without it.** That package puts a loader of glibc's name in front of musl, which was all 0.9.5 looked for: the installer said Done over a program that answers "Error relocating ...: mallopt: symbol not found". (#149)

Both editions, the controls and the viewer:

- **A checkbox reacts on its box and its text**, not on the whole row beside it. On Windows that is *Convert automatically when this app opens*, *Recover if the conversion process stops* and *Minimize and close to the tray*, which save at once: a click on the blank card to bring the window forward could change how the program starts. (#148)
- **Assistive software is given the controls.** UI Automation showed one element on every page of the Windows controls, and the Linux viewer was not on the accessibility bus at all, although the notes of 0.8.7 say that buttons and Help topics announce a name and a role: the program was built without the part of its interface library that does it (AccessKit). It is built in now (15 more libraries in the Windows program, 20 more on Linux, about 0.4 MB; both third-party lists name them). Sliders, drop-down boxes, the path box and the lens number fields have the name of their row ("Depth strength, slider"), and the window has a name on Linux. (#148)
- **Tab starts at About and goes in reading order, and the page follows the focus.** The first stop was the title bar's invisible drag area, then the title bar from Close backwards; and the focus went on to buttons below the window's edge, where Space pressed something nobody could see. (#148)
- **A selected list entry and selected text can be read** (a contrast of 2.6:1 in the light theme and 1.7:1 in the dark one; now 7.1:1 and 5.1:1), and so can the primary button under the pointer, which now darkens there in the light theme. (#148)
- **Zoom stops where the title bar is still whole.** Ctrl+Plus, Ctrl+Minus and Ctrl+0 zoom the window; at 3.0 About and Help were outside it. The limit is 1.7 for the default window and more for a wider one. (#148)
- **`--screenshot FILE` no longer destroys the picture that was there when the new one cannot be written**: on a full disk, and for `shot.jpg` on a healthy one (JPEG has no transparency, and the file had been emptied before the encoder said so). It is written beside the destination and moved into place. (#147)
- Past the path limit of Windows, a name that Windows reads as a folder (only dots or spaces, or `..`) made a file called like the folder, and the write said it had succeeded. It is refused, as it is in a short folder. (#149)

Windows:

- **Uninstalling goes on past a shortcut that cannot be deleted** (a Desktop that has become read-only, a file another program holds open). Every release stopped there with the worker removed and the program left, at every further try. The shortcut stays and is named. (#149)
- **The About page names the replaced version as the releases are named**: "Replaced 0.9.4", where it read "0.9.4.0" beside "Installed version 0.9.5". (#146)

### Faster

- **Linux, a still picture on the Odyssey 3D output: a step of a slider takes 22 ms instead of 125 at 1080p, and 34 instead of 180 at 4K** (measured without a GPU). Each view is now resized row by row, on several cores, straight into the picture; the result is the same byte for byte. The lens-interleave output is fitted the same way (20 ms instead of 176). (#151)
- **Linux, live screen: each picture is copied into the frame file's memory instead of being written to it.** The hand-over was the largest single part of a live picture's turn. At 2160 lines with sharpening a turn takes 17.4 ms instead of 21.9, and the test loop shows 55 pictures a second where it showed 41; at 1080 and 1440 lines 60 a second were reached before, now with 15 % less processor time. The file, its header and the order in which they change are as before; where the file is not kept in memory, or the system does not set the memory aside, the picture is written as before. While the output is on, the viewer's memory reads higher by the pictures in the frame file (49 MB at 1080 lines, 194 MB at 2160). This is the one change here that is new `unsafe` code, and what Wine's side sees of it could not be run without the monitor: it is the first hardware check. (#151)
- **The six Linux releases, measured the same way, show no step back that needs a change.** Start, idle (four wake-ups a second, 122 MB) and memory are level from 0.9.0 to 0.9.5. Two steps cost time and buy something: a step of a slider in the preview is a third slower since 0.9.2, for a session that holds 561 MB instead of 752; and the first 2D to 3D of a run takes 50 to 100 ms more since 0.9.1, for the check of the depth model's SHA-256. (#151)

### Tests and releases

- **The check of the actions' commits could pass a commit that no tag and no branch holds.** When GitHub's first answer was an error (a rate limit, a 502), the error's text stood where the names of tags are expected, and the commit was accepted. Only an answer that arrived counts now, and a question GitHub did not answer is told apart from the answer no. (#149)
- **The saved files of all fourteen releases are in the repository, written by each release's own code, and are read in CI**: settings, the status files, the lens numbers, what the published Linux programs wrote through their windows, and the installation records under both PowerShells (270 small files). Every release's files are read by 0.9.5 with every value kept and nothing called damaged, and the reverse; a change of a file's format now fails a test in the pull request that makes it. (#146)
- **About 2,200 runs of the Linux program with a fault in them left no settings file, exported picture, guide or depth model half written or empty**, and no temporary file beside them. The atomic writer has a test with a write that fails part way, which runs on Windows too. (#147)
- **The interface is swept without a display in the tests**: 41 states at 29 sizes for controls cut by the window's edge or lying over each other, the accessibility tree for names, the Tab order, and the themes' contrasts. At the default and the smallest window, at scales 1 to 3 and maximized up to 4K, nothing was cut or overlapped. (#148)
- **The changes of 0.9.5 were reviewed by running counter-examples**, and each fix of 0.9.5 was taken out again to see whether a test notices: five did not have one (the lock held to the end of a run, the three package managers' options for an unknown package, the last line on a `noexec` home), and have one now. (#149)
- **A second account on the same machine and a hostile server found nothing.** The other account could not connect to the instance socket, read the camera bridge's secret, answer its challenge or take its port unnoticed; a wrong, oversized or redirected depth model download left nothing behind. The Wine side was not part of it. (no pull request)
- One test of the depth model's download never ended when no file descriptor was left; it ends now. (#147)

### Known, not fixed

- Window titles and paths in Chinese, Japanese, Korean, Hebrew or Arabic are drawn as boxes, in the Windows source list and in the viewer's status line: the program's typefaces do not have those letters.
- Linux, live screen: a rotated screen is shown on its side on sway (the frame says how it is turned; the program does not read that yet). Not tried on KDE or GNOME.
- Linux, live screen: the picture buffers keep the largest size a session has seen until the output is stopped.
- Linux, live screen at 2160 lines with sharpening: still below 60 pictures a second on the processor alone.
- Linux: with `XDG_RUNTIME_DIR` set to something that cannot be used (a file, a read-only folder, another user's), every launch is a window of its own, as before.
- Linux: an ONNX Runtime file that is cut short ends the viewer inside the system's loader ("Bus error") when 2D to 3D is chosen.
- The smallest window is 580 points high, which is more than a 1080p screen at 200 % or a 768p screen at 150 % shows.
- Going back to an old release brings its own faults back: the installers of 0.8.0 to 0.8.6 refuse to install after a newer release was uninstalled (`install.ps1 -AllowDowngrade`, or delete `%LOCALAPPDATA%\Any2Odyssey\install-result.json` first), and the Linux `install.sh` of 0.9.0 and 0.9.1 over a newer installation removes the Wine side.
- The items of 0.9.5 that are not named above stand as they were.

### Hardware verification

Pending: these changes were made and tested without the monitor, Wine, the camera or the lens; the live screen ran on sway with a stand-in for the Wine side, the Windows controls on a desktop that is not shown, and the timings without a GPU. `docs/TESTING.md`, "Pending hardware verification", lists the checks under "New in 0.9.6". Most in need of a real run: the Odyssey 3D output under Linux with a still picture and a live screen (that the picture copied into the frame file's memory arrives as the written one did), the live screen on KDE Plasma when the shown screen is changed or taken away, and the Windows controls with Narrator while 3D is switched on and off.

## 0.9.5 (2026-11-17)

A fifth audit release: six pull requests (#139 to #144) from a round that again used methods not used before. The eight published 0.9.4 files were downloaded and taken apart, and the release run and the workflows behind them were read with tools; the Linux archive was run as a first-time user in root trees of eleven systems, each with its own package manager and libraries; the Windows setup program was compiled around a stand-in package and run more than 230 times, in every mode and with every outcome the installer can have; the Windows controls were driven for thousands of operations and measured from outside the process; the changes of 0.9.4 were attacked with counter-examples that were run; and analysers no earlier round had used were pointed at all of the code. No new features. Built and tested in CI; not yet run on the monitor, see Hardware verification.

**Linux: the archive's installer now works on openSUSE and on AlmaLinux 10 and its relatives, where 0.9.4 installed no package and said Done over a program that did not start.**

**Linux: 2D to 3D accepts ONNX Runtime 1.17 and newer.** 0.9.4 refused the 1.21 that Debian 13 and Ubuntu 25.10 package, and ended with Microsoft's own 1.22.0.

**Windows: controls that another program restored from the tray answered no click.** Update if a window-layout tool is in use.

### Fixed

Linux, installing on distributions other than Arch:

- **AlmaLinux 10, RHEL 10, Rocky 10 and CentOS Stream 10: the package step no longer installs nothing.** Their lists have no ONNX Runtime; with that one name unknown `dnf` installed none of the others, the installer still said Done, and the program stopped with "error while loading shared libraries: libpipewire-0.3.so.0". It is the fault 0.9.4 fixed for `apt-get`. `dnf` is now told to leave out a name it does not have, and after the package manager has run the installer looks again and names what is still missing. (#141)
- **openSUSE: the installer knows zypper and openSUSE's package names.** It knew no package manager there, gave no advice and ended with Done over a program that does not start. Where no package manager is known, the list of missing files now comes with what to do. (#141)
- **A user who may not use sudo, a wrong password, a system without sudo or without udev no longer end the installation at the udev step.** The depth model and everything after it were skipped. A failure there is reported with the command to run as root, and the installation goes on; without sudo the installer names the commands instead of trying them, and as root it runs them itself. The same in `--uninstall`. (#141)
- **Alpine (musl) and systems with a glibc older than 2.35 (AlmaLinux 9) are refused before anything is installed**, with the reason. Everything was installed there although the program cannot start. (#141)
- The last line no longer says Done when libpipewire or libxkbcommon was not found, or when the system does not let the program run from where it was put (a home folder mounted noexec). (#141)
- **Ubuntu 26.04: the Odyssey 3D set-up finds `wineserver`.** That release's wine package has the program beside Wine's libraries only, and the set-up ended with "'wineserver' is needed". (#141)
- The installer asks for `pgrep` (`procps-ng`, `procps`), without which the Odyssey 3D set-up ended on Fedora's and AlmaLinux's minimal installations. The set-up's messages that named Arch's packages name the others too. (#141)
- `INSTALL.txt` says that the program needs glibc 2.35 or newer, not 2.39: it runs on Debian 12 and Ubuntu 22.04, with one line of the loader at every start that can be ignored. `docs/PLATFORMS.md` names the releases the archive was run on. (#141)

Linux, installing:

- **Two installations at the same time, or an installation and an `--uninstall`, now wait for each other.** 0.9.4 gave each run its own copy of the program, but the icon, the documents and the Wine set-up were still written in place: of 100 pairs started together over an installation, one run stopped in 37 and 26 ended different from what one run leaves (files of the Wine set-up gone, `setup.sh` among them), and an installation racing a removal left half an installation in 20 of 100. A run now takes a lock on the prefix folder (`flock`); a second run says that it waits. Without `flock`, or where the folder has no locks, a run goes on as before. (#140)

Linux, the viewer:

- **2D to 3D works with ONNX Runtime 1.17 and newer**, where it asked for 1.22: the 1.21 of Debian 13 and Ubuntu 25.10 is accepted. The depth maps are the same byte for byte. (#141)
- **With Microsoft's own build of ONNX Runtime 1.22.0 the viewer no longer ends the moment 2D to 3D is chosen** ("std::vector<const OrtEpDevice*>::front(): Assertion '!this->empty()' failed"). It was the version the message of 0.9.4 named as the oldest accepted. (#141)

Linux, the Wine side:

- A tracker that ends without a word of its own (a tool it needs missing, for example) is reported with the command to run to see why. Since 0.9.4 the reason was empty: "without eye tracking ()". (#140)

Windows, the controls:

- **Controls hidden in the tray and put back on the screen by anything but Any2Odyssey answer again.** A window that a window-layout tool or a task switcher restored stayed "hidden" as far as the program knew: it stood above every other window, with no taskbar button, and answered no click, the Close button's included. The program now asks the window itself whether it is minimized. (#144)
- The same state came of a show request within a few milliseconds of the hide (10 of 10 at 0 ms, 7 of 10 at 5 ms, none from 20 ms on): a script, or two requests that cross. Fixed with it. (#144)

Windows, the setup program:

- **A full installation shows the installer's notices and asks before the elevation prompt.** That another Windows account's conversion worker is about to be replaced was said only after it had been replaced (a "Known, not fixed" item of 0.9.4). The wizard first runs the installer's read-only checks on their own, shows what they print with **Continue with the installation?**, and only then starts the installation. No stops there with nothing changed. A controls-only installation and a silent one run as before and show the notices afterwards. (#142)
- **When PowerShell is ended in the middle of the installation, the setup program no longer says that nothing was installed.** It says that the installation may be incomplete and to run Setup again. (#142)
- A notice is shown once; after a controls-only installation the box held it twice. (#142)
- The page that shows the installer's reason names the Back button instead of "Setup cannot continue. Please click Cancel to exit." (#142)
- A long reason is shown whole, in a box that scrolls and from which it can be copied; the page cut it at the right edge and after the eighth line. (#142)

Windows, installing from the ZIP:

- **`Install.cmd` and `Install-GUI.cmd` no longer run the installer of another folder.** Windows PowerShell takes `[` and `]` in the path of a script for a pattern: extracted to a folder `Any2Odyssey[1]` beside a folder `Any2Odyssey1` that held a package as well, the other folder's release was installed, and nothing said so. The launchers now ask first which files would be run, and when those are not the package's own they say to move the package to a folder without `[` and `]` and install nothing. Brackets that fit no other folder install as before. (#143)
- A folder with a `[` that no `]` closes gets the same advice instead of "The specified wildcard character pattern is not valid". (#143)

Windows:

- **Uninstalling no longer stops half-way at a read-only shortcut whose attributes cannot be changed** (a Desktop on a read-only share, for example). Since 0.9.4 the uninstaller ended there with the worker removed and the program left. The shortcut stays, the uninstaller names it and goes on. (#140)
- Past the path limit a file is no longer saved under a name that ends in a dot or a space, which neither Explorer nor the program can open or delete: `--export-guide "<long folder>\guide.html."` makes `guide.html`, as it does in a short folder. (#140)

### Tests and releases

- **The published 0.9.4 files are what the release says they are.** All four checksums and attestations verify; the ZIP, the Linux archive and the Arch package hold the tag's files and nothing else; the setup program, decoded and never run, holds the 27 files of the ZIP inside the loader of the pinned Inno Setup 6.7.4. (#139)
- **The workflows named one action by a commit that its repository no longer holds on any branch or under any tag**: the `stable` branch of `dtolnay/rust-toolchain` was rebuilt fourteen hours after the 0.9.4 release. The action is now named by the commit of its `master` that the old one was built on, and `scripts/check-action-pins.sh` asks GitHub at every CI run and release whether each of the seven pinned commits is still under a tag or on a branch. (#139)
- **Two tests of the Linux installer that need root ran in no job.** The Arch job, which runs as root, now runs the installer's tests too and fails when one is skipped. (#139)
- The Windows third-party list is compared with what `cargo tree` gives for the Windows target; it was compared with nothing, and was right. (#139)
- **The setup program's own decisions are tested in CI**: `installer/tests/wizard/test-setup-wizard.ps1` compiles the wizard around the real launcher and a stand-in installer and checks the exit codes, the reasons, `/ALLOWDOWNGRADE`, an interrupted installation and the notice shown once. It installs nothing. (#142)
- The release workflow compares the glibc the installer asks for with the one the built program needs. (#141)
- **The Windows controls were driven for more than 16,000 operations** (tray, settings, Help, resizing, display and power messages) and measured from outside: handles, GDI and USER objects, threads and memory do not grow, and the data folder is left clean. (#144)
- **The changes of 0.9.4 were reviewed by running counter-examples**, and each test 0.9.4 added was run against the code it was written for put back. Besides the faults above, one test would have waited for CI's time limit instead of failing, and nothing tested that the depth model's lock is kept until the model has its name; both are corrected. (#140)
- Miri, PSScriptAnalyzer, gcc's and clang's analysers, cppcheck, clippy's stricter lints, ruff, mypy, bandit, shellcheck with every optional check and two spelling tools found nothing besides the launchers' brackets. Two assertions of `test-uninstall-flow.ps1` failed where `%TEMP%` has a `[` or a backtick in its path; they compare text now. (#143)

### Known, not fixed

- The setup program is not code-signed.
- With `%TEMP%` at or below a junction, the setup program stops at once with "Error 448: The path cannot be traversed because it contains an untrusted mount point" (a protection Inno Setup switches on). `/NOREDIRECTIONGUARD` on its command line gets past it, and the ZIP is not affected.
- `install.ps1` and `Install-Launcher.ps1` started by hand from a folder with `[` and `]`, and the setup program with such a `%TEMP%`, are not protected as the two `.cmd` launchers are.
- Debian 12, Ubuntu 22.04 and 24.04 and the RHEL 10 family have no ONNX Runtime package: 2D to 3D there needs a library named in `ORT_DYLIB_PATH`. Any of Microsoft's releases from 1.17 on works.
- On Debian 12 and Ubuntu 22.04 the loader prints "weak version `GLIBC_2.39' not found" at every start, and Debian's and Ubuntu's ONNX Runtime prints several hundred "Schema error" lines when it is loaded. Both are harmless.
- Immutable systems (Fedora Silverblue, SteamOS): the installer names what is not installed and does not say Done, but it does not know `rpm-ostree` or how to unlock SteamOS.
- A file dialog's helper that has a session of its own is not ended by closing or replacing the dialog. Open and Save go on working.

### Hardware verification

Pending: these changes were made and tested without the monitor, Wine, the camera or the lens; the setup program's new question was run with a stand-in package only, and the distributions were root trees, not installed systems. `docs/TESTING.md`, "Pending hardware verification", lists the checks under "New in 0.9.5". Most in need of a real run: a full installation through the setup program on the G90XF PC (the ordinary case, from "Checking this PC" to the elevation prompt), the controls restored from the tray by another program, and the Odyssey 3D set-up on Ubuntu 26.04.

## 0.9.4 (2026-11-14)

A Windows setup program, and a fourth audit release: five pull requests (#133 to #137). The audit round again used methods not used before. The published 0.9.3 Linux archive was run as a first-time user on a distribution that is not Arch (an Ubuntu 24.04 tree and Ubuntu 22.04, with every package name checked against the lists of Debian, Ubuntu and Fedora); the scripts, the installers and the program were run under foreign locales, encodings, time zones and clocks; the changes of 0.9.3 were attacked with counter-examples that were run; and the defects earlier rounds had described but not fixed were reproduced and fixed.

**Windows: each release now has a setup program, `Any2Odyssey-<version>-windows-x64-setup.exe`.** The ZIP stays, holds the same files and installs the same thing.

**Linux on Ubuntu 24.04, Ubuntu 22.04 and Debian 12: the installer's package step installed nothing, and on Ubuntu 24.04 the Odyssey 3D set-up could not be built.** Both are fixed.

**Linux: two installations running at the same time could leave nothing installed in 0.9.3.** Update.

### Added

Windows:

- **A setup program.** Run it, choose **Full converter** or **Desktop controls only** (preselected on a PC without Odyssey 3D Hub), and approve the elevation prompt of a full installation with the account you are signed in to. It is a window in front of the installer the ZIP has always had, not a second installer: it carries the release package and runs the same scripts, so every check, the Installed apps entry and the uninstaller are the same, and an installation can be made, updated and repaired with the ZIP or with the setup program in any order. When the installer refuses, its own reason is shown, and Back returns to the choice; when it refuses an older release, the setup program asks whether to install it anyway. For an unattended installation: `/VERYSILENT /SUPPRESSMSGBOXES /LOG=<file>`, with `/CONTROLSONLY` and `/ALLOWDOWNGRADE`; the exit code is 0 when installed. It is not code-signed, so Windows SmartScreen may ask about a freshly downloaded copy: verify it against its `.sha256` and with `gh attestation verify`, then choose More info, Run anyway. `installer/README.md` has the details. (#133)
- The About page offers **Download installer** when the newest release has a setup program, under the same rules as the package link: the link is built from the version, never taken from the answer. Help and the offline guide describe the setup program. (#133)

### Fixed

Linux, installing on distributions other than Arch:

- **The package step no longer fails as a whole when the release lacks one of the packages.** Ubuntu 24.04, Ubuntu 22.04 and Debian 12 have no ONNX Runtime package; `apt-get` then installed none of the others either, the installer still said Done, and the program stopped with "error while loading shared libraries: libpipewire-0.3.so.0". Packages a release does not have are now named apart and the rest are installed. (#137)
- **The Odyssey 3D set-up builds on Ubuntu 24.04.** One of the small stand-ins for Wine did not compile with that release's MinGW headers, and the set-up ended there. (#137)
- **The installer asks for libxkbcommon-x11**, without which the program ended at once in an X11 session ("Library libxkbcommon-x11.so could not be loaded"). The Arch package lists it as an optional dependency. (#137)
- The ONNX Runtime counts as installed only under the name the program opens, `libonnxruntime.so`; Fedora has that in `onnxruntime-devel`, which is what the installer now asks for there. (#137)
- The Odyssey 3D set-up finds Wine's libraries where Fedora keeps them. (#137)
- `INSTALL.txt` says what the archive needs on Ubuntu, Debian and Fedora, and that 2D to 3D needs ONNX Runtime 1.22 or newer. When the library cannot be loaded, the message says so and names the packages of Debian, Ubuntu and Fedora beside Arch's. (#137)

Linux, installing:

- **Two installations at the same time no longer remove each other's copy of the program.** 0.9.3 copied the new program under one fixed name: of 100 pairs of installers started together, 52 ended wrong, 40 of them with nothing installed. 0.9.2 was not affected. (#136)
- A folder of your own in `share/applications` whose name looks like the installer's staging folder (`any2odyssey.desktop.` and ten characters) keeps its files; the clean-up removed it once it was a day old, and `--uninstall` at once. (#136)
- `./install.sh --uninstall` run from inside the installed folder also finds the program when `~/.local/share` is a link and the shell has followed it. (#136)
- A relative `XDG_DATA_HOME` (an unexpanded `~/.local/share`, for example) is ignored by the installer and by the Odyssey 3D set-up, as it is by the program. They used it as written, so the depth model was downloaded again at every update and `--uninstall` named the wrong folder. (#134)

Linux, the viewer:

- **An interrupted `--install-depth-model` no longer leaves its partial download behind.** Each left up to 99 MB, removed only a day later, and never if the next attempt worked. The next attempt removes them, and never one that is still being written by another installer. (#134)
- Closing or replacing a file dialog whose own program has already gone also ends what it left running. (#134)
- When neither Wayland nor X11 can be used, the message names the `WAYLAND_DISPLAY` that was tried instead of calling it unset. (#136)

Linux, the Wine side:

- **With `LC_ALL` set to a locale the system does not have, the Odyssey 3D output no longer says that eye tracking could not be started when it had**, and a tracker that does fail no longer ends the picture. The shell's own warning about the locale was read as part of the tracker's answer. (#135)
- The set-up reads the list of Wine's library exports correctly when the cross-compiler's objdump speaks the user's language (French, Portuguese, Russian and others); the list came out empty. Arch's objdump has no translations and was not affected. (#135)

Windows:

- **Uninstalling removes a shortcut that was made read-only.** It was left behind, pointing at a program that was gone. (#136)
- Settings, the guide and exported pictures can be saved in a folder whose path is near or beyond 260 characters (from 240 for the settings). Saving failed with "The system cannot find the path specified". (#134)
- A read-only settings file is refused at once instead of after 25 attempts. (#134)
- The installer's messages that said to run `Install.cmd` or `Install-GUI.cmd` also name the setup program's word for it. (#133)

Both editions:

- `--export-guide` names the file it could not write. (#134)

### Tests and releases

- **The release has eight files**: the setup program and its `.sha256` beside the ZIP, both Linux files and theirs. The setup program is built from the ZIP that is published beside it, by an Inno Setup pinned by version and SHA-256, and has a build attestation like the others. (#133)
- **CI runs the setup program for real** on its Windows runner: a full installation is refused there (no Odyssey 3D Hub), the controls-only installation installs exactly the ZIP's files, a second run repairs it, and the installed uninstaller removes it. (#133)
- **The Linux archive was run on Ubuntu for the first time.** What held: glibc, dash, mawk, `python3` without `python`, Debian's `wine` wrapper, both MinGW thread models, a user without sudo rights, no terminal. CI now compiles the Wine stand-ins with Ubuntu's MinGW, which would have shown the failure above. (#137)
- **Everything was run under foreign locales and clocks**: the script suites in ten locales with bash and with dash as `sh` (180 runs), the Windows installer tests under six cultures with both PowerShells (84 runs), the Rust tests and the program's command line under far-east and far-west time zones, at daylight-saving boundaries and with clocks up to 2106. The Windows installer, the Rust code and `install.sh` showed no locale or clock defect. (#135)
- **The changes of 0.9.3 were reviewed by running counter-examples.** Besides the faults above: the Wayland pacing (0 % CPU when idle in every state under sway), `--toggle`, the empty file argument, the settings recovery, the launchers in folders with `!`, `^`, `%` and `&`, and the camera secret held. (#136)
- The seven pinned GitHub Actions were checked against the tags their comments name: each commit is the one its tag points to.

### Known, not fixed

- ONNX Runtime 1.21, which Debian 13 and Ubuntu 25.04 and 25.10 package, is refused: the program is built for 1.22 or newer.
- The setup program is not code-signed.
- In the setup program, another account's conversion worker being replaced is announced after the installation, not before the elevation prompt as in the console.

### Hardware verification

Pending: these changes were made and tested without the monitor, Wine, the camera or the lens, and the setup program has installed the full converter on no PC yet (CI has no Odyssey 3D Hub). `docs/TESTING.md`, "Pending hardware verification", lists the checks under "New in 0.9.4". Most in need of a real run: a full installation and an update through the setup program on the G90XF PC, and the Odyssey 3D set-up on Ubuntu 24.04.

## 0.9.3 (2026-11-13)

A third audit release: seven pull requests (#125 to #131) from a round that again used methods not used before. The Linux viewer was run under a Wayland compositor for the first time; the published 0.9.0, 0.9.1 and 0.9.2 files were updated to one another with settings, the depth model and a Wine side in place; installations were run on full disks and killed at every step; the changes of 0.9.2 were attacked with counter-examples that were run; and the Windows installer and the settings code were run against read-only files, files held open, unusual folder names and damaged records. No new features. Built and tested in CI; not yet run on the monitor or on a real Wayland desktop, see Hardware verification.

**Linux on Wayland: up to 0.9.2 the viewer stopped answering whenever its controls could not be seen**, for example with the output window full screen over them. Update.

### Fixed

Linux, the viewer on Wayland:

- **The viewer no longer stops answering while its controls are covered, minimized or on another workspace.** A window that cannot be seen is given no moment to draw, and the controls and the output window are drawn by one thread: F11, Esc and a double click did nothing and the window could not be closed. On Wayland the viewer now paces its own drawing, at most 120 pictures a second. X11 and Windows are unchanged. (#129)
- **`--show` makes the controls ask for attention in the task bar.** It did nothing that could be seen: a Wayland window cannot raise itself. (#129)
- With `WAYLAND_DISPLAY` left over from a session that is gone and an X11 display available, the viewer starts on X11 instead of stopping. Where no window system can be used, the message names both variables and is also written to `companion.log`. (#129)

Linux, the viewer:

- **`--toggle` says that it switches the Windows conversion and exits 1.** It was listed in `--help`, exited 0, changed nothing and wrote a line to the log on every call; with no instance running it opened the viewer. (#127)
- Starting and closing the viewer no longer leaves a `settings.json` of Windows conversion defaults in the data folder. A file that is there is still read and left as it is. (#127)
- An empty file argument is a usage error (exit 2) instead of a silent exit 1. (#127)

Linux, installing:

- **Installing again from a shell with another `XDG_DATA_HOME` or `ANY2ODYSSEY_DATA_DIR` no longer removes a Wine side built in `~/.local/share/any2odyssey/wine-lens`**, about 1 GB with Leia's runtime in it. A folder that holds a built Wine side is never taken for the setup's own. (#128)
- **An update that runs out of disk space, or is killed, no longer leaves a damaged program.** Updating 0.9.1 on a full disk left 14 of the program's 15 MB in its place, not executable. The new program is written beside the old one and moved into its place, so an update needs about 15 MB free. (#130)
- **`--uninstall` removes the program where `~/.local/share` is a link to another place.** It left `~/.local/bin/any2odyssey` behind and reported success. (#130)
- **Updating an installation of 0.9.0 or 0.9.1 removes that version's setup files from the Wine side.** 0.9.2 left them there (`setup.sh`, `src`, `tests`), and that old `setup.sh`, run again, puts old files back and stops. (#130)
- An installation running at the same time no longer loses its staging folder to this one. One left by an installer that was killed is removed once it is a day old, and by `--uninstall`. (#125, #128)
- The packages' `README.md` comes with the picture at its top. (#128)
- Arch package: `any2odyssey-setup` says when an installation from the release archive in `~/.local` is still the one that is started, and how to remove it without losing the settings or the depth model. (#130)

Linux, the Wine side:

- The camera bridge refuses a secret that is too short once its line ending is taken off, instead of counting the line ending. (#126)

Windows:

- **Uninstalling no longer stops at files that are read-only.** A package unpacked with the attribute installed read-only files, and the uninstaller then failed with "You do not have sufficient access rights", in one order of events half-way: worker gone, program still registered, the same on every further try. The installer no longer copies the attribute and the uninstallers remove such files. (#131)
- `Install.cmd` and `Install-GUI.cmd` work from a folder with `!` in its name where delayed expansion is switched on in cmd; they answered "Extract the whole ZIP". (#131)

Both editions:

- A `settings.json` that is valid JSON but not an object (`[]`, for example) is reported as unreadable instead of being read as first-run defaults and written over. (#131)
- Recovering unreadable settings no longer leaves one more `settings-recovery-*.json` behind every time the settings file cannot be replaced. (#131)

### Tests and releases

- **The Linux viewer was run under a Wayland compositor for the first time** (sway and weston without a screen, software OpenGL). Start-up with and without XWayland, the application id both windows report, full screen on two outputs, unplugging an output, scales 1.5 and 2, file dialogs, 200 hand-overs and live capture without a screen-cast portal behaved as they should. `docs/PLATFORMS.md` says what was checked and what a Wayland window cannot do: the output window cannot place itself on a chosen monitor, and on desktops that draw no window frames (GNOME) it has no title bar. (#129)
- **Updates between the published releases were run for the first time**, as an ordinary user with settings, the depth model and a Wine side in place: nothing of the user's was lost in any direction, and after 434 kills of a running update every second run ended like a clean one. Interrupted, cut-short and wrong downloads of the depth model installed nothing. (#130)
- **The changes of 0.9.2 were reviewed by running counter-examples**: the refusal to install as root into a folder that is not root's alone, the camera secret's length check, the log's 4 MiB turnover under four writers and the readiness check against a log growing without end held. (#128)
- **The Windows installer was run with what its tests had always assumed away**: folder names with `&`, `'`, `%`, `!`, `^`, brackets, non-ANSI characters and emoji, UNC paths, a missing TEMP, damaged and half-written records, junctions, and files held open by another program (the previous installation came back whole). Only the two faults above were found. (#131)
- The check that every link in the packaged documents leads to a packaged file now also reads pictures and links written as HTML. (#128)
- CI: the checks of a pull request can no longer cancel the run on `main` when a commit is pushed moments before merging; this happened once during the 0.9.2 release. (#127)

### Hardware verification

Pending: these changes were made and tested without the monitor, Wine, the camera, the lens or a real Wayland desktop. `docs/TESTING.md`, "Pending hardware verification", lists the checks under "New in 0.9.3". Most in need of a real run: the viewer on KDE Plasma (Wayland) with the output window full screen over the controls, and an update of a real 0.9.0 or 0.9.1 installation whose Odyssey 3D setup had run.

## 0.9.2 (2026-11-13)

A second audit release, mostly for the Linux edition: eight pull requests (#116 to #123) from two further rounds that used methods not used before. The published 0.9.1 files were inspected, round 14's own fixes were reviewed again, the shared core was put through mutation testing, the running Linux viewer was soaked for thousands of operations, the tests were run under sanitizers and repeated under load, every script was run in hostile environments, and every Win32 call of the Windows edition was checked against its contract. No new features. Built and tested in CI; not yet run on the monitor, see Hardware verification.

**Linux: an installation of 0.9.0 or 0.9.1 made the usual way (`./install.sh`, under `~/.local`) never finished preparing the Odyssey 3D.** Update, then run `./install.sh` again and let it prepare the Odyssey 3D. Do not run the old installation's `--uninstall` first: it removes your settings and the depth model.

### Security

- **Linux: the installer does not install as root into a folder that is not root's alone.** Started as root with a user's home folder, it wrote by path into folders that user can change, and whoever can change a folder can redirect what is written there. It now stops before the first write and says to run it as the user it is for. Uninstalling is not refused. (#123)
- **Linux: the menu entry is never written through a link.** It is prepared in a private folder and moved into place, so an entry that is there is whole. (#120, #123)
- **Linux: nothing the installer writes can be written by others**, whatever the shell's umask is. With umask 000 the menu entry and the Wine setup were. (#120)

### Fixed

Linux, installing:

- **The usual installation put its setup files where the Wine side is built.** `setup.sh` stopped with "are the same file" before it linked the lens helper; installing again, even with `--no-odyssey`, removed the Wine prefix and the camera's secret; `--uninstall` removed settings, the depth model and the Wine side while saying they stay. Installations with `--prefix` were not affected, which is why no test saw it. (#120)
- **Arch package: its copy of the installer, run directly with `--uninstall`, took `/usr` for its own installation** and as root removed the package's files. It now behaves like `any2odyssey-setup --uninstall`. (#117)
- The packages' `README.md` comes with everything it links to (screenshots, `docs/TESTING.md`, `docs/PORTING.md`, `installer/README.md`; in the Arch package also the licence files beside it), and no longer with the Wine setup's test programs. (#116)
- `--uninstall` refreshes the desktop and icon caches, so the removed entry is no longer offered for image files. (#116)
- Installed libraries are no longer reported missing where `ldconfig` is not on the PATH (Debian). (#120)
- A staging folder left by an installer that was killed is removed by the next run. (#123)

Linux, the viewer:

- **The viewer no longer keeps hundreds of megabytes after large images are closed.** Each 8K-wide pair that was opened and replaced left about 200 MB behind: 120 MB grew to 1.7 GB over 1000 images. (#121)
- **Quitting while the Odyssey output is on no longer leaves a hung helper, or a file dialog's window, behind.** (#121)
- **Open... and Save preview PNG... work again after a file dialog hung or was lost**: pressing either closes the dialog that is still open and opens a new one. (#118)
- The Output panel stays inside the window with an Odyssey output selected. (#121)
- `--help` lists the modes of the platform it runs on, and Windows-only diagnostics say so on Linux; `--help` and error messages no longer end in a panic when their output is closed; `--help`, `--quit` and `--export-guide` no longer create the data folder. (#121)

Linux, the Wine side:

- Mirrored monitors no longer stop `mkedid.py`, and with it the start of the lens stack; the Odyssey 3D is chosen for the shared position. (#118)

Windows:

- **Apply or a tray mode command during the 8-second "Trying once more" wait no longer turns 3D off** with "Another renderer is already running"; the wait runs to its end. (#117)
- **A conversion is no longer stopped as "did not initialize" when Samsung's log was restarted with several megabytes ahead of its ready line.** (#118)

Both editions:

- **`companion.log` no longer grows for ever.** Once past 4 MiB it is kept as `companion.old.log` at the next start and a new log begins. (#118)

### Performance

- **Linux: showing a still on the Odyssey resizes both views at once**: 202 to 111 ms for a 3840x2160 pair, 291 to 200 ms for 7680x2160, with the same picture byte for byte. (#118)

### Tests and releases

- **The published 0.9.1 files were checked for the first time**: checksums, build provenance, versions, contents against the tag, hardening of the programs, and an installation and removal from the archive. Nothing was wrong with the Windows ZIP. A check that every link in the packaged documents leads to a packaged file now runs when the Linux packages are made. (#116)
- **Mutation testing of the shared core** (1,474 mutants) found no defect; about 140 gaps in its tests were closed, among them live frame scaling by rows, depth-map resizing, the readiness probe on an untouched log, the settings-recovery backup and the temporary-file sweep. (#119)
- **Sanitizers** (thread, address, memory) on the Rust and C tests reported nothing. Repeating the tests under load found two races in the tests of the file dialogs, which failed now and then on a busy machine. (#122)
- **A ledger of every Win32 and COM call site of the Windows edition** (149 entries: handle ownership, return values, buffers, lifetimes, threading) found no defect, and repeated calls left handle counts where they were. Nothing changed.
- CI: a newer push to `main` cancels the run of the commit it supersedes. (#116)

### Hardware verification

Pending: these changes were made and tested without the monitor, Wine, the camera or the lens. `docs/TESTING.md`, "Pending hardware verification", lists the checks under "New in 0.9.2". Most in need of a real run: the usual Linux installation from start to "setup: done", and the live screen's frame rate after the viewer's memory change.

## 0.9.1 (2026-11-12)

An audit release for both editions: 44 pull requests (#71 to #114) from a series of security, performance and correctness reviews of 0.9.0, the later rounds by fuzzing, measurement and a walkthrough across components. No new features. Built and tested in CI on Windows and Linux (x86_64 and aarch64) and in an Arch Linux container; almost none of it has been run on the monitor yet, see Hardware verification.

**Linux: run `setup.sh` (or `any2odyssey-setup`) again after updating.** The camera bridge and Leia's tracker now prove themselves to each other with a secret that setup creates; an old setup's bridge is refused.

### Security

- **Linux: the camera bridge and its client authenticate each other.** Any local program could connect to the bridge's loopback port and read the webcam, or stand in for the bridge towards Leia's tracker. Both sides now prove knowledge of a per-user secret (fresh random numbers per connection, constant-time comparison, one second for the whole exchange) before a picture or a command is accepted. (#96)
- **Linux: the instance socket accepts only the same user**, under a lock, with bounded messages and a time limit for a whole line; a file that is not the instance's own socket is no longer removed. (#75, #85, #95)
- **Linux: the installer no longer takes a Windows drive it was not given from removable or foreign media.** With `--yes` it ran `setup.sh` on the first mount that looked like Windows, a USB stick included. Only one internal drive owned by the user or root is used unasked; USB drives, memory cards, network shares and a user's FUSE mounts are named and passed over. (#92, #101)
- **Linux: the depth model is checked again when it is loaded**, and downloaded from a pinned revision; the screen-cast token is written privately and never through a link. (#71, #79, #95)
- **Linux: images are decoded with size limits, off the UI thread** (16384 pixels a side, 2^26 pixels). (#95)
- **Windows: "View logs" always opens the data folder.** The shell could run a program of the same name lying beside the folder instead. (#104)
- **Windows: named events are owned by the instance**, and files the controls read are capped at 1 MiB. (#93)
- **A malformed answer to the update check can no longer put the whole response into the About page and the log**; the error text is cut at 200 characters. (#112)
- **Releases carry build provenance.** Each published file has a Sigstore attestation that it was built by this repository's release workflow from the tag (`gh attestation verify`, see the README). The release workflow runs every test CI runs, pins the Arch container by digest, and refuses to sign or publish when the dependency policy (`cargo-deny`, new in CI) fails. (#92, #101, #108)

### Fixed

Linux, the live screen:

- **A still screen costs almost nothing.** The live loop rendered at full rate whether or not anything had changed; it now idles until the picture, the depth or a control changes. (#94, #102)
- **Helpers that hang are ended**, with their whole process group, three seconds after a stop; the picture file lives in the runtime folder (with a check for room) instead of `/tmp`, and is released on exit. (#94, #102)
- A capture that ended just as its first frame arrived stayed "running" with a frozen picture; choosing another output format during a live screen left stale rates on the page; a failing depth model now returns the output to a flat picture and recovers; queued model work for a stopped session is skipped. (#77, #90, #106, #107)
- **Starting 3D right after stopping it no longer leaves the picture without eye tracking, and stopping within a moment of starting no longer leaves Leia's tracker running.** (#110)

Linux, the viewer:

- **Odyssey output survives a pending depth estimate** and shows the page's messages; stale results of an earlier image are ignored; a worker thread that panics wakes the page with an error instead of leaving it waiting. (#72, #91, #95, #99)
- **PNG export** confirms a replacement after the extension is added, writes through a temporary file, reports a failed write, and keeps file names that are not UTF-8, as does handing a file to the running instance. (#76, #86, #87)
- Calibration edits are saved after a pause and on exit instead of at every step; lens output that ends by itself frees its panel-sized pictures; a file dialog left open ends with the viewer. (#88, #107, #109)

Linux, the Wine side:

- **Camera reader lifetimes.** A callback after a failed stream start, a use after free when shutdown timed out, a reader consuming a reused socket, an unanswered read after a flush, and driver buffers left allocated when a capture start failed early. (#83, #84, #97, #100, #103)
- The UDP shim no longer keeps completed receive buffers; the camera timeout and the model download no longer race; the weaver helper's watcher no longer spins where `flock` cannot wait (BusyBox); paired port keepers no longer hold each other; a malformed EDID's maker letters cannot break the registry file; an empty camera frame is dropped; the port keeper refuses an interval it cannot count. (#71, #73, #100, #113)
- The port keeper is a small native program that leaves with Leia's service, instead of a shell loop forking every 50 ms. (#96)
- `setup.sh` stops with a clear message when `pgrep` is BusyBox's, which made every lens-helper start restart Leia's service. (#109)

Linux, installing:

- Desktop launchers work for prefixes with spaces and other awkward characters; the uninstall command the installer prints removes the installation it was printed for, also after `--prefix` (it looked in `~/.local`); `--uninstall` stops the port keeper and leaves other Wine prefixes alone. (#80, #101, #110)
- The Linux archive and the Arch package no longer pick up Python cache files from the test run. (#108)

Windows:

- **A worker that panics says so and exits** (log line, status file, exit code 1) instead of dying silently with the overlay possibly left up. (#93)
- **The capture interval follows the display's refresh rate** instead of a fixed 16 ms that beat against it (57 pictures a second at 120 Hz). (#93)
- **Stops are quicker**, overlay raises no longer block the capture thread, and tray events wake the poller only when it acts on them. (#93, #98)
- **Controls started again within a few seconds of dying** found their old worker still holding the engine, turned 3D off and asked the user to close a renderer that had no window. The start is repeated once after 8 seconds. (#112)
- **The installer no longer stops with "Could not request a clean shutdown"** when Any2Odyssey had only just been started: `--quit` retries briefly, like `--toggle` and `--show`. (#109)
- The installation record is written the same way under PowerShell 7 as under Windows PowerShell; from PowerShell 7 on a Japanese, Chinese or Korean system with a non-ASCII profile folder the uninstaller could report it unreadable. (#105)
- Numbers in Samsung's `OverlaySettings.json` that Any2Odyssey does not set come back digit for digit; about one in thirteen was rewritten a unit in the last place off. (#112)

### Performance

- **Linux, live screen: the two views are rendered about 1.5 times faster and sharpened about 1.9 times faster**, with the same picture byte for byte. Measured on the processor alone (Ryzen 9950X3D under WSL 2, 4K screen, sharpening on): 8.9 to 6.7 ms a picture at 1080 lines, 15.3 to 11.8 ms at 1440, 32.4 to 23.5 ms at 2160 (about 55 pictures a second instead of 44; still short of 60). (#114)
- **Linux, live screen: fewer copies of each frame**, native-size frames taken without a resize. (#71, #94)
- **Linux, still images:** stereo rendering streams rows instead of holding whole intermediate images, flat stills are resized once, side-by-side conversion and frame preparation copy rows in bulk, and half-width pairs, eye fitting and the lens interleave run on several threads. (#74, #78, #81, #82, #89, #99)

### Changed

- **Docs: the pointer on the Odyssey's own screen.** 0.9.0's notes list the desktop's real pointer looking doubled over the 3D picture as a known issue. It looked right by the end of the same session on the G90XF, after the eye tracking's timing had been corrected; `docs/LENS-SWITCH.md` and `docs/PLATFORMS.md` now say so. Nothing in the program changed.

### Hardware verification

Pending for nearly everything in this release: the changes were made and tested without the monitor, Wine, the camera or the lens. `docs/TESTING.md`, "Pending hardware verification", lists the checks for the Linux installer, the Wine side, the live screen, the viewer and Windows; each pull request names its own. Most in need of a run: the camera bridge's authentication after a fresh `setup.sh`; the UDP shim under the tracker's pause and resume (#73); stop and start of eye tracking in quick succession (#110, whose timings for Leia's service are assumed); the live screen's idling and its rates at 1080, 1440 and 2160 lines; on Windows, a steady 60 pictures a second at 120 Hz in `desktop-capture-status.json`, and a relaunch right after the controls are killed. The first tag is also the first run of the attestation job, the dependency job and the Linux gates in the release workflow.

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

Partly done for Linux. Otherwise run only on Arch Linux under WSL 2 (X11, virtual display, Openbox, software OpenGL; AI depth on the CPU). The lens interleave was checked there only as pixels (the full-screen pattern matches the expected subpixel assignment exactly). The lens switch through Wine was run on a G90XF (CachyOS, x86_64, KDE Wayland, NVIDIA, Wine 11.21, SR runtime 1.36.4): from the viewer, full-screen lens output switched the lens on within about half a minute of a cold start and at once afterwards, and leaving full screen switched it off; the interleaved picture looked reasonable after tuning by hand, with display scaling at 125 %. The helper for Leia's Linux runtime has been neither compiled against that SDK nor run, because the runtime could not be obtained. Leia's eye tracker, the weaver helper and the tracker's pause were run on that machine as recorded in `docs/LENS-SWITCH.md`, and AI depth ran on its GPU (RTX 5090, `onnxruntime-cuda` 1.29). On the same machine the Odyssey 3D output was seen from the viewer on the panel: a still image with AI depth (upright, depth the right way round, following the viewer), and the live screen with the Odyssey's own desktop as the screen on KDE Plasma 6.8, including video playing. Known there: the desktop's real pointer is drawn flat over the 3D picture and looked doubled through the lens (it looked right later in the same session; see 0.9.1 above); some flicker while the head moves remained after the camera's time stamps were corrected (which reduced it); the repeated stop and start and the 2160-line detail were fixed afterwards and checked by measurement only. Also to check: the anaglyph and side-by-side results viewed with glasses or on a side-by-side display. To check on a physical Arch-based desktop: Wayland and X11 sessions, the Open and Save dialogs with zenity and with kdialog, drag and drop, the output window made full screen on a second display, an image opened from the file manager's "Open with".

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
