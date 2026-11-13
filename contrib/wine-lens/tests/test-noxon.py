#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Run the serial port's keeper against a pseudo-terminal: no lens board, Wine, or root."""
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import termios
import time
import unittest


class NoxonTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.directory = tempfile.TemporaryDirectory()
        cls.binary = os.environ.get("NOXON_TEST_BINARY", str(Path(cls.directory.name) / "noxon"))
        if "NOXON_TEST_BINARY" not in os.environ:
            source = Path(__file__).resolve().parents[1] / "src/noxon.c"
            subprocess.run(["cc", "-O2", "-Wall", "-Wextra", "-Werror", "-o", cls.binary, str(source)], check=True)

    @classmethod
    def tearDownClass(cls):
        cls.directory.cleanup()

    def setUp(self):
        self.master, self.port = os.openpty()
        self.addCleanup(os.close, self.master)
        self.addCleanup(os.close, self.port)
        # Stands in for Leia's service: a process with a name of its own in its command line.
        self.name = f"a2o-noxon-test-{os.getpid()}-{time.monotonic_ns()}"
        self.service = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(300)", self.name])
        self.addCleanup(self.end, self.service)

    def end(self, process):
        if process.poll() is None:
            process.kill()
        process.wait(timeout=5)

    def keeper(self, name):
        process = subprocess.Popen([self.binary, os.ttyname(self.port), name, "1"])
        self.addCleanup(self.end, process)
        return process

    def flags(self):
        return termios.tcgetattr(self.port)[0]

    def set_flags(self, on, off=0):
        settings = termios.tcgetattr(self.port)
        settings[0] = (settings[0] | on) & ~off
        termios.tcsetattr(self.port, termios.TCSANOW, settings)

    def wait_until(self, condition, seconds):
        deadline = time.monotonic() + seconds
        while not condition():
            self.assertLess(time.monotonic(), deadline, "timed out")
            time.sleep(0.01)

    def test_flow_control_is_switched_off_again(self):
        self.set_flags(termios.IXON | termios.IXOFF)
        self.keeper(self.name)
        for _ in range(3):  # Wine switches it on again whenever the service configures the port
            self.wait_until(lambda: not self.flags() & (termios.IXON | termios.IXOFF), 2)
            self.set_flags(termios.IXON | termios.IXOFF)

    def test_other_settings_are_left_alone(self):
        # As the shell loop did: nothing is touched while IXON is off, and only the two flags when it is on.
        self.set_flags(termios.IXOFF | termios.ICRNL, termios.IXON)
        before = termios.tcgetattr(self.port)
        self.keeper(self.name)
        time.sleep(0.5)
        self.assertEqual(termios.tcgetattr(self.port), before)
        self.set_flags(termios.IXON)
        self.wait_until(lambda: not self.flags() & termios.IXON, 2)
        after = termios.tcgetattr(self.port)
        before[0] &= ~(termios.IXON | termios.IXOFF)
        self.assertEqual(after, before)

    def test_leaves_when_the_service_has_gone(self):
        keeper = self.keeper(self.name)
        time.sleep(2.5)  # it has seen the service by now
        self.assertIsNone(keeper.poll())
        self.end(self.service)
        self.assertEqual(keeper.wait(timeout=6), 0)

    def test_two_keepers_do_not_hold_each_other(self):
        # A keeper has the service's name among its own arguments. One left over from an earlier start must
        # not pass for the service, or the two would keep each other running for good.
        keepers = [self.keeper(self.name), self.keeper(self.name)]
        time.sleep(2.5)
        self.assertEqual([keeper.poll() for keeper in keepers], [None, None])
        self.end(self.service)
        for keeper in keepers:
            self.assertEqual(keeper.wait(timeout=6), 0)

    def test_a_missing_port_is_waited_for(self):
        keeper = subprocess.Popen([self.binary, "/nonexistent/odyssey-lens", self.name, "1"])
        self.addCleanup(self.end, keeper)
        time.sleep(1.5)
        self.assertIsNone(keeper.poll())

    def test_usage(self):
        self.assertEqual(subprocess.run([self.binary], stderr=subprocess.DEVNULL).returncode, 2)
        self.assertEqual(subprocess.run([self.binary, "/dev/null", "x", "0"], stderr=subprocess.DEVNULL).returncode, 2)


if __name__ == "__main__":
    unittest.main()
