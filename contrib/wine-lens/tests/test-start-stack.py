#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Run tools/start-stack.sh and tools/ensure-stack.sh against stand-ins for Wine: no hardware, no Wine, no root.

Two things a real Wine (11.21) showed with a stand-in for Leia's service are kept here in stand-ins for Wine.

Wine ends the services of a prefix when no ordinary program of it has run for as long as its server has
patience: three seconds, unless the server was started with another number ("wineserver -pN"). A service is
not an ordinary program. Until SRSession.exe runs, start-stack.sh has only short ordinary programs (sc,
monprobe, regedit) with pauses of several seconds between them, so with three seconds of patience Leia's
service is ended in the middle of its own start wherever "wine sc start" comes back quickly. The pauses are
not waited for here: `sleep` is a stand-in too, which moves a clock that the stand-in for Wine reads.

And a server that is killed outright leaves the prefix's programs behind. They can do nothing without it, but
they are found by name: Leia's service and session passed for a running stack at every request.
"""
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import time
import unittest

TOOLS = Path(__file__).resolve().parents[1] / "tools"
# The scripts are POSIX ones; another shell can be named to run the same tests under it
# (for example "dash" or "busybox sh").
SHELL = os.environ.get("A2O_START_STACK_TEST_SHELL", "sh").split()
# Wine's own patience when nothing else was asked for, in seconds.
WINE_PATIENCE = 3
BOARD = "BOARD=/dev/odyssey-lens"

# Each stand-in notes the clock and what it was asked. The clock is a number in a file.
NOTE = 'echo "$(cat "$ROOT/clock") {} $*" >>"$ROOT/calls"\n'
WINE = "#!/bin/sh\n" + NOTE.format("wine") + "exit 0\n"
WINESERVER = "#!/bin/sh\n" + NOTE.format("wineserver") + "exit 0\n"
# A program started in the background just before a pause must have read the clock before the pause moves it.
SLEEP = """#!/bin/sh
"$REAL_SLEEP" 0.15
echo $(( $(cat "$ROOT/clock") + $1 )) >"$ROOT/clock.new" && mv "$ROOT/clock.new" "$ROOT/clock"
"""
PASS_ON = '#!/bin/sh\nexec "$@"\n'
TIMEOUT = '#!/bin/sh\nshift\nexec "$@"\n'
# A program that stays: under the name of Wine's loader, of Wine's server, or of the serial port's keeper.
STAYS = "#include <unistd.h>\nint main(void) { for (;;) pause(); }\n"


class StackTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.built = tempfile.TemporaryDirectory()
        cls.addClassCleanup(cls.built.cleanup)
        source = Path(cls.built.name) / "stays.c"
        source.write_text(STAYS)
        for name in ("wine", "wineserver", "any2odyssey-noxon"):
            subprocess.run(["cc", "-o", str(Path(cls.built.name) / name), str(source)], check=True)

    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="a2o-start-stack-test-")
        self.addCleanup(self.directory.cleanup)
        # (as the scripts see it: they resolve their own path)
        self.root = Path(self.directory.name).resolve()
        for name in ("tools", "build", "bin", "run", "prefix/drive_c/Program Files/LeiaSR/Platform/bin"):
            (self.root / name).mkdir(parents=True)
        real_sleep = subprocess.run(["sh", "-c", "command -v sleep"], capture_output=True, text=True, check=True)
        self.environment = dict(os.environ, ROOT=str(self.root), REAL_SLEEP=real_sleep.stdout.strip(),
                                XDG_RUNTIME_DIR=str(self.root / "run"))
        self.environment.pop("WINEPREFIX", None)
        # Everything started below has the folder's name in its command line.
        self.addCleanup(subprocess.run, ["pkill", "-KILL", "-f", re.escape(str(self.root))])

    def stays(self, program, *arguments, prefix="prefix"):
        """A process of `program` (one of the three built above) that belongs to a prefix of this folder."""
        process = subprocess.Popen([str(Path(self.built.name) / program), *arguments, f"({self.root})"],
                                   env=dict(self.environment, WINEPREFIX=str(self.root / prefix)))
        self.addCleanup(process.wait)
        self.addCleanup(process.kill)
        return process

    def needs_pgrep(self):
        # BusyBox's pgrep cannot be told whose processes to look at; setup.sh refuses to set up with it.
        probe = subprocess.run([*SHELL, "-c", 'pgrep -u "$(id -u)" -f "[n]o such program"'], capture_output=True)
        if probe.stderr:
            self.skipTest("the pgrep this shell runs has no -u: the tools need the one from procps")

    def gone(self, process, seconds=3):
        try:
            process.wait(timeout=seconds)
        except subprocess.TimeoutExpired:
            return False
        return True


class StartStackTests(StackTests):
    def setUp(self):
        super().setUp()
        (self.root / "tools/start-stack.sh").write_text((TOOLS / "start-stack.sh").read_text())
        # What the script runs between its programs; the real one needs a display and a monitor.
        (self.root / "tools/mkedid.py").write_text("raise SystemExit(0)\n")
        (self.root / "build/monprobe.exe").write_text("")
        (self.root / "clock").write_text("0\n")
        for name, text in (("wine", WINE), ("wineserver", WINESERVER), ("sleep", SLEEP), ("setsid", PASS_ON),
                           ("timeout", TIMEOUT)):
            (self.root / "bin" / name).write_text(text)
            (self.root / "bin" / name).chmod(0o755)
        self.environment["PATH"] = f"{self.root}/bin{os.pathsep}{os.environ['PATH']}"

    def calls(self):
        """What the script asked of Wine: (seconds on the clock, program, arguments), in the clock's order."""
        result = subprocess.run([*SHELL, str(self.root / "tools/start-stack.sh")], env=self.environment,
                                stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=60)
        self.assertEqual(result.returncode, 0, result.stderr)
        # The last program is started in the background as the script ends.
        text = ""
        for _ in range(100):
            text = (self.root / "calls").read_text()
            if "SRSession.exe" in text:
                break
            time.sleep(0.05)
        calls = []
        for line in text.splitlines():
            clock, program, arguments = line.split(" ", 2)
            calls.append((int(clock), program, arguments))
        return sorted(calls, key=lambda call: call[0])

    def test_leias_service_outlives_the_pauses_of_its_own_start(self):
        calls = self.calls()
        if calls[-1][0] == 0:
            # BusyBox's shell runs its own sleep, not the stand-in: it waited the pauses out and the clock
            # never moved.
            self.skipTest("this shell has a sleep of its own; the stand-in clock did not move")
        programs = [call for call in calls if call[1] == "wine"]
        self.assertIn("sc start SR Service", [call[2] for call in programs])
        self.assertTrue(programs[-1][2].endswith("SRSession.exe"), programs)
        # The server's patience for this start: asked for after the old server was ended and before the first
        # program (which would start a server with Wine's own three seconds).
        ended = [call for call in calls if call[1] == "wineserver" and call[2].startswith("-k")]
        self.assertTrue(ended, calls)
        asked = [call for call in calls if call[1] == "wineserver" and call[2].startswith("-p")]
        patience = WINE_PATIENCE
        if asked:
            self.assertGreater(calls.index(asked[-1]), calls.index(ended[-1]), calls)
            self.assertLess(calls.index(asked[-1]), calls.index(programs[0]), calls)
            patience = int(asked[-1][2].split()[0][2:])
        # Every ordinary program here is as short as it can be (it takes no time on the clock): the service,
        # started by the second of them, lives only while the next comes within the server's patience.
        for before, after in zip(programs, programs[1:]):
            self.assertLess(after[0] - before[0], patience,
                            f'nothing ran between "{before[2]}" and "{after[2]}" for longer than Wine waits '
                            f"({patience} s) before it ends the prefix's services")
        # And not for ever: a server that never gives up would keep Leia's service after the session has gone.
        self.assertLessEqual(patience, 600)

    def test_the_toast_stand_in_is_registered_again_at_every_start(self):
        # Seen on the monitor's machine after Wine 11.21 became 11.22: the prefix's update registered Wine's
        # own toast library for two of the stand-in's three classes, and Leia's session ended at every start.
        winrt = "[HKEY_LOCAL_MACHINE\\Software\\Microsoft\\WindowsRuntime\\ActivatableClassId\\"
        classes = ("Windows.UI.Notifications.ToastNotification", "Windows.UI.Notifications.ToastNotificationManager",
                   "Windows.Data.Xml.Dom.XmlDocument")
        stand_in = '"DllPath"="C:\\\\windows\\\\system32\\\\a2otoast.dll"'
        # As tools/mkboard.py writes it: the set-up's other entries first, which a start must not write again.
        device = ["REGEDIT4", "", "[HKEY_LOCAL_MACHINE\\Software\\Wine\\Ports]", '"COM3"="/dev/odyssey-lens"', ""]
        for name in classes:
            device += [f"{winrt}{name}]", stand_in, '"Threading"=dword:00000000', ""]
        device += ["[HKEY_LOCAL_MACHINE\\Software\\Any2Odyssey\\After]", '"Later"=dword:00000001', ""]
        (self.root / "build/device.reg").write_text("\n".join(device))
        # The monitors' entries, as tools/mkedid.py writes them: Windows line ends, no empty line at the end.
        monitors = 'REGEDIT4\r\n\r\n[HKEY_LOCAL_MACHINE\\System\\Monitors]\r\n"Linked"=dword:00000001\r\n'
        (self.root / "tools/mkedid.py").write_text(
            f"import sys\nopen(sys.argv[2], 'w', newline='').write({monitors!r})\n")
        calls = self.calls()
        imported = [call[2] for call in calls if call[1] == "wine" and call[2].startswith("regedit")]
        self.assertEqual(imported, [f"regedit /S {self.root}/edid.reg"])
        text = (self.root / "edid.reg").read_bytes().decode()
        self.assertTrue(text.startswith(monitors), text)
        added = text[len(monitors):]
        self.assertEqual(added.replace("\r\n", ""), added.replace("\r\n", "").replace("\n", ""), "a bare line end")
        sections = [block.split("\r\n") for block in added.strip("\r\n").split("\r\n\r\n")]
        self.assertEqual([block[0] for block in sections], [f"{winrt}{name}]" for name in classes])
        for block in sections:
            self.assertEqual(block[1:], [stand_in, '"Threading"=dword:00000000'])

    def test_a_start_without_the_set_ups_entries_imports_the_monitors_alone(self):
        # (an installation whose build folder is from before the entries were kept there)
        monitors = 'REGEDIT4\r\n\r\n[HKEY_LOCAL_MACHINE\\System\\Monitors]\r\n"Linked"=dword:00000001\r\n'
        (self.root / "tools/mkedid.py").write_text(
            f"import sys\nopen(sys.argv[2], 'w', newline='').write({monitors!r})\n")
        calls = self.calls()
        self.assertIn(f"regedit /S {self.root}/edid.reg", [call[2] for call in calls])
        self.assertEqual((self.root / "edid.reg").read_bytes().decode(), monitors)

    # Wine's library folder as the script lists it first, and the names by which setup.sh keeps copies of
    # Wine's libraries in the prefix.
    WINE_LIBRARIES = "for d in /usr/lib/wine/x86_64-windows "
    # objdump -p, as far as the script reads it: the export names of a "library" whose first line lists them.
    OBJDUMP = """#!/bin/sh
echo "[Ordinal/Name Pointer] Table -- Ordinal Base 1"
echo "              Ordinal   Hint Name"
for name in $(sed -n '1s/^names: //p' "$2"); do echo "    [   0] +base[   1]  0000 $name"; done
echo
echo "The Function Table (interpreted .pdata section contents)"
"""

    def wine_libraries(self, installed, copies, listed):
        """Wine's libraries as installed now, the copies the set-up made, and the names its stand-ins list."""
        text = (TOOLS / "start-stack.sh").read_text()
        self.assertEqual(text.count(self.WINE_LIBRARIES), 1)
        (self.root / "tools/start-stack.sh").write_text(
            text.replace(self.WINE_LIBRARIES, f"for d in {self.root}/wine-libraries "))
        self.system32 = self.root / "prefix/drive_c/windows/system32"
        for folder in (self.root / "wine-libraries", self.system32):
            folder.mkdir(parents=True)
        for name, (names, build) in installed.items():
            (self.root / "wine-libraries" / f"{name}.dll").write_text(f"names: {names}\n{build}\n")
        for name, (names, build) in copies.items():
            (self.system32 / f"{name}.dll").write_text(f"names: {names}\n{build}\n")
        for name, names in listed.items():
            lines = [f"LIBRARY {name}.dll", "EXPORTS"] + [f"    {entry} = copy.{entry}" for entry in names.split()]
            (self.root / "build" / f"{name}.def").write_text("\n".join(lines) + "\n")
        (self.root / "bin/x86_64-w64-mingw32-objdump").write_text(self.OBJDUMP)
        (self.root / "bin/x86_64-w64-mingw32-objdump").chmod(0o755)
        # Every program of Wine notes with which libraries it ran: the overrides it was given, and which copy
        # of cfgmgr32 was in the prefix. The program that only lets Wine update the prefix can be made to fail.
        for name in ("wine", "wineserver"):
            (self.root / "bin" / name).write_text(
                "#!/bin/sh\n" + NOTE.format(name) +
                'case $1 in -k*) exit 0 ;; esac\n'
                'echo "$WINEDLLOVERRIDES with $(sed -n 2p "$ROOT/prefix/drive_c/windows/system32/cfgmgw.dll")" >>"$ROOT/libraries seen"\n'
                'if [ "$1" = cmd ] && [ -e "$ROOT/the update fails" ]; then exit 1; fi\n'
                "exit 0\n")

    OWN = "cfgmgr32,dxva2,wtsapi32,mf,mfreadwrite=b;mscoree,mshtml="
    STAND_INS = "mscoree,mshtml="

    def test_after_an_upgrade_wine_updates_the_prefix_with_its_own_libraries_first(self):
        # Seen with set-ups made under Wine 11.10, 11.14 and 11.17 and Wine then 11.22: with the cfgmgr32
        # stand-in in its way the new Wine's update of the prefix never ended, and with it no program of the
        # prefix; with Wine's own cfgmgr32 answering it took 14 seconds.
        same = "CM_Get_Child CM_Get_Parent CM_Locate_DevNodeW"
        self.wine_libraries(
            installed={"cfgmgr32": (same, "Wine 11.22"), "dxva2": ("DestroyPhysicalMonitor", "Wine 11.22"),
                       "wtsapi32": ("WTSFreeMemory", "the same in both"), "mf": ("MFGetService", "Wine 11.22")},
            copies={"cfgmgw": (same, "Wine 11.17"), "dxva2w": ("DestroyPhysicalMonitor", "Wine 11.17"),
                    "wtsapiw": ("WTSFreeMemory", "the same in both")},
            listed={"cfgmgr32": same, "dxva2": "DestroyPhysicalMonitor", "wtsapi32": "WTSFreeMemory"})
        untouched = (self.system32 / "wtsapiw.dll").stat().st_mtime_ns
        self.calls()
        # The first program of the new Wine is one that does nothing, given Wine's own five libraries; its
        # server is ended before the start proper, which runs with the stand-ins and the installed Wine's copies.
        seen = (self.root / "libraries seen").read_text().splitlines()
        self.assertEqual(seen[0], f"{self.OWN} with Wine 11.17")
        self.assertGreaterEqual(len(seen), 6, seen)
        self.assertEqual(set(seen[1:]), {f"{self.STAND_INS} with Wine 11.22"})
        asked = [line.split(" ", 1)[1] for line in (self.root / "calls").read_text().splitlines()]
        self.assertEqual(asked[:4], ["wineserver -k", "wine cmd /c exit", "wineserver -k", "wineserver -p60"])
        self.assertEqual((self.system32 / "cfgmgw.dll").read_text(), f"names: {same}\nWine 11.22\n")
        self.assertEqual((self.system32 / "dxva2w.dll").read_text(), "names: DestroyPhysicalMonitor\nWine 11.22\n")
        # A copy that is the installed Wine's is left as it is; a library the set-up made no copy of gets none
        # (eye tracking was not set up); and nothing is left of the copying.
        self.assertEqual((self.system32 / "wtsapiw.dll").stat().st_mtime_ns, untouched)
        self.assertEqual(sorted(path.name for path in self.system32.iterdir()), ["cfgmgw.dll", "dxva2w.dll", "wtsapiw.dll"])
        self.assertFalse((self.root / "stack.error").exists())
        # The next start finds nothing to do: no update of its own, the stand-ins from the first program on.
        (self.root / "libraries seen").unlink()
        (self.root / "calls").unlink()
        self.calls()
        seen = (self.root / "libraries seen").read_text().splitlines()
        self.assertEqual(set(seen), {f"{self.STAND_INS} with Wine 11.22"})
        self.assertNotIn("wine cmd /c exit", (self.root / "calls").read_text())

    def test_an_update_of_the_prefix_that_does_not_end_well_is_tried_again_at_the_next_start(self):
        same = "CM_Get_Child CM_Get_Parent"
        self.wine_libraries(installed={"cfgmgr32": (same, "Wine 11.22")}, copies={"cfgmgw": (same, "Wine 11.17")},
                            listed={"cfgmgr32": same})
        (self.root / "the update fails").write_text("")
        self.calls()
        # The start went on (as it always did), with the copy it had: that copy is what says an update is due.
        self.assertEqual((self.system32 / "cfgmgw.dll").read_text(), f"names: {same}\nWine 11.17\n")
        (self.root / "the update fails").unlink()
        (self.root / "libraries seen").unlink()
        self.calls()
        seen = (self.root / "libraries seen").read_text().splitlines()
        self.assertEqual(seen[0], f"{self.OWN} with Wine 11.17")
        self.assertEqual((self.system32 / "cfgmgw.dll").read_text(), f"names: {same}\nWine 11.22\n")

    def test_a_stand_in_that_no_longer_fits_wines_library_ends_the_start_with_its_reason(self):
        # Seen with a set-up made under Wine 11.0 and Wine then 11.22: Wine's setupapi asked the stand-in for a
        # name its library has had since, and no program of the prefix started any more.
        self.wine_libraries(
            installed={"cfgmgr32": ("CM_Get_Child CM_Get_Class_PropertyW CM_Get_Parent", "Wine 11.22")},
            copies={"cfgmgw": ("CM_Get_Child CM_Get_Parent", "Wine 11.0")},
            listed={"cfgmgr32": "CM_Get_Child CM_Get_Parent"})
        (self.root / "stack.error").write_text("the reason of an earlier start\n")
        result = subprocess.run([*SHELL, str(self.root / "tools/start-stack.sh")], env=self.environment,
                                stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=60)
        self.assertEqual(result.returncode, 1, result.stderr)
        reason = (self.root / "stack.error").read_text()
        self.assertIn("Wine has been upgraded", reason)
        self.assertIn("cfgmgr32.dll", reason)
        self.assertIn("any2odyssey-setup", reason)
        self.assertEqual(len(reason.splitlines()), 1, reason)
        # Nothing of the new Wine was started on the way, and the copy is as it was.
        self.assertFalse((self.root / "libraries seen").exists())
        self.assertEqual((self.system32 / "cfgmgw.dll").read_text(), "names: CM_Get_Child CM_Get_Parent\nWine 11.0\n")
        # With the names as the stand-in lists them the start goes on, and the old reason is gone.
        (self.root / "wine-libraries/cfgmgr32.dll").write_text("names: CM_Get_Parent CM_Get_Child\nWine 11.22\n")
        self.calls()
        self.assertFalse((self.root / "stack.error").exists())
        self.assertEqual((self.system32 / "cfgmgw.dll").read_text(), "names: CM_Get_Parent CM_Get_Child\nWine 11.22\n")

    def test_what_wine_imports_is_what_the_set_up_wrote_for_the_three_classes(self):
        # The set-up's file as tools/mkboard.py itself writes it, from a stand-in for the lens board's folder
        # in sysfs: Windows line ends, five lines an entry, and after the three the entries of the board, one
        # of which removes a key. With bare line ends in its place nothing here read the "\r" the script takes
        # off: left on, no entry ever ends, and every start imports the rest of the file with the three.
        board = self.root / "board-in-sysfs"
        board.mkdir()
        device = bytes([18, 1, 0, 2, 2, 0, 0, 64, 0x4B, 0x35, 0x16, 0x01, 0x01, 0x02, 1, 2, 3, 1])
        (board / "descriptors").write_bytes(device + bytes([9, 2, 9, 0, 1, 1, 0, 0x80, 50]))
        for name, text in (("idVendor", "354b"), ("idProduct", "0116"), ("serial", "A2O0001"), ("devnum", "7"),
                           ("manufacturer", "A maker"), ("product", "A lens board")):
            (board / name).write_text(text + "\n")
        source = (TOOLS / "mkboard.py").read_text()
        run_it = "main(*sys.argv[1:4])"
        self.assertEqual(source.count(run_it), 1)
        tool = {}
        exec(compile(source.replace(run_it, ""), "mkboard.py", "exec"), tool)
        tool["usb_device"] = lambda tty: str(board)
        tool["main"]("/dev/odyssey-lens", str(self.root / "build/board.h"), str(self.root / "build/device.reg"))
        header, registry = self.root / "build/board.h", self.root / "build/device.reg"
        before = (header.read_bytes(), registry.read_bytes())
        descriptors = board / "descriptors"
        valid = descriptors.read_bytes()
        for incomplete in [valid[:17], valid[:23], valid[:-1]]:
            descriptors.write_bytes(incomplete)
            with self.assertRaisesRegex(SystemExit, "USB descriptors"):
                tool["main"]("/dev/odyssey-lens", str(header), str(registry))
            self.assertEqual((header.read_bytes(), registry.read_bytes()), before)
        written = registry.read_bytes().decode()
        self.assertNotIn("\n", written.replace("\r\n", ""), "the set-up's file has Windows line ends")
        wanted = [entry for entry in written.split("\r\n\r\n") if "\\WindowsRuntime\\ActivatableClassId\\" in entry]
        self.assertEqual(len(wanted), 3, written)
        self.assertTrue(all("a2otoast.dll" in entry and len(entry.split("\r\n")) == 5 for entry in wanted), wanted)
        self.assertIn("\r\n[-HKEY_LOCAL_MACHINE\\", written)

        monitors = 'REGEDIT4\r\n\r\n[HKEY_LOCAL_MACHINE\\System\\Monitors]\r\n"Linked"=dword:00000001\r\n'
        (self.root / "tools/mkedid.py").write_text(
            f"import sys\nopen(sys.argv[2], 'w', newline='').write({monitors!r})\n")
        # What counts is the file at the moment Wine is asked to import it, not as the script leaves it.
        (self.root / "bin/wine").write_text("#!/bin/sh\n" + NOTE.format("wine")
                                            + '[ "$1" = regedit ] && cp "$3" "$ROOT/imported.reg"\nexit 0\n')
        self.calls()
        imported = (self.root / "imported.reg").read_bytes().decode()
        self.assertTrue(imported.startswith(monitors), imported)
        added = imported[len(monitors):]
        self.assertNotIn("\n", added.replace("\r\n", ""), "a bare line end")
        self.assertNotIn("\r", added.replace("\r\n", ""), "a line end with a second \\r")
        self.assertEqual(added.strip("\r\n").split("\r\n\r\n"), wanted)
        self.assertEqual((self.root / "edid.reg").read_bytes().decode(), imported, "changed after the import")

    def test_what_a_killed_server_left_behind_is_ended_and_nothing_else(self):
        self.needs_pgrep()
        service = r"C:\Program Files\LeiaSR\Platform\bin\SRService.exe"
        left = self.stays("wine", service)
        tracker = self.stays("wine", r"C:\Program Files\LeiaSR\Tracker\bin\SREyeTracker.exe")
        # Not of this prefix; not one of Wine's programs (the keeper has the prefix in its environment, and
        # the name of Leia's service in its command line).
        other = self.stays("wine", service, prefix="another-prefix")
        keeper = self.stays("any2odyssey-noxon", "/dev/odyssey-lens", "SRService.exe")
        self.calls()
        self.assertTrue(self.gone(left), "Leia's service of a server that is gone was left running")
        self.assertTrue(self.gone(tracker), "Leia's tracker of a server that is gone was left running")
        self.assertFalse(self.gone(other, 0.3), "a program of another prefix was ended")
        self.assertFalse(self.gone(keeper, 0.3), "the serial port's keeper was ended")


