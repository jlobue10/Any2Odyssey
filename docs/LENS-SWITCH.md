# The Odyssey's lens, eye tracker and weaver on Linux

A glasses-free picture on the G90XF needs three things of Leia's: the lens switched on, the viewer's eye positions, and the weaver that interleaves the two views for the lens with the panel's calibration. This page records how each is reached from Linux, what Any2Odyssey does, what has been checked and what is still missing. The first sections are about the lens alone (enough for the viewer's own interleave); the later ones about eye tracking and Leia's weaver.

## How the lens is switched

- The monitor has a lens board on its internal USB hub (`354b:0116`, "Dimenco BV SR Lens Switch Board", a USB serial device). It is powered with the monitor: it leaves the bus when the monitor goes to standby and returns in the off state. **The lens state therefore does not survive a restart**; switching it on in Windows and restarting into Linux leaves it off.
- The board only obeys Leia's SR runtime (`SRService`), which authenticates to it. Any2Odyssey does not talk to the board and does not imitate that conversation.
- A program asks the SR runtime for the lens through Leia's SDK: `SwitchableLensHint` on Windows, `srCreateLens` / `srLensEnable` / `srLensDisable` in the Linux srSDK. It is a preference held per connected client: the runtime keeps the lens on while any client asks for it and switches it off when the last one leaves.
- Leia's Linux runtime is the `leiasr-runtime` package (Debian/Ubuntu, installed under `/opt/leiasr`, headers and loader included). It is commercial, is not public at the time of writing, and is not part of Any2Odyssey. The public description of its API used here is the [DisplayXR Leia plug-in](https://github.com/DisplayXR/displayxr-leia-plugin) (`docs/display-mode-switching.md`, `docs/leia-linux-sdk-contract.md`).

## What Any2Odyssey does

The viewer asks for the lens exactly while **lens output is full screen** (the picture or the calibration pattern), and withdraws the request when the output leaves full screen, changes format, or closes. The line under the lens controls says what happened: off, on, no helper installed, or the reason it failed.

The asking is done by a separate small program, `any2odyssey-lens-helper`, so that the GPL-3.0-only program contains nothing of Leia's SDK and does not link it:

- There are two helpers, described below: one for Leia's Windows runtime under Wine (`contrib/wine-lens/`, works on the G90XF) and one for Leia's Linux runtime (`contrib/lens-helper/`, untested). Both are MIT; no package of Any2Odyssey contains either.
- The viewer looks for it in `$ANY2ODYSSEY_LENS_HELPER` (a full path), then beside its own executable, then in its data folder (`~/.local/share/any2odyssey/`, where the Wine setup puts it), then in `/usr/lib/any2odyssey/` and `/usr/local/lib/any2odyssey/`. Without one the lens stays off and everything else works as before.
- The viewer starts it and holds its standard input open. The helper asks the runtime for the lens and prints `on`, or prints `error: <reason>` and exits. When its input closes (the viewer stopped asking, quit or crashed) or it receives SIGTERM, it withdraws the request and exits.

## Leia's Windows runtime under Wine

**A Windows installation is required for the Odyssey 3D part.** Leia's runtime is not distributed with Any2Odyssey and has no public Linux version, so the lens, the eye tracker and the weaver come from your own Windows installation: the expected set-up is a dual-boot PC, with Windows and the Odyssey 3D Hub installed on a drive of the same machine that Linux can mount. What that Windows installation has to be:

- **Windows with the Odyssey 3D Hub installed** (it installs Leia's SR runtime under `Program Files\LeiaSR` and keeps the monitor's data under `ProgramData\Simulated Reality`). Hub 1.5.1 with SR runtime 1.36.4 is what was used.
- **Used with this monitor at least once**, with 3D working: the runtime fetches the monitor's own calibration and tracker data when it first sees it and keeps them under `ProgramData`, by the monitor's serial number. A Windows installation that has never run this monitor has nothing for it.
- **On a drive Linux can read.** The Windows system partition (NTFS) is mounted read-only; nothing is written to it. A partition encrypted with BitLocker cannot be read from Linux: suspend or turn off BitLocker for it in Windows, or unlock it with a tool such as `dislocker` first. Windows left hibernated or with Fast Startup on can still be mounted read-only.
- **Only at set-up time.** `setup.sh` copies what it needs (about 1 GB) into your data folder; afterwards the drive need not be mounted, and Windows need not be started again. Run `setup.sh` again after the Hub has updated Leia's runtime in Windows, if you want the newer one, and after replacing the monitor.

A Windows installation in a virtual machine, or the files copied over from another PC, would do as well if they meet the first two points; neither has been tried. Without any Windows installation the Odyssey 3D output and the lens switch are not available, and the rest of the viewer works as before.

