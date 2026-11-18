#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
"""scripts/check-doc-numbers.sh: what it accepts and what it refuses.

On the checkout it only ever shows that today's documents agree with the sources. Here it is also run on
copies in which one of them is left behind, as the release text was in 0.9.5 and 0.9.6 (glibc 2.39 and
ONNX Runtime 1.22, where the program needed 2.35 and 1.17), and on copies in which the sources move and
the documents stay. A2O_TEST_SHELL runs the script under another shell (dash, "busybox sh").
"""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

REPO = Path(__file__).resolve().parents[2]
SCRIPT = REPO / "scripts" / "check-doc-numbers.sh"
SHELL = os.environ.get("A2O_TEST_SHELL", "sh").split()
FILES = ["README.md", "packaging/linux/INSTALL.txt", "docs/PLATFORMS.md", ".github/workflows/release.yml",
         "packaging/linux/install.sh", "crates/app/Cargo.toml"]


class DocNumbers(unittest.TestCase):
    def setUp(self):
        self.tree = Path(tempfile.mkdtemp(prefix="a2o-doc-numbers-"))
        self.addCleanup(shutil.rmtree, self.tree, True)
        for name in FILES:
            (self.tree / name).parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(REPO / name, self.tree / name)

    def check(self):
        return subprocess.run([*SHELL, str(SCRIPT), str(self.tree)], capture_output=True, text=True)

    def change(self, name, old, new):
        path = self.tree / name
        text = path.read_text(encoding="utf-8")
        self.assertIn(old, text, f"{name} no longer has {old!r}; change this test with it")
        path.write_bytes(text.replace(old, new).encode("utf-8"))

    def numbers(self):
        """The two numbers as the checkout has them today."""
        said = self.check()
        self.assertEqual(said.returncode, 0, said.stdout + said.stderr)
        words = said.stdout.split()
        return words[words.index("glibc") + 1], words[words.index("Runtime") + 1].rstrip(",")

    def test_the_checkout_agrees_with_itself(self):
        said = self.check()
        self.assertEqual(said.returncode, 0, said.stdout + said.stderr)
        self.assertIn("check-doc-numbers: glibc 2.", said.stdout)

    def test_a_release_text_left_at_the_old_c_library_is_refused(self):
        glibc, _ = self.numbers()
        self.change(".github/workflows/release.yml", f"needs glibc {glibc} or newer", "needs glibc 2.39 or newer")
        said = self.check()
        self.assertEqual(said.returncode, 1, said.stdout + said.stderr)
        self.assertIn('release.yml says "glibc 2.39 or newer"', said.stdout)
        self.assertNotIn("README.md", said.stdout)

    def test_a_release_text_left_at_the_old_onnx_runtime_is_refused(self):
        _, ort = self.numbers()
        self.change(".github/workflows/release.yml", f"ONNX Runtime {ort} or newer", "ONNX Runtime 1.22 or newer")
        said = self.check()
        self.assertEqual(said.returncode, 1, said.stdout + said.stderr)
        self.assertIn('release.yml says "ONNX Runtime 1.22 or newer"', said.stdout)

    def test_a_number_that_moves_in_the_sources_names_every_document_left_behind(self):
        glibc, ort = self.numbers()
        self.change("packaging/linux/install.sh", f"GLIBC_MIN={glibc}", "GLIBC_MIN=2.31")
        self.change("crates/app/Cargo.toml", f'"api-{ort.split(".")[1]}"', '"api-19"')
        said = self.check()
        self.assertEqual(said.returncode, 1, said.stdout + said.stderr)
        for name in ("README.md", "packaging/linux/INSTALL.txt", "docs/PLATFORMS.md", ".github/workflows/release.yml"):
            self.assertIn(f"{name} says", said.stdout)
        self.assertIn("GLIBC_MIN=2.31", said.stdout)
        self.assertIn("built for 1.19", said.stdout)

    def test_the_comma_of_install_txt_is_read_too(self):
        _, ort = self.numbers()
        self.change("packaging/linux/INSTALL.txt", f"ONNX Runtime, {ort} or newer", "ONNX Runtime, 1.22 or newer")
        said = self.check()
        self.assertEqual(said.returncode, 1, said.stdout + said.stderr)
        self.assertIn('INSTALL.txt says "ONNX Runtime 1.22 or newer"', said.stdout)

    def test_a_document_that_stops_saying_the_number_is_refused(self):
        glibc, _ = self.numbers()
        self.change("README.md", f"glibc {glibc} or newer", "a recent glibc")
        said = self.check()
        self.assertEqual(said.returncode, 1, said.stdout + said.stderr)
        self.assertIn(f'README.md does not say "glibc {glibc} or newer"', said.stdout)

    def test_sources_without_the_numbers_are_an_error_not_a_pass(self):
        glibc, _ = self.numbers()
        self.change("packaging/linux/install.sh", f"GLIBC_MIN={glibc}", "GLIBC_OLDEST=x")
        said = self.check()
        self.assertEqual(said.returncode, 1, said.stdout + said.stderr)
        self.assertIn("no GLIBC_MIN", said.stderr)


if __name__ == "__main__":
    unittest.main(verbosity=2)
