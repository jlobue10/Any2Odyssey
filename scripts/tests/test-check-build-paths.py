#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
"""scripts/check-build-paths.sh: what it accepts and what it refuses.

On a release it only ever shows that the program that was just built names no folder of the runner. Here
it is given stand-ins for a program: bytes with a path of each kind in them. A2O_TEST_SHELL runs the script
under another shell (dash, "busybox sh").
"""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[2] / "scripts" / "check-build-paths.sh"
SHELL = os.environ.get("A2O_TEST_SHELL", "sh").split()
# What surrounds a path in a program: bytes that are no text.
BEFORE, AFTER = b"\x00\x01\xff\x7f\x00", b"\x00\x90\x00"


class BuildPaths(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.program = Path(directory.name) / "program"

    def check(self, held, *folders):
        self.program.write_bytes(BEFORE + AFTER.join(text.encode() for text in held) + AFTER)
        return subprocess.run([*SHELL, str(SCRIPT), str(self.program), *folders], stdin=subprocess.DEVNULL,
                              capture_output=True, text=True, timeout=60)

    def test_a_program_with_other_names_for_the_folders_passes(self):
        done = self.check(["/cargo/registry/src/index.crates.io-1949cf8c6b5b557f/egui-0.33.0/src/context.rs",
                           "/any2odyssey/target/release/build/libspa-sys-0f3a/out/bindings.rs",
                           "crates/app/src/main.rs", "/rustc/0123abcd/library/core/src/option.rs",
                           # Not paths of the folders asked about, though they hold their names.
                           "/home/runners/x.rs", "accessible/root/home/runner", "see /home/runner"],
                          "/home/runner", "/home/runner/work/Any2Odyssey/Any2Odyssey")
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertEqual(done.stderr, "")
        self.assertIn("names none of", done.stdout)

    def test_a_path_under_a_folder_of_the_build_machine_is_refused_and_shown(self):
        path = "/home/runner/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/egui-0.33.0/src/context.rs"
        for folders in (["/home/runner"], ["/nowhere", "/home/runner"], ["/home/runner/.cargo", "/nowhere"]):
            with self.subTest(folders=folders):
                done = self.check(["crates/app/src/main.rs", path, path.replace("egui", "winit")], *folders)
                self.assertEqual(done.returncode, 1)
                self.assertIn("2 times", done.stderr)
                self.assertIn(path, done.stderr)

    def test_a_windows_path_is_found_whatever_its_case_and_separator(self):
        for held in (r"C:\Users\runneradmin\.cargo\registry\src\index.crates.io-1949cf8c6b5b557f\egui-0.33.0\src\lib.rs",
                     r"c:\users\RunnerAdmin\.cargo\registry\src\x\lib.rs", "C:/Users/runneradmin/.cargo/registry/lib.rs"):
            with self.subTest(held=held):
                done = self.check([held], r"C:\Users\runneradmin", r"D:\a\Any2Odyssey\Any2Odyssey")
                self.assertEqual(done.returncode, 1, done.stdout)
                # (shown with the forward separator, whichever it had)
                self.assertIn(held.replace("\\", "/"), done.stderr)
        done = self.check([r"cargo\registry\src\index.crates.io-1949cf8c6b5b557f\egui-0.33.0\src\lib.rs",
                           r"any2odyssey\target\release\build\any2odyssey-0f3a\out\icon.rs"],
                          r"C:\Users\runneradmin", r"D:\a\Any2Odyssey\Any2Odyssey")
        self.assertEqual(done.returncode, 0, done.stderr)

    def test_what_cannot_be_checked_is_an_error_of_its_own(self):
        for arguments in ([], [str(self.program)], [str(self.program.parent / "missing"), "/home/runner"],
                          [str(self.program), ""]):
            with self.subTest(arguments=arguments):
                self.program.write_bytes(b"/home/runner/x")
                done = subprocess.run([*SHELL, str(SCRIPT), *arguments], stdin=subprocess.DEVNULL,
                                      capture_output=True, text=True, timeout=60)
                self.assertEqual(done.returncode, 2, done.stderr)


if __name__ == "__main__":
    unittest.main()
