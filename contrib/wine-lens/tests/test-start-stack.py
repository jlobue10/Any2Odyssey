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
        (self.root / "tools/ensure-stack.sh").write_text(text.replace(BOARD, f"BOARD={self.root}/board"))
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


if __name__ == "__main__":
    unittest.main()