class EnsureStackTests(StackTests):
    def setUp(self):
        super().setUp()
        self.needs_pgrep()
        # The script finds Leia's programs among all of this user's processes.
        if subprocess.run(["pgrep", "-u", str(os.getuid()), "-f", r"bin.[S]RService\.exe"],
                          stdout=subprocess.DEVNULL).returncode == 0:
            self.skipTest("Leia's programs are running here")
        text = (TOOLS / "ensure-stack.sh").read_text()
        # The one thing changed in the script: where the lens board is.
        self.assertEqual(text.count(BOARD), 1)
        # (in quotes: a temporary folder with a blank in its name cut the path short, and the test said
        # that the board was missing)
        (self.root / "tools/ensure-stack.sh").write_text(text.replace(BOARD, f'BOARD="{self.root}/board"'))
        (self.root / "board").write_text("")
        for name, body in (("start-stack.sh", 'echo started >>"$ROOT/starts"'), ("noxon.sh", "exit 0")):
            (self.root / "tools" / name).write_text(f"#!/bin/sh\n{body}\n")
            (self.root / "tools" / name).chmod(0o755)
        log = self.root / "prefix/drive_c/ProgramData/Simulated Reality/Server/Log"
        log.mkdir(parents=True)
        (log / "service.txt").write_text("lens board responded to initial disable\n")
        # Leia's service and session, in the list of processes, and a start that came after the board did.
        for name in ("bin/SRService.exe", "SRSession.exe"):
            program = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(300)", f"{self.root}/{name}"])
            self.addCleanup(program.wait)
            self.addCleanup(program.kill)
        (self.root / "stack.started").write_text("")
        deadline = time.monotonic() + 5
        while subprocess.run(["pgrep", "-u", str(os.getuid()), "-f", re.escape(f"{self.root}/SRSession.exe")],
                             stdout=subprocess.DEVNULL).returncode != 0:
            self.assertLess(time.monotonic(), deadline, "timed out")
            time.sleep(0.02)

    def starts(self):
        """How often the stack was started again by one request."""
        result = subprocess.run([*SHELL, str(self.root / "tools/ensure-stack.sh")], env=self.environment,
                                stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=60)
        self.assertEqual((result.returncode, result.stdout), (0, ""), result.stderr)
        started = self.root / "starts"
        count = len(started.read_text().splitlines()) if started.exists() else 0
        started.unlink(missing_ok=True)
        return count

    def test_programs_whose_server_is_gone_are_not_a_running_stack(self):
        # Leia's programs are there and no server of Wine is: they were left behind.
        self.assertEqual(self.starts(), 1)
        # Nor does the server of another prefix make them a stack.
        self.stays("wineserver", prefix="another-prefix")
        self.assertEqual(self.starts(), 1)
        # With this prefix's server the stack is as it should be, and is left alone.
        self.stays("wineserver")
        self.assertEqual(self.starts(), 0)
        self.assertEqual(self.starts(), 0)

    def test_a_start_that_ends_with_a_reason_of_its_own_is_reported_with_it(self):
        def asked(body):
            (self.root / "tools/start-stack.sh").write_text(f"#!/bin/sh\n{body}\n")
            return subprocess.run([*SHELL, str(self.root / "tools/ensure-stack.sh")], env=self.environment,
                                  stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=60)

        result = asked('echo "Wine has been upgraded: run the set-up again." >"$ROOT/stack.error"; exit 1')
        self.assertEqual((result.returncode, result.stdout), (1, "error: Wine has been upgraded: run the set-up again.\n"))
        # Without one it is the line it always was (the reason of a start before is not this start's: the
        # script removes it first, as the stand-in does here).
        result = asked('rm -f "$ROOT/stack.error"; exit 1')
        self.assertEqual((result.returncode, result.stdout),
                         (1, f"error: could not start Leia's service (see {self.root}/edid.pairs)\n"))

    def test_a_server_whose_wine_was_replaced_is_not_a_running_stack(self):
        # Seen with Wine 11.21 replaced by 11.22 (WineHQ's packages) while the stack was up: Leia's programs
        # and the old server ran on, every program started afterwards was the new Wine's and was refused by
        # that server ("version mismatch"), and the stack counted as running at every request.
        old = self.root / "the-wine-before"
        old.mkdir()
        shutil.copy2(Path(self.built.name) / "wineserver", old / "wineserver")
        server = subprocess.Popen([str(old / "wineserver"), f"({self.root})"],
                                  env=dict(self.environment, WINEPREFIX=str(self.root / "prefix")))
        self.addCleanup(server.wait)
        self.addCleanup(server.kill)
        self.assertEqual(self.starts(), 0)
        # What a package manager does with the file of a program that runs: the name goes, the program stays.
        (old / "wineserver").unlink()
        self.assertEqual(self.starts(), 1)
        # A server of the Wine that is installed now makes it a stack again.
        self.stays("wineserver")
        self.assertEqual(self.starts(), 0)


if __name__ == "__main__":
    unittest.main()