To find and mount the Windows partition: `lsblk -f` lists the partitions with their file systems (the Windows system partition is the NTFS one that holds `Program Files`); mount it from the file manager, or with `udisksctl mount -b /dev/<partition> -o ro`, which prints where it is mounted (usually under `/run/media/<you>/`). That folder is what `setup.sh` and the installer's `--windows-root` take.

The Odyssey 3D Hub has installed Leia's SR runtime there. `contrib/wine-lens/setup.sh` copies that installation into a Wine prefix of its own and makes Leia's unmodified `SRService` and `SRSession` run in it. The helper then asks that service for the lens exactly as a Windows program would. Leia's service does all the talking to the lens board, including its authentication; Any2Odyssey and the helper send the board nothing, and nothing of Leia's is distributed here.

What is needed: Wine (11.x was used), `mingw-w64-gcc`, Python 3, `xrandr` (the service runs through XWayland or X11), `rsync`, and the Windows system drive mounted read-only. A native C compiler (`cc`) is needed as well for the eye tracker's camera bridge, and `clang++` for the weaver host; without them only the lens switch is set up. On KDE Plasma, `busctl` (part of systemd) is used to load the script that lets the Odyssey show its own screen live.

The Linux installer (`install.sh` in the release archive, `any2odyssey-setup` with the Arch package) does all of this when asked. By hand, the commands below are for a checkout; in the Arch package the same folder is `/usr/share/any2odyssey/wine-lens`, in the Linux archive `wine-lens`.

```sh
sudo cp contrib/wine-lens/70-any2odyssey-lens.rules /etc/udev/rules.d/     # the board as /dev/odyssey-lens, usable from the seat
sudo udevadm control --reload && sudo udevadm trigger
contrib/wine-lens/setup.sh /path/to/mounted/windows-drive
```

After that the viewer finds the helper by itself. The first request after a login or after the monitor has slept takes about half a minute, because the service has to start and find the board again; later requests take effect at once. To try it without the viewer: `sleep 20 | ~/.local/share/any2odyssey/any2odyssey-lens-helper`. Run `setup.sh` again after a Wine upgrade.

Leia's service expects a number of things from Windows that Wine does not provide yet. `setup.sh` builds a small stand-in for each (sources in `contrib/wine-lens/src`), and these may be of use to others running services under Wine:

| What the service asks | What Wine does | Stand-in |
| --- | --- | --- |
| A toast notifier at start-up (`SRSession`) | class not implemented | `a2otoast.dll`: classes that accept every call and show nothing |
| The monitors' EDIDs | leaves them empty under XWayland | `mkedid.py`: writes the real EDIDs, paired with Wine's monitors by desktop position, at every start |
| The lens board as a USB device with a serial-port function, and walking between the two | no USB device tree; `CM_Get_Child` is a stub | registry entries from `mkboard.py` and a `cfgmgr32.dll` that links parent and child |
| The board's USB descriptors, read through the hub | no hub devices | `a2ousb.sys`: a read-only driver answering with the descriptors Linux shows in sysfs |
| The board's device status | reports success and fills in nothing | `cfgmgr32.dll`: reports a started device |
| "The monitor is on" (`RegisterPowerSettingNotification` for a service) | stub that never notifies | `cfgmgr32.dll`: delivers the one notification |
| The number of physical monitors behind a display handle | failing stub | `dxva2.dll`: one each |
| Whether the console session is active and unlocked (`WTSSessionInfoEx`) | not implemented, so the service keeps the lens off | `wtsapi32.dll`: active and unlocked |
| Binary data on the serial port | switches XON/XOFF handling on in the tty | `noxon.sh`: keeps it off while the service runs |
| The webcam as a Windows camera device (eye tracker) | no USB device tree | registry entries (`camera.reg`), including the device classes: Wine reports an error for a device whose class has no entry |
| The webcam's pictures and controls through Media Foundation (eye tracker) | no Media Foundation video capture | `a2ocam.dll` behind forwarding `mf.dll` / `mfreadwrite.dll`, fed by the native `any2odyssey-camera-bridge` (V4L2) over a loopback port |

Limits of the lens helper: it switches the lens and nothing else, which is what the viewer's own interleave needs (a picture for one head position). The Odyssey 3D output uses the weaver helper described below instead. The stand-ins say "monitor on" and "session unlocked" unconditionally; the lens still goes off when the viewer leaves full screen, exits or crashes, and with the monitor's own standby.

## Eye tracking under Wine

On Windows a second program of Leia's, the eye tracker (`LeiaSR/Tracker`, service "SR Eye Tracker"), watches the viewer through the monitor's webcam and gives Leia's service the eye positions that Leia's weaver steers the picture with. When the Windows drive has it, `setup.sh` copies it into the prefix as well and builds what it needs:

