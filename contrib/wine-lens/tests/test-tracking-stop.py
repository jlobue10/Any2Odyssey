#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Run tools/tracking.sh against stand-ins for Wine, Leia's programs and the camera: no hardware, no root.

How long Leia's eye tracker takes to appear after "sc start" and to leave after "sc stop" is the stand-in's
to choose here, which is the point: stopping must hold for any of it.
"""
import os
from pathlib import Path
import re
import signal
import subprocess
import sys
import tempfile
import time
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / "tools/tracking.sh"
# The script is a POSIX one; another shell can be named to run the same tests under it
# (for example "dash" or "busybox sh").
SHELL = os.environ.get("A2O_TRACKING_TEST_SHELL", "sh").split()
DEVICES = "/sys/bus/usb/devices/*"
BRIDGE = "#include <unistd.h>\nint main(void) { for (;;) pause(); }\n"
# Stands in for Wine: "sc start" brings the tracker after a while, "sc stop" answers at once and the tracker
# leaves after a while (or never).
WINE = r"""#!/bin/sh
echo "$*" >>"$ROOT/wine-calls"
case "$*" in
"sc start SR Eye Tracker")
    sleep "$(cat "$ROOT/start-delay")"
    setsid python3 -c 'import time; time.sleep(300)' "$ROOT/SREyeTracker.exe" </dev/null >/dev/null 2>&1 &
    echo $! >"$ROOT/tracker.pid"
    ;;
"sc stop SR Eye Tracker")
    pid=$(cat "$ROOT/tracker.pid" 2>/dev/null) || exit 0
    delay=$(cat "$ROOT/stop-delay")
    case $delay in
        never) ;;
        0) kill "$pid" 2>/dev/null; while [ -d "/proc/$pid" ]; do sleep 0.01; done ;;
        *) (sleep "$delay"; kill "$pid") </dev/null >/dev/null 2>&1 & ;;
    esac
    ;;
