# Any2Odyssey Windows installer

PowerShell installer family, ported from Any2Odd 0.7.3's installer (GPL-3.0-only) to Any2Odyssey's own identity. It never shares files, folders, registry keys or process names with an installed Any2Odd.

| Item | Value |
| --- | --- |
| Application folder | `%LOCALAPPDATA%\Programs\Any2Odyssey` |
| Data folder | `%LOCALAPPDATA%\Any2Odyssey` |
| Worker beside Samsung's engine | `C:\Program Files\Odyssey 3D Hub\Conversion\Any2OdysseyWorker.exe` |
| Installed-apps key | `HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\Any2Odyssey` |
| Shortcuts | `Any2Odyssey.lnk` on the Desktop and in the Start menu |

## Files

- `Install.cmd` / `Install-GUI.cmd`: double-click launchers (full install with elevation, or controls only without elevation). Arguments are passed on to the launcher, so `.\Install.cmd -AllowDowngrade` from a terminal installs an older release over a newer one. From a 32-bit host (a 32-bit file manager or console) they start the 64-bit PowerShell through `Sysnative`, since `install.ps1` refuses a 32-bit process.
- `Install-Launcher.ps1`: preflight before UAC, same-account check, relays the elevated result to the original console. `-ControlsOnly` and `-AllowDowngrade` reach `install.ps1` on the direct and on the elevated path.
- `install.ps1`: the installer. `-CheckOnly` (read-only), `-ControlsOnly`, `-AllowDowngrade`.
- `InstallSupport.ps1`: version detection, install action (Install/Upgrade/Repair/Downgrade) and the file transaction with rollback. The installed version is the `FileVersionInfo` of the installed executable (`version.json` if that resource is unreadable); without an executable, the Installed-apps registration and then the records. A record on its own (`install-result.json` survives an uninstall on purpose) is not an installation: installing after an uninstall is a fresh Install, whichever release was installed before.
- `UninstallWorker.ps1`: installed under `%ProgramFiles%\Any2Odyssey`; the only elevated uninstall step, removes the fixed worker path, itself and its owner marker, and checks the approving account. Its window closes with it, so when it fails it writes the reason to `UninstallWorker.result` beside itself, and `Uninstall.ps1` shows that reason in the user's own console (ignoring a note older than the run it just started). That is the only file the helper writes: a fixed name in its own Program Files folder, where only administrators can write, never a path supplied by the caller, and the helper reads nothing from it. The installer writes `UninstallWorker.owner` (the installing account's SID) beside it. A controls-only uninstall never runs the helper: a helper present beside a controls-only record belongs to another account's full installation or to an older release, and `Install.cmd` followed by a full uninstall from that account (or an administrator by hand) removes it. A full uninstall whose marker names another account (someone else ran `Install.cmd` later on a shared PC) also leaves worker and helper alone and says so; that later install prints a notice that it is replacing them. Because the worker is one file for every account, `install.ps1` reads its version before replacing it and refuses a package that is older (naming the other account when the marker does), unless `-AllowDowngrade` is passed; the per-user downgrade guard cannot see this case, since for a second account an older package is a plain Install.
- `Uninstall.ps1`: runs as the normal user, signals the quit event without launching the installed EXE, and removes exactly the files the installer placed; Samsung files, settings and logs stay. Running instances are judged per Windows session: the installer and uninstaller ask and wait for the instance of their own session (the quit event is `Local\`, so it reaches no other), an instance in another signed-in account's session does not block per-user work, and anything that replaces or removes the shared worker (`Install.cmd`, the uninstall helper) refuses while any session runs Any2Odyssey and names that session. The registered uninstall command adds `-Pause`, which keeps the console open until a key is pressed when something fails and when a completed uninstall printed a notice (an unreadable record, a worker left in place for another account); without the switch (the README command, the tests) errors propagate as before.
- `tests/`: isolated tests that run on Windows only (temporary folders, isolated registry key, simulated engine hash and elevation; `test-uninstall-helper-elevated.ps1` runs the real helper as an administrator against routed copies and is skipped in a console that is not elevated). They need `build\Any2Odyssey.exe` in the repository root; see `docs/TESTING.md`.

## Accounts

The application folder, shortcuts, settings and Installed-apps entry are per user; only the worker and its uninstall helper are per PC. A full installation therefore has to be approved by the same account that started it, and that account has to be an administrator:

- From a standard account the elevation prompt asks for an administrator's credentials, and the elevated step then runs as that administrator. `Install-Launcher.ps1` passes the starting account's SID to the elevated step, which refuses when it runs as someone else, so nothing is installed into the administrator's profile by accident.
- **Run as administrator** on `Install.cmd` skips that comparison, because the whole launcher then runs as the administrator account. `install.ps1` prints the target account and application folder in its first lines, and the launcher adds a sentence when it finds itself already elevated.
- `Install-GUI.cmd` needs no administrator rights and works from a standard account, but `install.ps1 -ControlsOnly` refuses while a worker or its helper exists on the PC. On a PC with a full installation, only administrator accounts can install Any2Odyssey.

## Where the launchers cannot help

`Install.cmd` and `Install-GUI.cmd` start Windows PowerShell with `-ExecutionPolicy Bypass`, which covers the usual case of scripts being disabled for the user or the machine. Three situations are outside their reach:

- **Run from inside the ZIP.** Double-clicking `Install.cmd` in Explorer's ZIP preview extracts only that file. It detects that the rest of the package is missing and says to extract the whole ZIP first.
- **Execution policy set by Group Policy.** A `MachinePolicy` or `UserPolicy` execution policy (check with `Get-ExecutionPolicy -List`) overrides `-ExecutionPolicy Bypass`. If it is `Restricted` or `AllSigned`, PowerShell refuses the scripts ("running scripts is disabled on this system" or "is not digitally signed"); the package's scripts are not signed. Only the administrator of that policy can allow them.
- **Constrained Language Mode** (AppLocker or App Control for Business policies). The scripts use .NET types and COM objects (the account check, the version resource, shortcuts), which that mode blocks; setup then stops with PowerShell's own error, such as "Cannot invoke method. Method invocation is supported only on core types in this language mode". Any2Odyssey cannot be installed with these scripts on such a PC.

## Trust model

`Install.cmd` elevates `Install-Launcher.ps1` from the folder the ZIP was extracted to. That elevated process runs `install.ps1` and `InstallSupport.ps1` from the same folder, runs `build\Any2Odyssey.exe --quit` from it when an instance is still open, copies `build\Any2Odyssey.exe` and `UninstallWorker.ps1` from it into Program Files, and stages rollback copies of the files it replaces under `%TEMP%`, restoring them from there if the installation fails. Those copies are deleted when the installation completes and when a failed one has been rolled back completely; they are kept, and their folder named, only when a file could not be put back. Everything the elevated step reads is therefore under the control of the user who extracted the package, exactly as with any setup program run from a Downloads folder: extract to a folder you control, verify the ZIP against the published `.sha256` first, and approve elevation only for a package you trust. The same-account check in the launcher prevents installing into the wrong user profile; it is not what protects the elevated step. The uninstall side is deliberately different: its only elevated step, `UninstallWorker.ps1`, runs from Program Files and never from `%LOCALAPPDATA%` or a temporary folder. It reads no file, environment variable or registry value that the unelevated user controls; its one argument is the SID it compares with its own; it deletes only fixed paths and writes only its result note beside itself.

## Package layout expected

```
VERSION
build\Any2Odyssey.exe
assets\Any2Odyssey.ico
USER-GUIDE.html
LICENSE
NOTICE
THIRD-PARTY-LICENSES.md
Install.cmd  Install-GUI.cmd  Install-Launcher.ps1  install.ps1  InstallSupport.ps1  Uninstall.ps1  UninstallWorker.ps1
```

`scripts/package.ps1` builds this layout into `dist/Any2Odyssey-<version>-windows-x64.zip`. The ZIP also carries what the installer does not need: `README.md`, `CHANGELOG.md`, `docs\`, the PNG assets and this file as `installer\README.md`, the path `README.md` links to. The script stops if any relative link in the packaged `README.md` has no target in the package.

## Differences from the Any2Odd installer

- Single native executable: no SharpDX dependency DLLs, no `dependencies.json`, no shared-folder ownership tracking, no `restore.ps1`.
- The executable's version is read from its Windows version resource. The Rust build must embed `FileVersion` = `VERSION` + `.0` (see the app crate's build script).
- Any2Odd's clean-shutdown contract is kept in spirit: `Any2Odyssey.exe --quit` signals the running instance through its own named event.
