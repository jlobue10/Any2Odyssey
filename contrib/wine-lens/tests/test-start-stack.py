#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Run tools/start-stack.sh against stand-ins for Wine that keep Wine's rule for services: no hardware, no Wine.

Wine ends the services of a prefix when no ordinary program of it has run for as long as its server has
patience: three seconds, unless the server was started with another number ("wineserver -pN"). A service is
not an ordinary program. Until SRSession.exe runs, start-stack.sh has only short ordinary programs (sc,
monprobe, regedit) with pauses of several seconds between them, so with three seconds of patience Leia's
service is ended in the middle of its own start wherever "wine sc start" comes back quickly. That was found
with a real Wine (11.21) and a stand-in service; this test keeps the rule in a stand-in for Wine.

The pauses are not waited for: `sleep` is a stand-in too, which moves a clock that the stand-in for Wine reads.
"""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / "tools/start-stack.sh"
# The script is a POSIX one; another shell can be named to run the same tests under it
# (for example "dash" or "busybox sh").
SHELL = os.environ.get("A2O_START_STACK_TEST_SHELL", "sh").split()
# Wine's own patience when nothing else was asked for, in seconds.
WINE_PATIENCE = 3

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


class StartStackTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="a2o-start-stack-test-")
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        for name in ("tools", "build", "bin", "prefix/drive_c/Program Files/LeiaSR/Platform/bin"):
            (self.root / name).mkdir(parents=True)
        (self.root / "tools/start-stack.sh").write_text(SCRIPT.read_text())
        # What the script runs between its programs; the real one needs a display and a monitor.
        (self.root / "tools/mkedid.py").write_text("raise SystemExit(0)\n")
        (self.root / "build/monprobe.exe").write_text("")
        (self.root / "clock").write_text("0\n")
        real_sleep = subprocess.run(["sh", "-c", "command -v sleep"], capture_output=True, text=True, check=True)
        for name, text in (("wine", WINE), ("wineserver", WINESERVER), ("sleep", SLEEP), ("setsid", PASS_ON),
                           ("timeout", TIMEOUT)):
            (self.root / "bin" / name).write_text(text)
            (self.root / "bin" / name).chmod(0o755)
        self.environment = dict(os.environ, ROOT=str(self.root), REAL_SLEEP=real_sleep.stdout.strip(),
                                PATH=f"{self.root}/bin{os.pathsep}{os.environ['PATH']}")

    def calls(self):
        """What the script asked of Wine: (seconds on the clock, program, arguments), in the clock's order."""
        result = subprocess.run([*SHELL, str(self.root / "tools/start-stack.sh")], env=self.environment,
                                stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=60)
        self.assertEqual(result.returncode, 0, result.stderr)
        # The last program is started in the background as the script ends.
        calls = []
        for _ in range(100):
            text = (self.root / "calls").read_text()
            if "SRSession.exe" in text:
                break
            subprocess.run([self.environment["REAL_SLEEP"], "0.05"], check=True)
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


if __name__ == "__main__":
    unittest.main()