- The webcam is the monitor's "3D Stereo WebCam" (`04e8:20d7`, a side-by-side infrared stereo camera, ordinary UVC). `any2odyssey-camera-bridge`, a native program, owns it through V4L2 and hands frames and controls to the Wine side over a loopback TCP port, protected by a secret in a file only its owner can read. `a2ocam.dll` presents that to the tracker as a Media Foundation camera.
- The frames are stamped with the system's running clock at the moment they arrive, as Windows stamps them. Stamped from zero instead, the tracker follows a face for about two seconds and then falls back to its fixed 2D position. Stamped with the kernel's own capture time, which marks the start of a frame and is a frame time (16 ms) earlier, the tracker takes every picture for 16 ms older than it does on Windows and predicts the eyes that much too far ahead.
- **The webcam must not be put into USB power saving.** After a suspend it still negotiates a stream but sends no pictures, and only power-cycling the monitor brings it back. The udev rule file switches power saving off for it, both when it appears and again when the camera driver attaches (the driver switches it back on at that moment).
- `~/.local/share/any2odyssey/wine-lens/tools/tracking.sh start` starts the bridge and the tracker (Leia's service must be running), `stop` stops them, `status` reports. The weaver helper below does both by itself.
- **Pausing.** On Windows the tracker pauses (camera off, no processor use) when no program wants eye positions. It learns that from a bounced datagram: the service closes its UDP port, the tracker's next datagram to it is refused, and Windows fails the tracker's waiting receive with the address of that port. Linux reports a refused datagram on such a socket only when asked, and Wine neither asks nor fills in the address, so under Wine the tracker ran on at a full processor core. Two small pieces restore the Windows behaviour: `any2odyssey-udp.so`, preloaded into the prefix's Wine processes, asks Linux for those reports (`IP_RECVERR`), and `a2ocam.dll` puts the address in place inside the tracker. With them the tracker pauses and resumes as on Windows.

  The address shim fills only receive buffers that have not yet been submitted to Winsock. It never retains their pointers: a pending operation may complete and its caller may reuse the storage at any time. If a receive was submitted before the first destination was known, the address is available on the next receive; pause/resume timing for that case must be rechecked on the target Wine/Leia installation. The earlier hardware measurements below describe the previous implementation, not verification of this lifetime correction.

The tracker reports the eye positions to Leia's service, where a program using Leia's SDK gets them; Leia's weaver is such a program.

## The picture through Leia's weaver

The viewer's output format "Odyssey 3D" does not interleave anything itself. It starts `any2odyssey-weaver` (next to the lens helper in `~/.local/share/any2odyssey/`), which makes sure Leia's service is running, starts eye tracking and runs a small Windows program, the weaver host (`src/weaver-host.c`), under Wine:

- The host opens a full-screen window on the Odyssey 3D (found by its EDID name among the monitors when the service starts), asks for the lens, and creates Leia's OpenGL weaver for that window. The weaver is in `SimulatedRealityOpenGL.dll` of your copy of the runtime; the host finds it at run time and links nothing of Leia's. Its interface is a C++ class in the Microsoft layout, so one small file (`src/srweaver.cpp`) is compiled with clang for that layout; this is why `setup.sh` wants `clang++` for this part.
- Pictures cross from the native program in a file in the session's runtime folder (memory, readable by the user only): a small header and RGBA pixels, the left view in the left half and the right view in the right half. The native side writes it; the host has it mapped and gives the weaver each new picture. Each half is shown over the whole panel, so the viewer fits the picture into a 16:9 half before writing it.
- The helper speaks through its standard streams like the lens helper: `on` once a picture is showing, `eyes 1` / `eyes 0` as the tracker finds or loses the viewer, `error: <reason>`; it leaves, lens off and tracking stopped, when its input closes or Esc is pressed in its window.
- One more gap in Wine is covered inside the host: Wine lists a monitor it has lost track of with a null handle, and Leia's display code gives up on the whole list when it meets one. The host leaves those entries out of what Leia's libraries see in its own process.

The weaver needs eye positions to show two views. Without a viewer in the tracker's range (about 50 to 130 cm, in front of the screen) it shows the left view only, a 2D picture.

### A live screen

The input format "Live screen → 3D" feeds the same output continuously (`crates/app/src/capture.rs`, `live.rs`, `crates/core/src/live.rs`). The desktop's screen-cast portal asks which screen to share and hands out a PipeWire stream of it; each frame is shrunk to one view's size, shifted into a left and a right view by the depth model's latest map (the model runs on its own thread, as fast as it can; the picture does not wait for it) and written to the weaver's file. The host's window lets the pointer through in this mode.

The screen can be the Odyssey's own on KDE Plasma 6: while a live screen is shown, `any2odyssey-weaver` loads a small KWin script (`tools/kwin-overlay.js`) that asks KWin to leave the host's window out of screen casts and to keep it above the other windows, so the capture sees the desktop underneath and not the 3D picture of it. The helper reports `capture: excluded` and the viewer then says the Odyssey itself can be chosen. Other desktops have no such switch that this project knows of; there the screen has to be another one. On the Odyssey's own screen the real pointer is drawn by the desktop on top of the 3D picture, as a flat pointer, so the viewer's "Draw the pointer into the picture" should be off.

## The other lens helper: Leia's runtime for Linux

`contrib/lens-helper/` is one C file and a CMake file, built by whoever has Leia's SDK for Linux. With `leiasr-runtime` installed (or `SRSDK_ROOT` pointing at an unpacked SDK):

```sh
cmake -S contrib/lens-helper -B build/lens-helper
cmake --build build/lens-helper
sudo cmake --install build/lens-helper        # /usr/local/lib/any2odyssey/any2odyssey-lens-helper
```

To try it alone, run it in a terminal: it prints `on`, keeps the lens on, and releases it on Ctrl+D.

## What has been checked

- The viewer's side: unit tests drive stand-in helpers through start, `on`, release by closing the input, release when the viewer's state is dropped, a reported error, a silent exit and a missing program (CachyOS, x86_64).
- The Wine helper, on a G90XF (CachyOS, x86_64, KDE Wayland, NVIDIA, Wine 11.21, SR runtime 1.36.4 copied from Windows 11 on the same machine): `setup.sh` on an empty prefix; from the viewer, full-screen lens output switched the lens on (about 28 s cold, under 0.2 s warm) and leaving full screen switched it off, confirmed by eye and in the service's log. On Windows the same request, with nothing else running, switches the lens with no eye tracking involved.
- Eye tracking under Wine, same machine: `setup.sh` on an empty prefix, then the lens helper and `tracking.sh start`; a test program using Leia's SDK received a steady pair of eye positions about 65 mm apart for as long as it ran (about 240 a second), and they followed the viewer; with Leia's own weaver (OpenGL) in that test program showing a red picture to the left eye and a cyan one to the right, the person watching reported that it looked right. That test program is not part of this repository.
- The weaver helper, same machine: `any2odyssey-weaver` from a cold start (service, tracking, weaver host, about 30 s) showed pictures written to its file by a test feeder and reported `on` and `eyes 1`, in both the ordinary and the pointer-through window mode.
- The tracker's pause, same machine: with `any2odyssey-udp.so` alone the tracker noticed the closed port but removed the address 0.0.0.0:0 and did not pause; with the address put in place by `a2ocam.dll` it logged the same lines as on Windows when the weaver host left (the listener removed by its address, "in pause mode now", the camera released), fell from a full processor core to about 1 % of one, and resumed for the next start.
- From the viewer, on the panel, same machine: a still image with AI depth through the Odyssey 3D output (upright, depth the right way round, following the viewer), and the live screen with the Odyssey's own desktop as the screen (KDE Plasma 6.8), including video playing. Known there: the desktop's real pointer, drawn flat over the 3D picture, looked doubled through the lens early in that session and looked right by its end, after the eye tracking's timing had been corrected (nothing was changed for the pointer itself); some flicker while the head moves remained after the camera frames' time stamps were corrected, which reduced it. Starting again after a stop, and the 2160-line detail, were fixed after that session and have been checked by measurement only. The stages after the capture were timed on that machine: shrinking and rendering a 3840 × 2160 screen to 1080-line views takes about 3 ms a picture; a depth map takes about 75 ms on the processor and under 5 ms on the GPU (RTX 5090, CUDA provider of `onnxruntime-cuda` 1.29). That package's provider library does not name cuDNN among the libraries it needs and fails to load by itself; Any2Odyssey opens cuDNN first, which is all it lacks. Run without the viewer's page against the Odyssey's own screen, the whole path gave 62 pictures and 62 depth maps a second (it is paced at 60) with about three processor cores; every frame of the screen cast was the bare desktop, the 3D picture's window being left out by KWin.
- **Not checked:** the helper for Leia's Linux runtime. It was written from the public description of the srSDK 1.0.0 API and has been neither compiled against the SDK nor run with a runtime, because the runtime could not be obtained. Whether `leiasr-runtime` runs on Arch-based distributions, and whether the G90XF is among the displays it accepts on Linux, is unknown. Also not checked: the Wine helper and the eye tracker on other distributions, Wine versions, X11 sessions or GPUs; the eye tracker over long sessions, with several faces in view, or across a monitor standby.