esac
exit 0
"""


def running(pattern):
    return subprocess.run(["pgrep", "-u", str(os.getuid()), "-f", pattern], stdout=subprocess.DEVNULL).returncode == 0


class TrackingStopTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.built = tempfile.TemporaryDirectory()
        cls.addClassCleanup(cls.built.cleanup)
        source = Path(cls.built.name) / "bridge.c"
        source.write_text(BRIDGE)
        subprocess.run(["cc", "-o", str(Path(cls.built.name) / "bridge"), str(source)], check=True)

    def setUp(self):
        # The script finds Leia's programs among all of this user's processes.
        if running(r"[S]REyeTracker\.exe|bin.[S]RService\.exe"):
            self.skipTest("Leia's programs are running here")
        # BusyBox's pgrep cannot be told whose processes to look at, and the script finds nothing with it.
        text = SCRIPT.read_text()
        probe = subprocess.run([*SHELL, "-c", 'pgrep -u "$(id -u)" -f "[n]o such program"'], capture_output=True)
        if "pgrep -u" in text and probe.stderr:
            self.skipTest("the pgrep this shell runs has no -u: tracking.sh needs the one from procps")
        self.directory = tempfile.TemporaryDirectory(prefix="a2o-tracking-test-")
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        for name in ("tools", "build", "bin", "run", "usb/camera/power"):
            (self.root / name).mkdir(parents=True)
        # The one thing changed in the script: where it looks for the webcam.
        self.assertEqual(text.count(DEVICES), 1)
        (self.root / "tools/tracking.sh").write_text(text.replace(DEVICES, f"{self.root}/usb/*"))
        (self.root / "usb/camera/idVendor").write_text("04e8\n")
        (self.root / "usb/camera/idProduct").write_text("20d7\n")
        (self.root / "usb/camera/power/control").write_text("on\n")
        bridge = self.root / "build/any2odyssey-camera-bridge"
        bridge.write_bytes((Path(self.built.name) / "bridge").read_bytes())
        bridge.chmod(0o755)
        (self.root / "camera.port").write_text("47816\n")
        (self.root / "camera.secret").write_text("not a secret\n")
        (self.root / "bin/wine").write_text(WINE)
        (self.root / "bin/wine").chmod(0o755)
        self.delays(start="0.1", stop="0")
        self.environment = dict(os.environ, ROOT=str(self.root), XDG_RUNTIME_DIR=str(self.root / "run"),
                                PATH=f"{self.root}/bin{os.pathsep}{os.environ['PATH']}")
        # Everything started below has the folder's name in its command line.
        self.addCleanup(subprocess.run, ["pkill", "-KILL", "-f", re.escape(str(self.root))])
        # Leia's service and session, which "start" asks for.
        for name in ("bin/SRService.exe", "SRSession.exe"):
            service = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(300)", f"{self.root}/{name}"])
            self.addCleanup(service.wait)
            self.addCleanup(service.kill)
        self.wait_until(lambda: running(re.escape(f"{self.root}/SRSession.exe")), 5)

    def delays(self, start=None, stop=None):
        if start is not None:
            (self.root / "start-delay").write_text(start + "\n")
        if stop is not None:
            (self.root / "stop-delay").write_text(stop + "\n")

    def wait_until(self, condition, seconds):
        deadline = time.monotonic() + seconds
        while not condition():
            self.assertLess(time.monotonic(), deadline, "timed out")
            time.sleep(0.02)

    def command(self, verb):
        return [*SHELL, str(self.root / "tools/tracking.sh"), verb]

    def start(self):
        result = subprocess.run(self.command("start"), env=self.environment, stdin=subprocess.DEVNULL,
                                capture_output=True, text=True, timeout=40)
        self.assertEqual(result.stdout, "on\n", result.stderr)

    def stop(self):
        """Stop; returns the seconds it took."""
        began = time.monotonic()
        subprocess.run(self.command("stop"), env=self.environment, stdin=subprocess.DEVNULL, timeout=90, check=True)
        return time.monotonic() - began

    def wine_calls(self):
        calls = self.root / "wine-calls"
        return calls.read_text().splitlines() if calls.exists() else []

    def tracker(self):
        return running(re.escape(f"{self.root}/SREyeTracker.exe"))

    def bridge(self):
        return running(re.escape(f"{self.root}/build/any2odyssey-camera-bridge"))

    def test_a_start_right_after_a_stop_has_a_tracker_that_stays(self):
        self.start()
        # Leia's service answers "sc stop" at once; the tracker takes two more seconds to go.
        self.delays(stop="2")
        self.stop()
        self.start()
        time.sleep(3)
        self.assertTrue(self.tracker(), "the start took the leaving tracker for a running one")
        self.assertTrue(self.bridge())

    def test_a_stop_while_the_start_is_under_way_leaves_no_tracker(self):
        # Any2Odyssey ends the helper, and "start" with it, when the picture is stopped before it showed;
        # the request to Leia's service is on its way by then and the tracker comes two seconds later.
        self.delays(start="2")
        start = subprocess.Popen(self.command("start"), env=self.environment, stdin=subprocess.DEVNULL,
                                 stdout=subprocess.DEVNULL, start_new_session=True)
        self.addCleanup(start.wait)
        self.wait_until(lambda: "sc start SR Eye Tracker" in self.wine_calls(), 10)
        os.killpg(start.pid, signal.SIGKILL)
        start.wait()
        self.stop()
        time.sleep(3)
        self.assertFalse(self.tracker(), "the tracker came after the stop and stayed")
        self.assertFalse(self.bridge())

    def test_a_tracker_that_does_not_leave_holds_the_stop_for_a_bounded_time(self):
        self.start()
        self.delays(stop="never")
        seconds = self.stop()
        self.assertIn("sc stop SR Eye Tracker", self.wine_calls())
        self.assertGreater(seconds, 5)
        self.assertLess(seconds, 25)
        self.assertFalse(self.bridge())

    def test_a_tracker_that_leaves_at_once_costs_no_wait(self):
        self.start()
        seconds = self.stop()
        self.assertFalse(self.tracker())
        self.assertFalse(self.bridge())
        self.assertLess(seconds, 0.5)

    def test_a_stop_with_nothing_running_asks_wine_nothing(self):
        seconds = self.stop()
        self.assertEqual(self.wine_calls(), [])
        self.assertLess(seconds, 1.0)
        # And with the lock file of an earlier start left behind, as it always is.
        self.start()
        self.stop()
        calls = self.wine_calls()
        seconds = self.stop()
        self.assertEqual(self.wine_calls(), calls)
        self.assertLess(seconds, 1.0)


if __name__ == "__main__":
    unittest.main()
