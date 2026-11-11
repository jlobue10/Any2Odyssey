# Switching the Odyssey's lens on from Linux

The lens-interleaved output of the Linux stereo viewer only separates the two views while the G90XF's lens is switched on. This page records how the lens is switched, what Any2Odyssey does about it, the two helpers that can do the asking (one of them works today), and what is still missing.

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

## The helper that works today: Leia's Windows runtime under Wine

If the monitor is also used from Windows on the same machine, the Odyssey 3D Hub has installed Leia's SR runtime there. `contrib/wine-lens/setup.sh` copies that installation into a Wine prefix of its own and makes Leia's unmodified `SRService` and `SRSession` run in it. The helper then asks that service for the lens exactly as a Windows program would. Leia's service does all the talking to the lens board, including its authentication; Any2Odyssey and the helper send the board nothing, and nothing of Leia's is distributed here.

What is needed: Wine (11.x was used), `mingw-w64-gcc`, Python 3, `xrandr` (the service runs through XWayland or X11), `rsync`, and the Windows system drive mounted read-only.

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

Limits: only the lens is switched. Leia's eye tracker and weaver are not used, so the picture is the viewer's own two-view interleave for one head position. The stand-ins say "monitor on" and "session unlocked" unconditionally; the lens still goes off when the viewer leaves full screen, exits or crashes, and with the monitor's own standby.

## The other helper: Leia's runtime for Linux

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
- **Not checked:** the helper for Leia's Linux runtime. It was written from the public description of the srSDK 1.0.0 API and has been neither compiled against the SDK nor run with a runtime, because the runtime could not be obtained. Whether `leiasr-runtime` runs on Arch-based distributions, and whether the G90XF is among the displays it accepts on Linux, is unknown. Also not checked: the Wine helper on other distributions, Wine versions, X11 sessions or GPUs.
