#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""The scaler's switch against a stand-in for sysfs, and any2odyssey-weaver's use of it: no monitor, Wine or root."""
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import time
import unittest

HERE = Path(__file__).resolve().parents[1]
# The USB device the monitor's scaler is (read from one monitor).
SCALER_USB = "1603:452a"


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
        (self.root / "sys/class/scsi_generic").mkdir(parents=True)
        (self.root / "dev").mkdir()

    def device(self, name, vendor, usb=SCALER_USB, port="1-4.1"):
        """A SCSI device as sysfs has one: the name under class/scsi_generic is a link to a folder deep under
        devices, whose `device` is a link to the SCSI device three folders up. On USB (`usb` is its id), that
        is under the USB device's folder, which has the two numbers, under a hub's, which has its own."""
        number = name[2:]
        if usb:
            hub = self.root / "sys/devices/pci0000:00/0000:00:14.0/usb1/1-4"
            hub.mkdir(parents=True, exist_ok=True)
            (hub / "idVendor").write_text("05e3\n")
            (hub / "idProduct").write_text("0608\n")
            (hub / port).mkdir(exist_ok=True)
            (hub / port / "idVendor").write_text(usb.split(":")[0] + "\n")
            (hub / port / "idProduct").write_text(usb.split(":")[1] + "\n")
            host = hub / port / f"{port}:1.0" / f"host{number}"
        else:
            host = self.root / "sys/devices/pci0000:00/0000:00:17.0/ata1" / f"host{number}"
        scsi = host / f"target{number}:0:0" / f"{number}:0:0:0"
        (scsi / "scsi_generic" / name).mkdir(parents=True)
        (scsi / "vendor").write_text(vendor + "\n")
        (scsi / "scsi_generic" / name / "device").symlink_to("../../../" + scsi.name)
        (self.root / "sys/class/scsi_generic" / name).symlink_to(
            os.path.relpath(scsi / "scsi_generic" / name, self.root / "sys/class/scsi_generic"))
        (self.root / "dev" / name).touch()

    def run_scaler(self, *arguments):
        environment = dict(os.environ, ANY2ODYSSEY_SCALER_SYSFS=str(self.root / "sys/class/scsi_generic"),
                           ANY2ODYSSEY_SCALER_DEV=str(self.root / "dev"))
        return subprocess.run([self.binary, *arguments], env=environment, stdout=subprocess.PIPE,
                              stderr=subprocess.PIPE, text=True, timeout=10)

    def test_the_command_is_the_one_odysseyvk_sends_and_goes_to_the_scaler_alone(self):
        self.device("sg0", "ATA     ", usb=None)
        self.device("sg3", "GCREADER")
        self.device("sg1", "gcreader", port="1-4.3")  # found whatever the case, and the first by name is taken
        on = self.run_scaler("on", "--dry")
        self.assertEqual(on.returncode, 0, on.stdout)
        self.assertEqual(on.stdout.splitlines(), [
            str(self.root / "dev/sg1"),
            "usb: 1603:452a",
            "command: E9 61 00 00 00 00 00 00 02 00 00 00 00 00 00 00",
            "data (512 bytes): 01 00 00 00 D2 00 00 00 ...",
        ])
        off = self.run_scaler("off", "--dry")
        self.assertEqual(off.stdout.splitlines()[3], "data (512 bytes): 00 00 00 00 D2 00 00 00 ...")

    def test_without_the_monitor_it_says_so_and_sends_nothing(self):
        self.device("sg0", "ATA     ", usb=None)
        for arguments in (("on",), ("off",), ("on", "--dry")):
            done = self.run_scaler(*arguments)
            self.assertEqual(done.returncode, 1)
            self.assertTrue(done.stdout.startswith("error: the monitor's scaler was not found"), done.stdout)

    def test_without_the_sg_driver_it_names_the_driver_and_not_the_cable(self):
        # No folder of sg devices at all: the kernel's sg driver is not loaded (seen on CachyOS, with the
        # monitor connected and its device there as a disk).
        shutil.rmtree(self.root / "sys")
        for arguments in (("on",), ("off",), ("on", "--dry")):
            done = self.run_scaler(*arguments)
            self.assertEqual(done.returncode, 1)
            self.assertTrue(done.stdout.startswith("error: the kernel's SCSI generic driver (sg) is not loaded"),
                            done.stdout)
            self.assertIn("modprobe sg", done.stdout)
            self.assertNotIn("cable", done.stdout)
            self.assertEqual(len(done.stdout.splitlines()), 1, done.stdout)

    def test_a_device_of_the_name_that_is_another_usb_device_gets_nothing_and_is_named(self):
        # Another maker's card reader that calls itself the same: what E9 means to it is not known.
        self.device("sg2", "GCREADER", usb="05e3:0749")
        for arguments in (("on",), ("off",), ("on", "--dry")):
            done = self.run_scaler(*arguments)
            self.assertEqual(done.returncode, 1, done.stdout)
            lines = done.stdout.splitlines()
            self.assertEqual(len(lines), 1, done.stdout)
            self.assertTrue(lines[0].startswith("error: a device of the vendor GCREADER is not the scaler"), lines)
            self.assertIn("sg2 is USB 05e3:0749", lines[0])
            self.assertIn("known here is USB 1603:452a (read from one monitor); nothing was sent", lines[0])
            self.assertIn("please report that number", lines[0])
        # Only one of the two numbers is the scaler's: still another device.
        self.device("sg4", "GCREADER", usb="1603:0001", port="1-4.2")
        self.device("sg5", "GCREADER", usb="0001:452a", port="1-4.3")
        done = self.run_scaler("on")
        self.assertEqual(done.returncode, 1, done.stdout)
        self.assertIn("sg2 is USB 05e3:0749", done.stdout)

    def test_a_device_of_the_name_that_is_not_on_usb_gets_nothing(self):
        self.device("sg6", "GCREADER", usb=None)
        done = self.run_scaler("on")
        self.assertEqual(done.returncode, 1, done.stdout)
        self.assertEqual(len(done.stdout.splitlines()), 1, done.stdout)
        self.assertIn("sg6 is not a USB device", done.stdout)
        self.assertIn("nothing was sent", done.stdout)
        # Nor one whose USB device says something that is no id (more than four digits): the hub above
        # it has an id, and that is the hub's, not this device's.
        self.device("sg7", "GCREADER", usb="1603:452a", port="1-4.2")
        (self.root / "sys/devices/pci0000:00/0000:00:14.0/usb1/1-4/1-4.2/idProduct").write_text("452a0\n")
        (self.root / "sys/class/scsi_generic/sg6").unlink()
        done = self.run_scaler("on")
        self.assertEqual(done.returncode, 1, done.stdout)
        self.assertIn("sg7 is not a USB device", done.stdout)

    def test_the_scaler_is_taken_among_others_of_its_name_and_the_first_of_two(self):
        self.device("sg1", "GCREADER", usb="05e3:0749", port="1-4.3")
        self.device("sg5", "GCREADER", port="1-4.2")
        self.device("sg3", "GCREADER", port="1-4.1")
        self.assertEqual(self.run_scaler("on", "--dry").stdout.splitlines()[0], str(self.root / "dev/sg3"))
        # The numbers in capitals, as sysfs does not write them, are the same numbers.
        (self.root / "sys/devices/pci0000:00/0000:00:14.0/usb1/1-4/1-4.1/idProduct").write_text("452A\n")
        self.assertEqual(self.run_scaler("off", "--dry").stdout.splitlines()[0], str(self.root / "dev/sg3"))

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


