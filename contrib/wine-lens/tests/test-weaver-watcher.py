#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Run any2odyssey-weaver's leaving process against stand-ins: no Wine, monitor, desktop or root."""
import os
from pathlib import Path
import shutil
import signal
import subprocess
import tempfile
import time
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / "any2odyssey-weaver"


class WatcherTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        for name in ("build", "tools", "bin"):
            (self.root / name).mkdir()
        shutil.copy(SCRIPT, self.root / "any2odyssey-weaver")
        (self.root / "any2odyssey-weaver").chmod(0o755)
        (self.root / "build/any2odyssey-weaver.exe").touch()
        (self.root / "frame").touch()
        # Leia's service "starting": notes its process and stays, so the script waits in it.
        self.program("tools/ensure-stack.sh", 'echo $$ >"$ROOT/starting"\nexec sleep 60\n')
        self.program("tools/tracking.sh", 'echo "$1" >>"$ROOT/tracking"\n')
        self.program("bin/busctl", "exit 1\n")  # no desktop is asked anything
        self.environment = dict(os.environ, ROOT=str(self.root), PATH=f"{self.root}/bin:{os.environ['PATH']}")

    def program(self, name, body):
        path = self.root / name
        path.write_text("#!/bin/sh\n" + body)
        path.chmod(0o755)

    def wait_for(self, name, seconds):
        deadline = time.monotonic() + seconds
        while not (self.root / name).exists():
            self.assertLess(time.monotonic(), deadline, f"{name} did not appear")
            time.sleep(0.02)
        return (self.root / name).read_text()

    def start(self):
        script = subprocess.Popen([str(self.root / "any2odyssey-weaver"), str(self.root / "frame")],
                                  env=self.environment, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL)
        self.addCleanup(script.wait)
        self.addCleanup(script.kill)
        self.wait_for("starting", 10)
        self.addCleanup(self.end_stand_in)
        return script

    def end_stand_in(self):
        try:
            os.kill(int((self.root / "starting").read_text()), signal.SIGKILL)
        except (OSError, ValueError):
            pass

    def end(self, script):
        """End the script the way Any2Odyssey may: at once, with no chance to tidy up."""
        script.kill()
        script.wait()
        self.end_stand_in()
        # What was started is undone by the process that stayed: here, eye tracking is stopped.
        self.assertEqual(self.wait_for("tracking", 5), "stop\n")

    def test_the_leaving_is_done_when_the_script_is_killed(self):
        self.end(self.start())

    def test_a_flock_without_a_timed_wait_does_not_make_the_wait_spin(self):
        # BusyBox's flock has no -w and fails at once when given it.
        real = shutil.which("flock")
        self.program("bin/flock", 'case $1 in -w) echo wait >>"$ROOT/waits"; exit 1 ;; esac\n' f'exec "{real}" "$@"\n')
        script = self.start()
        time.sleep(2)
        waits = (self.root / "waits").read_text().count("\n") if (self.root / "waits").exists() else 0
        self.assertGreater(waits, 0)
        self.assertLess(waits, 30, "the wait for the script is a busy loop")
        self.end(script)


if __name__ == "__main__":
    unittest.main()
