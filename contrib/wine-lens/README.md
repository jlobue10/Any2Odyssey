# The Odyssey 3D's lens switch through Wine

Switches the Samsung Odyssey 3D (G90XF) lens on from Linux by running your own copy of Leia's Windows SR runtime
under Wine. `docs/LENS-SWITCH.md` explains how it works, what it needs and what has been checked.

```sh
sudo cp 70-any2odyssey-lens.rules /etc/udev/rules.d/ && sudo udevadm control --reload && sudo udevadm trigger
./setup.sh /path/to/mounted/windows-drive      # the drive that has the Odyssey 3D Hub installed
sleep 20 | ~/.local/share/any2odyssey/any2odyssey-lens-helper     # lens on for 20 s
```

- `setup.sh`: makes the Wine prefix, copies Leia's runtime into it, builds and installs the stand-ins.
- `any2odyssey-lens-helper`: what Any2Odyssey starts; brings Leia's service up when needed and asks it for the lens.
- `src/`: the stand-ins for what Wine lacks, the read-only USB information driver, and the Windows helper.
- `tools/`: describe the board and the monitors to Wine, start the service, keep the serial port binary-clean.

Everything here is MIT-licensed and contains nothing of Leia's or Samsung's. Nothing in it talks to the lens
board: Leia's own service does, exactly as on Windows.