class RuleTests(unittest.TestCase):
    """The udev rule opens the device any2odyssey-scaler sends to, and no other."""

    def setUp(self):
        text = (HERE / "71-any2odyssey-scaler.rules").read_text()
        self.rules = [line for line in text.splitlines() if line and not line.startswith("#")]

    def test_the_rule_asks_for_the_usb_id_the_program_asks_for(self):
        source = (HERE / "src/scaler.c").read_text()
        self.assertEqual(re.findall(r'#define USB_ID "([0-9a-f]{4}:[0-9a-f]{4})"', source), [SCALER_USB])
        vendor, product = SCALER_USB.split(":")
        # (the line that notes the id for the permission, and the one that loads the sg driver)
        usb = [line for line in self.rules if "idVendor" in line]
        self.assertEqual(len(usb), 2, self.rules)
        for line in usb:
            self.assertIn(f'ATTRS{{idVendor}}=="{vendor}"', line)
            self.assertIn(f'ATTRS{{idProduct}}=="{product}"', line)

    def test_no_line_asks_two_devices_above_for_what_only_one_of_them_has(self):
        # udev wants every ATTRS and SUBSYSTEMS of a line to be true of the same device above: the USB id
        # and the SCSI vendor name in one line match nothing, and the rule would open nothing.
        for line in self.rules:
            self.assertFalse("ATTRS{vendor}" in line and "ATTRS{idVendor}" in line, line)
            self.assertEqual(len(re.findall(r"SUBSYSTEMS==", line)), 1, line)
            self.assertTrue(line.startswith(('SUBSYSTEM=="scsi_generic", ', 'ACTION=="add", SUBSYSTEM=="scsi", ')), line)

    def test_the_tag_is_given_only_to_a_device_with_the_id_and_the_name(self):
        noted = [line for line in self.rules if line.endswith(', ENV{.A2O_SCALER_USB}="1"')]
        tagged = [line for line in self.rules if "uaccess" in line]
        self.assertEqual((len(noted), len(tagged), len(self.rules)), (1, 1, 3), self.rules)
        self.assertLess(self.rules.index(noted[0]), self.rules.index(tagged[0]))
        self.assertIn('ENV{.A2O_SCALER_USB}=="1"', tagged[0])
        self.assertIn('ATTRS{vendor}=="GCREADER*"', tagged[0])
        self.assertTrue(tagged[0].endswith(', TAG+="uaccess"'), tagged[0])
        # Nothing but the tag: no mode for everybody, no group, no program run (the one RUN is udev's own
        # loading of a kernel driver, in the test below).
        for line in self.rules:
            self.assertIsNone(re.search(r"MODE|GROUP|OWNER|PROGRAM|IMPORT", line), line)
            self.assertEqual("RUN" in line, line not in noted + tagged, line)

    def test_the_sg_driver_is_loaded_for_the_scaler_and_for_nothing_else(self):
        # Without the driver there is no /dev/sgN for the other two lines to open (Arch does not load it by
        # itself). It is loaded by udev itself, when the SCSI device with the name on the USB device with the
        # id appears: the name is that device's own (ATTR), the id the one device above's (ATTRS).
        loads = [line for line in self.rules if "RUN" in line]
        self.assertEqual(len(loads), 1, self.rules)
        vendor, product = SCALER_USB.split(":")
        self.assertEqual(loads[0], 'ACTION=="add", SUBSYSTEM=="scsi", ENV{DEVTYPE}=="scsi_device", '
                         'ATTR{vendor}=="GCREADER*", SUBSYSTEMS=="usb", '
                         f'ATTRS{{idVendor}}=="{vendor}", ATTRS{{idProduct}}=="{product}", '
                         'RUN{builtin}+="kmod load sg"')
        self.assertNotIn("uaccess", loads[0])

    def test_udev_reads_the_rule_without_a_complaint(self):
        # (udevadm verify came with systemd 254; it reads the file and touches no device)
        if not shutil.which("udevadm"):
            self.skipTest("no udevadm")
        if subprocess.run(["udevadm", "verify", "--help"], capture_output=True).returncode != 0:
            self.skipTest("this udevadm has no verify")
        done = subprocess.run(["udevadm", "verify", "--resolve-names=never", "--no-summary",
                               str(HERE / "71-any2odyssey-scaler.rules")], capture_output=True, text=True)
        # (what it says about the file's permissions in a checkout is not about the rule)
        self.assertEqual(done.returncode, 0, done.stdout + done.stderr)


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
        # A note of a mode switched on, left by a run whose leaving never happened (the PC lost power): not
        # this run's to act on: its leaving sent "off" with nothing asked for.
        (self.root / "scaler.sent").touch()
        self.assertEqual(self.shown(), (["on"], ["picture"]))
        self.assertFalse((self.root / "scaler.sent").exists())
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
