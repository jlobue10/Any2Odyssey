# The Odyssey 3D's lens, eye tracker and weaver through Wine

Switches the Samsung Odyssey 3D (G90XF) lens on from Linux by running your own copy of Leia's Windows SR runtime
under Wine, runs Leia's eye tracker there too, with the monitor's webcam handed to it by a small native
bridge, and shows Any2Odyssey's pictures through Leia's weaver. `docs/LENS-SWITCH.md` explains how it works, what it needs and what has been checked.

**Needs a dual-boot PC** (or another way to reach a Windows installation): Windows with the Odyssey 3D Hub, which has run this monitor in 3D at least once, on a drive Linux can mount read-only. A BitLocker-encrypted drive has to be unlocked first. The drive is needed only while `setup.sh` runs. `docs/LENS-SWITCH.md` has the details.

```sh
sudo cp 70-any2odyssey-lens.rules /etc/udev/rules.d/ && sudo udevadm control --reload && sudo udevadm trigger
./setup.sh /path/to/mounted/windows-drive      # the drive that has the Odyssey 3D Hub installed
sleep 20 | ~/.local/share/any2odyssey/any2odyssey-lens-helper     # lens on for 20 s
~/.local/share/any2odyssey/wine-lens/tools/tracking.sh start      # eye tracking, while the helper runs; then: stop
```

- `setup.sh`: makes the Wine prefix, copies Leia's runtime into it, builds and installs the stand-ins.
- `any2odyssey-lens-helper`: what Any2Odyssey starts; brings Leia's service up when needed and asks it for the lens.
- `any2odyssey-weaver`: what Any2Odyssey starts for its Odyssey 3D output; brings the service and eye tracking
  up and runs the weaver host, which shows the pictures Any2Odyssey writes into a shared file (set up when
  `clang++` is installed).
- `tools/tracking.sh`: starts and stops the camera bridge and Leia's eye tracker (set up when the Windows drive
  has the tracker and this machine has a C compiler).
- `src/`: the stand-ins for what Wine lacks, the read-only USB information driver, the Windows helper, the
  Media Foundation camera stand-in (`a2ocam.c`), the native camera bridge (`camera-bridge.c`), the weaver host
  (`weaver-host.c`, `srweaver.cpp`) and what lets the tracker pause (`udp-errors.c`, `connreset.c`).
- `tools/`: describe the board and the monitors to Wine, start the service, keep the serial port binary-clean.
- `70-any2odyssey-lens.rules`: the lens board for the person at the seat, and USB power saving off for the
  webcam (it stops sending pictures once it has been suspended, until the monitor is power-cycled).

Everything here is MIT-licensed (`../LICENSE-MIT`) and contains nothing of Leia's or Samsung's. Nothing in it talks to the lens
board: Leia's own service does, exactly as on Windows. The camera bridge listens on the loopback interface only
and gives pictures only to a program that knows a secret kept in a file only you can read.
