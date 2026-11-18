#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
"""scripts/third-party-linux.py against a stand-in for cargo: what its check of THIRD-PARTY-LICENSES.md refuses.

That file is written by hand, and the script compares its two tables and the number in its text with the
crates of a Windows build before it writes the Linux list. In CI the comparison only ever shows that today's
file is accepted. Here a `cargo` that prints a fixed tree stands in, and the refusals are run: a crate that is
missing from the table, one too many, a license that is not the manifest's, a wrong number, and a crate that
a table names twice (which nothing noticed up to 0.9.6: the rows were counted after the doubles were dropped).
"""
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / "third-party-linux.py"

TREE = """any2odyssey v0.0.0 (/somewhere/crates/app)|GPL-3.0-only
alpha v1.0.0|MIT
beta v2.0.0|MIT OR Apache-2.0
alpha v1.0.0|MIT (*)
"""

CRATES = ["| `alpha` | 1.0.0 | MIT |", "| `beta` | 2.0.0 | MIT OR Apache-2.0 |"]
NOTICES = ["| `alpha` 1.0.0 | MIT | `Copyright (c) 2020 A` |", "| `beta` 2.0.0 | MIT | `Copyright (c) 2021 B` |"]


# The toolchain the stand-in's rust-toolchain.toml names. The script also stops unless the list has a section
# on the standard library of that toolchain, which is in every program and in no `cargo tree`.
TOOLCHAIN = "1.0.0"


def document(crates=CRATES, notices=NOTICES, number=2, library=TOOLCHAIN):
    section = [] if library is None else [
        "## The Rust standard library", "",
        f"Both editions are built with Rust {library} (`rust-toolchain.toml`).", ""]
    return "\n".join([
        "# Third-party licenses", "",
        f"The {number} crates below are the dependencies.", "",
        "| Crate | Version | License |", "| --- | --- | --- |", *crates, "",
        "| Crate and version | Used under | Copyright notices |", "| --- | --- | --- |", *notices, "",
        *section])


@unittest.skipIf(os.name == "nt", "the stand-in for cargo is a shell script")
class ThirdPartyList(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.folder = Path(self.temp.name)
        (self.folder / "bin").mkdir()
        cargo = self.folder / "bin" / "cargo"
        cargo.write_text('#!/bin/sh\n[ "$1" = tree ] || exit 2\ncat "$STAND_IN_TREE"\n')
        cargo.chmod(0o755)
        (self.folder / "tree").write_text(TREE)
        (self.folder / "rust-toolchain.toml").write_text(f'[toolchain]\nchannel = "{TOOLCHAIN}"\n')

    def tearDown(self):
        self.temp.cleanup()

    def check(self, text):
        (self.folder / "THIRD-PARTY-LICENSES.md").write_text(text, encoding="utf-8")
        env = dict(os.environ, PATH=f"{self.folder / 'bin'}{os.pathsep}{os.environ['PATH']}",
                   STAND_IN_TREE=str(self.folder / "tree"))
        return subprocess.run([sys.executable, str(SCRIPT)], cwd=self.folder, env=env, capture_output=True,
                              text=True, timeout=60)

    def refused(self, text, *reasons):
        result = self.check(text)
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertIn("THIRD-PARTY-LICENSES.md does not match", result.stderr)
        for reason in reasons:
            self.assertIn(reason, result.stderr)
        # Nothing of the Linux list is written when the Windows list is wrong.
        self.assertEqual(result.stdout, "")

    def test_a_list_that_matches_is_accepted_and_the_linux_list_is_written(self):
        result = self.check(document())
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        # The stand-in's tree is the same for both targets: Linux has nothing beyond it.
        self.assertIn("the 0 crates below", result.stdout)

    def test_the_standard_library_of_another_toolchain_or_of_none_is_refused(self):
        # The section is written by hand from one toolchain's own lock file: when rust-toolchain.toml moves
        # on, the list has to be looked at again, and a list without the section names no standard library.
        for text, reason in ((document(library="0.9.0"), "describes the standard library of Rust 0.9.0"),
                             (document(library=None), 'has no section "## The Rust standard library"')):
            result = self.check(text)
            self.assertNotEqual(result.returncode, 0, result.stdout)
            self.assertIn(reason, result.stderr)
            self.assertEqual(result.stdout, "")

    def test_a_crate_named_twice_is_refused(self):
        self.refused(document(crates=CRATES + CRATES[:1]), "alpha 1.0.0 is in the table of crates twice")
        self.refused(document(notices=NOTICES + NOTICES[1:]), "beta 2.0.0 is in the table of notices twice")
        # Also when the number in the text counts the double.
        self.refused(document(crates=CRATES + CRATES[:1], number=3), "alpha 1.0.0 is in the table of crates twice")

    def test_what_differs_from_the_build_is_refused(self):
        self.refused(document(crates=CRATES[:1], notices=NOTICES[:1], number=1),
                     "beta 2.0.0 is in a Windows build and not in the table of crates")
        self.refused(document(crates=CRATES + ["| `gamma` | 3.0.0 | ISC |"], number=3),
                     "gamma 3.0.0 is in the table of crates and not in a Windows build",
                     "gamma 3.0.0 is in the table of crates and not in the table of notices")
        self.refused(document(crates=[CRATES[0], "| `beta` | 2.0.0 | MIT |"]),
                     "beta 2.0.0 is listed as MIT; its manifest says MIT OR Apache-2.0")
        self.refused(document(notices=NOTICES[:1]), "beta 2.0.0 is in the table of crates and not in the table of notices")
        self.refused(document(number=7), "the text says 7 crates; the table has 2")


if __name__ == "__main__":
    unittest.main()
