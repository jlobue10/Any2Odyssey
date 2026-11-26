#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""The scaler's switch against a stand-in for sysfs, and any2odyssey-weaver's use of it: no monitor, Wine or root."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time
import unittest

HERE = Path(__file__).resolve().parents[1]


class ScalerTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.directory = tempfile.TemporaryDirectory()
        cls.binary = str(Path(cls.directory.name) / "scaler")
        subprocess.run(["cc", "-std=c11", "-O2", "-Wall", "-Wextra", "-Werror", "-o", cls.binary,
                        str(HERE / "src/scaler.c")], check=True)

    @classmethod
    def tearDownClass(cls):
        cls.directory.cleanup()

    def setUp(self):
        self.tree = tempfile.TemporaryDirectory()
        self.addCleanup(self.tree.cleanup)
        self.root = Path(self.tree.name)
        (self.root / "sys").mkdir()
        (self.root / "dev").mkdir()

    def device(self, name, vendor):
        (self.root / "sys" / name / "device").mkdir(parents=True)
        (self.root / "sys" / name / "device/vendor").write_text(vendor + "\n")
        (self.root / "dev" / name).touch()

    def run_scaler(self, *arguments):
        environment = dict(os.environ, ANY2ODYSSEY_SCALER_SYSFS=str(self.root / "sys"),
                           ANY2ODYSSEY_SCALER_DEV=str(self.root / "dev"))
        return subprocess.run([self.binary, *arguments], env=environment, stdout=subprocess.PIPE,
                              stderr=subprocess.PIPE, text=True, timeout=10)

    def test_the_command_is_the_one_odysseyvk_sends_and_goes_to_the_scaler_alone(self):
        self.device("sg0", "ATA     ")
        self.device("sg3", "GCREADER")
        self.device("sg1", "gcreader")  # found whatever the case, and the first by name is taken
        on = self.run_scaler("on", "--dry")
        self.assertEqual(on.returncode, 0, on.stdout)
        self.assertEqual(on.stdout.splitlines(), [
            str(self.root / "dev/sg1"),
            "command: E9 61 00 00 00 00 00 00 02 00 00 00 00 00 00 00",
            "data (512 bytes): 01 00 00 00 D2 00 00 00 ...",
        ])
        off = self.run_scaler("off", "--dry")
        self.assertEqual(off.stdout.splitlines()[2], "data (512 bytes): 00 00 00 00 D2 00 00 00 ...")

    def test_without_the_monitor_it_says_so_and_sends_nothing(self):
        self.device("sg0", "ATA     ")
        for arguments in (("on",), ("off",), ("on", "--dry")):
            done = self.run_scaler(*arguments)
            self.assertEqual(done.returncode, 1)
            self.assertTrue(done.stdout.startswith("error: the monitor's scaler was not found"), done.stdout)
        # No folder of SCSI devices at all (a kernel without the sg driver).
        shutil.rmtree(self.root / "sys")
        self.assertEqual(self.run_scaler("on").returncode, 1)

    def test_a_device_that_does_not_take_the_command_is_an_error_in_one_line(self):
        self.device("sg2", "GCREADER")  # a plain file: it can be opened and takes no SCSI command
        done = self.run_scaler("on")
        self.assertEqual(done.returncode, 1)
        self.assertEqual(len(done.stdout.splitlines()), 1)
        self.assertTrue(done.stdout.startswith("error: the command could not be sent"), done.stdout)
        (self.root / "dev/sg2").unlink()
        self.assertTrue(self.run_scaler("off").stdout.startswith(f"error: {self.root}/dev/sg2: "))

    def test_anything_but_on_or_off_is_refused_before_a_device_is_looked_for(self):
        self.device("sg2", "GCREADER")
        for arguments in ((), ("ON",), ("on", "off"), ("--dry",), ("on", "--dry", "x"), ("1",)):
            done = self.run_scaler(*arguments)
            self.assertEqual((done.returncode, done.stdout), (2, ""), arguments)


