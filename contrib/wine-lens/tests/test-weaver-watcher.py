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
        self.end(script)
        if not waits:  # BusyBox's own sh runs its own flock whatever the path says: nothing was counted
            self.skipTest("this sh does not take flock from the path")
        self.assertLess(waits, 30, "the wait for the script is a busy loop")


class TrackingAnswerTests(unittest.TestCase):
    """What the script says about eye tracking is tracking.sh's answer, whatever else is written beside it."""

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
        self.program("tools/ensure-stack.sh", "exit 0\n")
        (self.root / "tools/mkedid.py").write_text(
            'import sys\nopen(sys.argv[2], "w").write("0 0 3840 2160\\n")\n')
        self.program("bin/busctl", "exit 1\n")
        # The weaver host: the picture shows at once, and it leaves.
        self.program("bin/wine", "echo on\n")
        self.environment = dict(os.environ, ROOT=str(self.root), PATH=f"{self.root}/bin:{os.environ['PATH']}")

    def program(self, name, body):
        path = self.root / name
        path.write_text("#!/bin/sh\n" + body)
        path.chmod(0o755)

    def said(self, start, **environment):
        """The script's lines, with a tracking.sh that does `start` when asked to start."""
        (self.root / "stopped").unlink(missing_ok=True)
        self.program("tools/tracking.sh",
                     f'case $1 in\nstart) {start} ;;\nstop) echo stopped >"$ROOT/stopped" ;;\nesac\n')
        done = subprocess.run([str(self.root / "any2odyssey-weaver"), str(self.root / "frame")],
                              env=dict(self.environment, **environment), stdin=subprocess.DEVNULL,
                              stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True, timeout=30)
        # The process that stays does the leaving; it is waited for before the folder goes.
        deadline = time.monotonic() + 15
        while not (self.root / "stopped").exists():
            self.assertLess(time.monotonic(), deadline, "tracking was not stopped")
            time.sleep(0.02)
        return done.stdout.splitlines()

    def test_tracking_that_started_is_not_reported(self):
        self.assertEqual(self.said("echo on"), ["on"])

    def test_a_reason_is_passed_on_in_one_line(self):
        self.assertEqual(self.said('echo "error: the webcam is not present"'),
                         ["tracking: the webcam is not present", "on"])

    def test_a_warning_beside_the_answer_is_no_part_of_it(self):
        warning = "echo 'sh: warning: setlocale: LC_ALL: cannot change locale (xx_XX.UTF-8)' >&2"
        self.assertEqual(self.said(f"{warning}; echo on"), ["on"])
        # Taken as part of the answer, the reason went out as a line of its own, "error: ...", and that
        # line ends the picture.
        self.assertEqual(self.said(f'{warning}; echo "error: the webcam is not present"'),
                         ["tracking: the webcam is not present", "on"])

    def test_a_locale_the_system_does_not_have_does_not_turn_on_into_a_reason(self):
        # bash, which is sh on Arch and Fedora, warns about such a locale whenever it starts; dash says nothing.
        self.assertEqual(self.said("echo on", LC_ALL="xx_XX.UTF-8"), ["on"])


if __name__ == "__main__":
    unittest.main()
