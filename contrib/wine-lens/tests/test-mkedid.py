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


if __name__ == "__main__":
    unittest.main()
