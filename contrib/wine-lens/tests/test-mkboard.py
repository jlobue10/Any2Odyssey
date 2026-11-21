#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""What mkboard.py writes for a lens board, and what it refuses: no lens board, Wine or root.

The tool reads the board's USB descriptors and strings from sysfs. Here a folder stands in for "/", with the
entries of a USB serial device under its sys, as the kernel lays them out (A2O_MKBOARD_TEST_ROOT names it).
"""
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

TOOL = Path(__file__).resolve().parents[1] / "tools/mkboard.py"
# A device descriptor (18 bytes: USB 2.0, class 2, 354b:0116, device release 1.02, strings 1, 2 and 3) and a
# configuration of 32 bytes in all: its header, an interface and two endpoints.
DEVICE = bytes([18, 1, 0x00, 0x02, 2, 0, 0, 64, 0x4B, 0x35, 0x16, 0x01, 0x02, 0x01, 1, 2, 3, 1])
CONFIG = (bytes([9, 2, 32, 0, 1, 1, 0, 0x80, 50]) + bytes([9, 4, 0, 0, 2, 2, 2, 1, 0])
          + bytes([7, 5, 0x81, 2, 64, 0, 0]) + bytes([7, 5, 0x02, 2, 64, 0, 0]))
INVALID = "incomplete or invalid lens board USB descriptors"


class MkboardTests(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        # (as the tool sees it: it follows the links of sysfs)
        self.root = Path(directory.name).resolve()
        # The board on port 1-2, its serial port function, and the tty the kernel made for it.
        self.board = self.root / "sys/devices/pci0000:00/0000:00:14.0/usb1/1-2"
        (self.board / "1-2:1.0/tty/ttyACM0").mkdir(parents=True)
        (self.board / "1-2:1.0/tty/ttyACM0/device").symlink_to("../../../1-2:1.0")
        (self.root / "sys/class/tty").mkdir(parents=True)
        (self.root / "sys/class/tty/ttyACM0").symlink_to("../../devices/pci0000:00/0000:00:14.0/usb1/1-2/1-2:1.0/tty/ttyACM0")
        self.describe(idVendor="354b", idProduct="0116", serial="A2O0001", devnum="7", manufacturer="A maker",
                      product="A lens board")
        (self.board / "descriptors").write_bytes(DEVICE + CONFIG)
        # The device file, and the name the udev rule gives it.
        (self.root / "dev").mkdir()
        (self.root / "dev/ttyACM0").touch()
        (self.root / "dev/odyssey-lens").symlink_to("ttyACM0")
        self.tty = str(self.root / "dev/odyssey-lens")
        self.header, self.registry = self.root / "board.h", self.root / "device.reg"

    def describe(self, **entries):
        for name, text in entries.items():
            (self.board / name).write_text(text + "\n")

    def run_tool(self, tty=None):
        environment = dict(os.environ, A2O_MKBOARD_TEST_ROOT=str(self.root), PYTHONDONTWRITEBYTECODE="1")
        return subprocess.run([sys.executable, str(TOOL), tty or self.tty, str(self.header), str(self.registry)],
                              env=environment, stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=30)

    def refused(self, reason, tty=None):
        """The tool ends with this reason, one line of it, and has written neither file."""
        done = self.run_tool(tty)
        self.assertEqual((done.returncode, done.stdout), (1, ""), done.stderr)
        self.assertEqual(len(done.stderr.splitlines()), 1, done.stderr)
        self.assertIn(reason, done.stderr)
        self.assertFalse(self.header.exists(), "board.h was written")
        self.assertFalse(self.registry.exists(), "device.reg was written")

    def test_a_lens_board_is_described_from_its_own_descriptors(self):
        done = self.run_tool()
        self.assertEqual((done.returncode, done.stdout, done.stderr), (0, "", ""))
        header = self.header.read_text().splitlines()
        numbers = lambda data: "{" + ",".join(str(byte) for byte in data) + "}"
        self.assertEqual(header[1:], [
            f"static const unsigned char board_device_desc[18] = {numbers(DEVICE)};",
            f"static const unsigned char board_config_desc[32] = {numbers(CONFIG)};",
            'static const WCHAR *const board_strings[4] = {0, L"A maker", L"A lens board", L"A2O0001"};',
            "static const unsigned char board_string_index[3] = {1,2,3};",
            "#define BOARD_ADDRESS 7",
        ])
        written = self.registry.read_bytes().decode()
        self.assertNotIn("\n", written.replace("\r\n", ""), "the registry file has Windows line ends")
        lines = written.split("\r\n")
        self.assertEqual(lines[0], "REGEDIT4")
        enum = "HKEY_LOCAL_MACHINE\\System\\CurrentControlSet\\Enum\\"
        # The serial port under the name it was asked with, the board under its serial number (removed
        # before it is written, whatever an earlier board left there), and its function on COM3.
        self.assertIn(f'"COM3"="{self.tty}"', lines)
        parent = f"{enum}USB\\VID_354B&PID_0116\\A2O0001"
        self.assertLess(lines.index(f"[-{parent}]"), lines.index(f"[{parent}]"))
        self.assertIn(f"[{enum}USB\\VID_354B&PID_0116&MI_00\\6&a2f0e15&0&0000\\Device Parameters]", lines)
        self.assertIn('"PortName"="COM3"', lines)
        # The device release of the descriptor (1.02), in the hardware ids: "USB\VID_354B&PID_0116&REV_0102".
        ids = bytes.fromhex(next(line for line in lines if line.startswith('"HardwareID"=hex(7):'))
                            .split(":", 1)[1].replace(",", "")).decode().split("\0")
        self.assertEqual(ids[:2], ["USB\\VID_354B&PID_0116&REV_0102", "USB\\VID_354B&PID_0116"])
        # One pair of brackets a key, and every key in one of the two roots.
        for key in (line for line in lines if line.startswith("[")):
            self.assertRegex(key, r"\A\[-?HKEY_(LOCAL_MACHINE|CURRENT_USER)\\[^\[\]]+\]\Z")

    def test_only_the_first_configuration_is_taken(self):
        # sysfs gives every configuration, one after the other; the stand-in driver answers with the first.
        (self.board / "descriptors").write_bytes(DEVICE + CONFIG + bytes([9, 2, 9, 0, 0, 2, 0, 0x80, 50]))
        self.assertEqual(self.run_tool().returncode, 0)
        self.assertIn("board_config_desc[32] = {9,2,32,0,", self.header.read_text())

    def test_the_device_file_itself_does_as_well_as_the_link_to_it(self):
        done = self.run_tool(str(self.root / "dev/ttyACM0"))
        self.assertEqual((done.returncode, done.stderr), (0, ""))
        self.assertIn(f'"COM3"="{self.root}/dev/ttyACM0"', self.registry.read_bytes().decode().split("\r\n"))

    def test_descriptors_that_are_incomplete_or_invalid_leave_nothing_written(self):
        # The stand-in driver hands out 18 bytes of the first array and reads the configuration's header:
        # a board.h made from less than that was compiled into the driver as it came.
        whole = DEVICE + CONFIG
        cases = {
            "nothing": b"",
            "a device descriptor cut short": whole[:17],
            "a device descriptor and nothing more": whole[:18],
            "a configuration header cut short": whole[:23],
            "a configuration cut short by a byte": whole[:-1],
            "a configuration of which the header is all there is": whole[:27],
            "a device descriptor of another length": bytes([17]) + whole[1:],
            "not a device descriptor": whole[:1] + bytes([2]) + whole[2:],
            "a configuration header of another length": whole[:18] + bytes([8]) + whole[19:],
            "not a configuration": whole[:19] + bytes([4]) + whole[20:],
            "a configuration shorter than its header": whole[:20] + bytes([8, 0]) + whole[22:],
            "a configuration longer than what is there": whole[:20] + bytes([33, 0]) + whole[22:],
            "a configuration far longer than what is there": whole[:20] + bytes([0xFF, 0xFF]) + whole[22:],
        }
        for name, descriptors in cases.items():
            with self.subTest(descriptors=name):
                (self.board / "descriptors").write_bytes(descriptors)
                self.refused(INVALID)

    def test_invalid_descriptors_leave_the_files_of_an_earlier_set_up_as_they_were(self):
        self.assertEqual(self.run_tool().returncode, 0)
        before = (self.header.read_bytes(), self.registry.read_bytes())
        (self.board / "descriptors").write_bytes((DEVICE + CONFIG)[:-1])
        done = self.run_tool()
        self.assertEqual(done.returncode, 1)
        self.assertIn(INVALID, done.stderr)
        self.assertEqual((self.header.read_bytes(), self.registry.read_bytes()), before)

    def test_another_usb_serial_device_is_not_the_lens_board(self):
        for entries in ({"idVendor": "04e8"}, {"idProduct": "0117"}, {"idVendor": "0116", "idProduct": "354b"}):
            with self.subTest(entries=entries):
                self.describe(idVendor="354b", idProduct="0116")
                self.describe(**entries)
                self.refused("is not the Odyssey 3D lens board (354b:0116)")

    def test_a_device_that_is_not_a_usb_serial_port_is_refused(self):
        # A serial port of the main board has no USB device above it; and a name sysfs does not know.
        (self.root / "sys/devices/platform/serial8250/tty/ttyS0").mkdir(parents=True)
        (self.root / "sys/devices/platform/serial8250/tty/ttyS0/device").symlink_to("../../../serial8250")
        (self.root / "sys/class/tty/ttyS0").symlink_to("../../devices/platform/serial8250/tty/ttyS0")
        (self.root / "dev/ttyS0").touch()
        self.refused("/dev/ttyS0 is not a USB serial device", str(self.root / "dev/ttyS0"))
        self.refused("/dev/nothing is not a USB serial device", str(self.root / "dev/nothing"))

    def test_strings_that_could_end_a_literal_or_a_key_leave_nothing_written(self):
        # They go into C string literals and into the registry file, the serial number into a key's name.
        for name in ("manufacturer", "product", "serial"):
            for text in ('A "maker"', "A\\maker", "A [maker", "A maker]", "A\x07maker", "A\nmaker", "A\x7fmaker"):
                with self.subTest(name=name, text=text):
                    self.describe(manufacturer="A maker", product="A lens board", serial="A2O0001")
                    self.describe(**{name: text})
                    self.refused("unexpected characters in the board's strings")
        # What a maker may well put there is taken.
        self.describe(manufacturer="Maker & Söhne, Ltd.", product="Lens board (rev. 2)", serial="A2O-0001_b")
        done = self.run_tool()
        self.assertEqual((done.returncode, done.stderr), (0, ""))
        self.assertIn('L"Maker & Söhne, Ltd.", L"Lens board (rev. 2)", L"A2O-0001_b"', self.header.read_text())


if __name__ == "__main__":
    unittest.main()