class WeaverScalerTests(unittest.TestCase):
    """any2odyssey-weaver switches the scaler only when asked to, and off again only what it switched on."""

    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        for name in ("build", "tools", "bin"):
            (self.root / name).mkdir()
        shutil.copy(HERE / "any2odyssey-weaver", self.root / "any2odyssey-weaver")
        (self.root / "any2odyssey-weaver").chmod(0o755)
        (self.root / "build/any2odyssey-weaver.exe").touch()
        (self.root / "frame").touch()
        self.program("tools/ensure-stack.sh", "exit 0\n")
        (self.root / "tools/mkedid.py").write_text('import sys\nopen(sys.argv[2], "w").write("0 0 3840 2160\\n")\n')
        self.program("tools/tracking.sh", 'case $1 in\nstart) echo on ;;\nstop) echo stopped >"$ROOT/stopped" ;;\nesac\n')
        self.program("bin/busctl", "exit 1\n")
        # The weaver host notes that the picture came after the scaler was switched.
        self.program("bin/wine", 'echo picture >>"$ROOT/order"\necho on\n')
        self.environment = dict(os.environ, ROOT=str(self.root), PATH=f"{self.root}/bin:{os.environ['PATH']}")
        self.environment.pop("ANY2ODYSSEY_SCALER", None)

    def program(self, name, body):
        path = self.root / name
        path.write_text("#!/bin/sh\n" + body)
        path.chmod(0o755)

    def scaler(self, answer="echo $1"):
        self.program("build/any2odyssey-scaler", f'echo "scaler $1" >>"$ROOT/order"\n{answer}\n')

    def shown(self, **environment):
        """Run the script to its end and return its lines and the order things happened in."""
        for name in ("stopped", "order"):
            (self.root / name).unlink(missing_ok=True)
        done = subprocess.run([str(self.root / "any2odyssey-weaver"), str(self.root / "frame")],
                              env=dict(self.environment, **environment), stdin=subprocess.DEVNULL,
                              stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True, timeout=30)
        deadline = time.monotonic() + 15
        while not (self.root / "stopped").exists():
            self.assertLess(time.monotonic(), deadline, "the leaving was not done")
            time.sleep(0.02)
        time.sleep(0.3)  # the scaler is switched off after tracking is stopped
        order = (self.root / "order").read_text().splitlines() if (self.root / "order").exists() else []
        return done.stdout.splitlines(), order

    def test_nothing_is_sent_unless_asked_for(self):
        self.scaler()
        self.assertEqual(self.shown(), (["on"], ["picture"]))
        self.assertFalse((self.root / "scaler.log").exists())

    def test_asked_for_by_file_or_by_variable_it_is_on_for_the_picture_and_off_after(self):
        self.scaler()
        wanted = (["on"], ["scaler on", "picture", "scaler off"])
        self.assertEqual(self.shown(ANY2ODYSSEY_SCALER="1"), wanted)
        (self.root / "scaler.on").touch()
        self.assertEqual(self.shown(), wanted)
        self.assertEqual((self.root / "scaler.log").read_text(), "on\nleaving: off\n")
        self.assertFalse((self.root / "scaler.sent").exists())

    def test_a_scaler_that_does_not_answer_leaves_the_picture_and_is_not_switched_off(self):
        (self.root / "scaler.on").touch()
        self.scaler('echo "error: the monitor\'s scaler was not found"; exit 1')
        self.assertEqual(self.shown(), (["on"], ["scaler on", "picture"]))
        self.assertEqual((self.root / "scaler.log").read_text(), "error: the monitor's scaler was not found\n")
        # Asked for where setup.sh built none (no C compiler): said in the log, and the picture shows.
        (self.root / "build/any2odyssey-scaler").unlink()
        self.assertEqual(self.shown(), (["on"], ["picture"]))
        self.assertIn("no any2odyssey-scaler", (self.root / "scaler.log").read_text())


if __name__ == "__main__":
    unittest.main()
