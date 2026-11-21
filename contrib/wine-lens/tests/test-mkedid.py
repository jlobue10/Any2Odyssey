#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""What mkedid.py writes into the registry file for a monitor's EDID: no monitor, Wine or xrandr."""
import importlib.util
import os
from pathlib import Path
import re
import sys
import tempfile
import unittest
import warnings

sys.dont_write_bytecode = True  # loading the tool would leave tools/__pycache__ in the checkout
TOOL = Path(__file__).resolve().parents[1] / "tools/mkedid.py"


def edid(maker, product=0x1234):
    """An EDID with these 16 bits for the maker's three letters, and nothing else in it."""
    data = bytearray(128)
    data[8:10] = maker.to_bytes(2, "big")
    data[10:12] = product.to_bytes(2, "little")
    return bytes(data)


class MkedidTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        # The tool does its work when it is loaded. Let it note the place of no monitors at all, with a
        # stand-in for xrandr that lists none, and keep the module for its functions.
        cls.directory = tempfile.TemporaryDirectory()
        root = Path(cls.directory.name)
        (root / "xrandr").write_text("#!/bin/sh\necho 'Monitors: 0'\n")
        (root / "xrandr").chmod(0o755)
        path, argv = os.environ["PATH"], sys.argv
        os.environ["PATH"] = f"{root}:{path}"
        sys.argv = [str(TOOL), "--place", str(root / "place")]
        try:
            spec = importlib.util.spec_from_file_location("mkedid", TOOL)
            cls.tool = importlib.util.module_from_spec(spec)
            with warnings.catch_warnings():
                warnings.simplefilter("ignore", ResourceWarning)  # it leaves its files to be closed at exit
                spec.loader.exec_module(cls.tool)
        finally:
            os.environ["PATH"], sys.argv = path, argv
        cls.root = root

    @classmethod
    def tearDownClass(cls):
        cls.directory.cleanup()

    def test_a_real_maker_keeps_its_letters(self):
        samsung = (19 << 10) | (1 << 5) | 13  # S, A, M
        self.assertEqual(self.tool.model_of(edid(samsung, 0x7412)), "SAM7412")

    def test_no_maker_gives_a_name_that_can_stand_in_the_registry_file(self):
        for maker in range(0x10000):
            self.assertRegex(self.tool.model_of(edid(maker)), r"\A[@A-Z_]{3}1234\Z")

    def test_a_malformed_maker_does_not_break_the_registry_file(self):
        # 27, 28 and 29 are the three values that used to come out as "[", "\" and "]".
        malformed = edid((27 << 10) | (28 << 5) | 29)
        self.tool.xrandr_monitors = lambda: [("DP-1", 3840, 2160, 0, 0)]
        self.tool.edid_of = lambda connector: malformed
        probe, out = self.root / "probe", self.root / "out.reg"
        probe.write_text("0001 \\\\.\\DISPLAY1 3840x2160 at 0,0 id=[]\n")
        stderr, sys.stderr = sys.stderr, open(os.devnull, "w")
        try:
            with warnings.catch_warnings():
                warnings.simplefilter("ignore", ResourceWarning)
                self.tool.main(str(probe), str(out))
        finally:
            sys.stderr.close()
            sys.stderr = stderr
        lines = out.read_text().split("\n")
        keys = [line.rstrip("\r") for line in lines if line.startswith("[")]
        self.assertTrue(any("___1234" in key for key in keys))
        for key in keys:  # one pair of brackets
            self.assertRegex(key, r"\A\[-?HKEY_LOCAL_MACHINE\\[^\[\]]+\]\Z")
        self.assertIn('"DeviceInstance"="DISPLAY\\\\___1234\\\\0000&0000"', [line.rstrip("\r") for line in lines])

    def run_main(self, monitors, edids, probe_lines, place=False, again=False):
        """main() for these xrandr monitors and EDIDs: the registry file's lines, what it said, the place.

        A first start, with no registry file of a start before it, unless it is one `again`."""
        self.tool.xrandr_monitors = lambda: list(monitors)
        self.tool.edid_of = lambda connector: edids[connector]
        probe, out, noted = self.root / "probe", self.root / "out.reg", self.root / "noted"
        if not again:
            out.unlink(missing_ok=True)
        probe.write_text("".join(probe_lines))
        stderr, sys.stderr = sys.stderr, open(self.root / "said", "w")
        try:
            with warnings.catch_warnings():
                warnings.simplefilter("ignore", ResourceWarning)
                self.tool.main(str(probe), str(out), *([str(noted)] if place else []))
        finally:
            sys.stderr.close()
            sys.stderr = stderr
        said = (self.root / "said").read_text()
        return out.read_text().splitlines(), said,noted.read_text() if place else None

    def test_mirrored_monitors_do_not_stop_the_setup(self):
        # Two monitors showing the same picture are at the same place; Wine shows them as one. This used
        # to end with "slot 0: no single monitor at 3840x2160+0+0", and the stack did not start.
        samsung, other = (19 << 10) | (1 << 5) | 13, (4 << 10) | (5 << 5) | 12
        odyssey, plain = named(edid(samsung, 0x7412), "Odyssey G90XF"), named(edid(other, 0x0001), "Plain")
        probe = ["0001 \\\\.\\DISPLAY1 3840x2160 at 0,0 id=[]\n"]
        # Whichever connector comes first, by name or in xrandr's list, the Odyssey 3D is the one named.
        for first, second in (("DP-1", "HDMI-A-1"), ("HDMI-A-1", "DP-1")):
            edids = {first: odyssey, second: plain}
            for monitors in ([(first, 3840, 2160, 0, 0), (second, 3840, 2160, 0, 0)],
                             [(second, 3840, 2160, 0, 0), (first, 3840, 2160, 0, 0)]):
                lines, said, place = self.run_main(monitors, edids, probe, place=True)
                self.assertEqual(said, f"slot 0: {first} SAM7412\n")
                self.assertEqual(place, "0 0 3840 2160\n")
                self.assertIn('"DeviceInstance"="DISPLAY\\\\SAM7412\\\\0000&0000"', lines)
                # The mirror has no slot of its own in Wine: nothing is written for a second one.
                self.assertFalse([line for line in lines if "0001&0000" in line and not line.startswith("[-")])
                self.assertFalse([line for line in lines if "DEL0001" in line])
        # Neither is the Odyssey 3D: the one with an EDID, then the first by name.
        monitors = [("HDMI-A-1", 1920, 1080, 0, 0), ("DP-1", 1920, 1080, 0, 0)]
        probe = ["0001 \\\\.\\DISPLAY1 1920x1080 at 0,0 id=[]\n"]
        _, said, _ = self.run_main(monitors, {"DP-1": bytes(128), "HDMI-A-1": plain}, probe)
        self.assertEqual(said, "slot 0: HDMI-A-1 DEL0001\n")
        _, said, _ = self.run_main(monitors, {"DP-1": plain, "HDMI-A-1": plain}, probe)
        self.assertEqual(said, "slot 0: DP-1 DEL0001\n")

    def test_mirrored_monitors_that_wine_lists_one_by_one_each_get_a_slot(self):
        samsung, other = (19 << 10) | (1 << 5) | 13, (4 << 10) | (5 << 5) | 12
        edids = {"DP-1": named(edid(other, 0x0001), "Plain"), "DP-2": named(edid(samsung, 0x7412), "Odyssey G90XF")}
        monitors = [("DP-1", 3840, 2160, 0, 0), ("DP-2", 3840, 2160, 0, 0), ("DP-3", 1920, 1080, 3840, 0)]
        edids["DP-3"] = named(edid(other, 0x0002), "Side")
        probe = ["0001 \\\\.\\DISPLAY1 3840x2160 at 0,0 id=[]\n", "0002 \\\\.\\DISPLAY2 3840x2160 at 0,0 id=[]\n",
                 "0003 \\\\.\\DISPLAY3 1920x1080 at 3840,0 id=[]\n"]
        _, said, _ = self.run_main(monitors, edids, probe)
        self.assertEqual(said, "slot 0: DP-2 SAM7412\nslot 1: DP-1 DEL0001\nslot 2: DP-3 DEL0002\n")
        # A mirrored pair beside a monitor Wine lists without a usable name: the slot left over is still its.
        _, said, _ = self.run_main(monitors, edids, probe[:1])
        self.assertEqual(said, "slot 0: DP-2 SAM7412\nslot 1: DP-3 DEL0002\n")

    ENUM = "HKEY_LOCAL_MACHINE\\System\\CurrentControlSet\\Enum\\DISPLAY\\"
    CLASSES = ("HKEY_LOCAL_MACHINE\\System\\CurrentControlSet\\Control\\DeviceClasses\\"
               "{e6f07b5f-ee97-4a90-b076-33f57bf4eaa7}\\##?#DISPLAY#")

    def entries(self, lines):
        """What a registry file does with the monitor entries: ({deleted}, {written}) as "MODEL slot"."""
        deleted, written, linked = set(), set(), set()
        for line in lines:
            entry = re.fullmatch(r"\[(-?)" + re.escape(self.ENUM) + r"([^\\\]]+)\\(\d{4})&0000\]", line)
            if entry and entry.group(2) != "Default_Monitor":
                (deleted if entry.group(1) else written).add(f"{entry.group(2)} {int(entry.group(3))}")
            link = re.fullmatch(r"\[-" + re.escape(self.CLASSES) + r"([^#]+)#(\d{4})&0000#\{[-0-9a-f]+\}\]", line)
            if link:
                linked.add(f"{link.group(1)} {int(link.group(2))}")
        # Every entry goes with its interface (which holds Linked=1), and is deleted before it is written.
        self.assertEqual(linked, deleted)
        self.assertLessEqual(written, deleted)
        for name in written:
            model, slot = name.split()
            key = lambda sign: f"[{sign}{self.ENUM}{model}\\{int(slot):04d}&0000]"
            self.assertLess(lines.index(key("-")), lines.index(key("")))
        return deleted, written

    THREE = [("DP-1", 3840, 2160, 0, 0), ("DP-2", 1920, 1080, 3840, 0), ("DP-3", 1920, 1080, 5760, 0)]
    PROBE = ["0001 \\\\.\\DISPLAY1 3840x2160 at 0,0 id=[]\n", "0002 \\\\.\\DISPLAY2 1920x1080 at 3840,0 id=[]\n",
             "0003 \\\\.\\DISPLAY3 1920x1080 at 5760,0 id=[]\n"]

    def test_a_monitor_fewer_than_at_the_start_before_leaves_no_entry_behind(self):
        # Three monitors of one model, then two: the third one's entry stayed in the prefix with Linked=1,
        # and Leia's service was shown a monitor that was not there.
        same = edid((4 << 10) | (5 << 5) | 12, 0x0001)
        edids = {"DP-1": same, "DP-2": same, "DP-3": same}
        _, written = self.entries(self.run_main(self.THREE, edids, self.PROBE)[0])
        self.assertEqual(written, {"DEL0001 0", "DEL0001 1", "DEL0001 2"})
        deleted, written = self.entries(self.run_main(self.THREE[:2], edids, self.PROBE[:2], again=True)[0])
        self.assertEqual(written, {"DEL0001 0", "DEL0001 1"})
        self.assertIn("DEL0001 2", deleted)

    def test_a_model_that_is_gone_leaves_no_entry_behind(self):
        samsung, other = (19 << 10) | (1 << 5) | 13, (4 << 10) | (5 << 5) | 12
        edids = {"DP-1": edid(samsung, 0x7412), "DP-2": edid(other, 0x0001), "DP-3": edid(other, 0x0002)}
        _, written = self.entries(self.run_main(self.THREE, edids, self.PROBE)[0])
        self.assertEqual(written, {"SAM7412 0", "DEL0001 1", "DEL0002 2"})
        # The second one unplugged: the third moves up a slot, and nothing of the second is kept.
        two = [self.THREE[0], ("DP-3", 1920, 1080, 3840, 0)]
        deleted, written = self.entries(self.run_main(two, edids, self.PROBE[:2], again=True)[0])
        self.assertEqual(written, {"SAM7412 0", "DEL0002 1"})
        self.assertLessEqual({"DEL0001 1", "DEL0002 2", "DEL0002 1", "SAM7412 0"}, deleted)
        # And at the starts after it (the file may not have been imported: Wine did not start, or the start
        # was ended before it), for as long as the model stays away.
        for _ in range(2):
            deleted, written = self.entries(self.run_main(two, edids, self.PROBE[:2], again=True)[0])
            self.assertEqual(written, {"SAM7412 0", "DEL0002 1"})
            self.assertLessEqual({"DEL0001 1", "DEL0002 2"}, deleted)
        # Plugged in again, it has its entry again.
        _, written = self.entries(self.run_main(self.THREE, edids, self.PROBE, again=True)[0])
        self.assertEqual(written, {"SAM7412 0", "DEL0001 1", "DEL0002 2"})

    def test_slots_no_start_named_are_cleared_too(self):
        # An installation from before this was kept track of: what an earlier layout left in higher slots of
        # a model that is connected goes, without any file naming it.
        deleted, written = self.entries(self.run_main(self.THREE[:1], {"DP-1": edid(1)}, self.PROBE[:1])[0])
        self.assertEqual(written, {"@@A1234 0"})
        self.assertEqual(deleted, {f"@@A1234 {slot}" for slot in range(16)})

    def test_only_monitor_entries_are_taken_from_the_file_of_the_start_before(self):
        # The file is the user's own, but what is deleted in Wine's registry is never read out of it as
        # written: a model as an EDID gives it, a slot number, and nothing else. (tools/start-stack.sh adds
        # other entries to the file, and a start that was ended can leave any part of it.)
        out = self.root / "out.reg"
        lines = ["REGEDIT4", "", f"[{self.ENUM}DEL0001\\0020&0000]", f"[-{self.ENUM}ABC00FF\\0003&0000]",
                 f"[{self.ENUM}Default_Monitor\\0005&0000]", f"[{self.ENUM}DEL0001\\9999&0000]", f"[{self.ENUM}..\\..\\Services\\0001&0000]",
                 f"[{self.ENUM}DEL0001\\0021&0000\\Device Parameters]", f"[-{self.ENUM}del0001\\0001&0000]",
                 f" [{self.ENUM}XYZ0001\\0001&0000]", f"[{self.ENUM}XYZ0002\\0001&0000] ",
                 "[HKEY_LOCAL_MACHINE\\Software\\Microsoft\\WindowsRuntime\\ActivatableClassId\\A.Class]",
                 "\x00\xff not a registry file at all"]
        out.write_bytes("\r\n".join(lines).encode("latin-1"))
        deleted, written = self.entries(self.run_main(self.THREE[:1], {"DP-1": edid(1)}, self.PROBE[:1], again=True)[0])
        self.assertEqual(written, {"@@A1234 0"})
        self.assertEqual(deleted, {f"{model} {slot}" for model in ("@@A1234", "DEL0001", "ABC00FF") for slot in range(21)})
        # No file: a first start.
        out.unlink()
        deleted, _ = self.entries(self.run_main(self.THREE[:1], {"DP-1": edid(1)}, self.PROBE[:1])[0])
        self.assertEqual(len(deleted), 16)

    def test_a_slot_where_no_monitor_is_says_so(self):
        with self.assertRaises(SystemExit) as stopped:
            self.run_main([("DP-1", 3840, 2160, 0, 0)], {"DP-1": edid(1)},
                          ["0001 \\\\.\\DISPLAY1 1920x1080 at 0,0 id=[]\n"])
        self.assertEqual(stopped.exception.code, "slot 0: no monitor at 1920x1080+0+0")


def named(data, name):
    """This EDID with a monitor name descriptor (0xFC) in its first descriptor block."""
    data = bytearray(data)
    data[54:72] = b"\0\0\0\xfc\0" + (name.encode() + b"\n").ljust(13, b" ")[:13]
    return bytes(data)


if __name__ == "__main__":
    unittest.main()
