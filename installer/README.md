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

- `Install.cmd` / `Install-GUI.cmd`: double-click launchers (full install with elevation, or controls only without elevation).
- `Install-Launcher.ps1`: preflight before UAC, same-account check, relays the elevated result to the original console.
- `install.ps1`: the installer. `-CheckOnly` (read-only), `-ControlsOnly`, `-AllowDowngrade`.
- `InstallSupport.ps1`: version detection (`FileVersionInfo` of the native executable, then `version.json`/registry), install action (Install/Upgrade/Repair/Downgrade) and the file transaction with rollback.
- `Uninstall.ps1`: removes exactly the files the installer placed; Samsung files, settings and logs stay.
- `tests/`: isolated tests that run on Windows only (temporary folders, isolated registry key, simulated engine hash and elevation). They need `build\Any2Odyssey.exe` in the repository root; see `docs/TESTING.md`.

## Package layout expected

```
VERSION
build\Any2Odyssey.exe
assets\Any2Odyssey.ico
USER-GUIDE.html
LICENSE
NOTICE
THIRD-PARTY-LICENSES.md
Install.cmd  Install-GUI.cmd  Install-Launcher.ps1  install.ps1  InstallSupport.ps1  Uninstall.ps1
```

`scripts/package.ps1` builds this layout into `dist/Any2Odyssey-<version>-windows-x64.zip`.

## Differences from the Any2Odd installer

- Single native executable: no SharpDX dependency DLLs, no `dependencies.json`, no shared-folder ownership tracking, no `restore.ps1`.
- The executable's version is read from its Windows version resource. The Rust build must embed `FileVersion` = `VERSION` + `.0` (see the app crate's build script).
- Any2Odd's clean-shutdown contract is kept in spirit: `Any2Odyssey.exe --quit` signals the running instance through its own named event.
