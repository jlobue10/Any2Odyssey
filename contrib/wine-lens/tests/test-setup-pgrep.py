#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""setup.sh refuses a pgrep that cannot look among one user's processes: no Wine, lens board or root."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

SETUP = Path(__file__).resolve().parents[1] / "setup.sh"
NEEDED = "wine wineserver x86_64-w64-mingw32-gcc x86_64-w64-mingw32-objdump python3 xrandr rsync stty flock setsid"


class SetupPgrepTests(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        # A Windows drive with as much of Leia's runtime as setup.sh looks for first, and the tools it asks for.
        (self.root / "win/Program Files/LeiaSR/Platform/bin").mkdir(parents=True)
        (self.root / "win/Program Files/LeiaSR/Platform/bin/SRService.exe").touch()
        (self.root / "win/ProgramData/Simulated Reality").mkdir(parents=True)
        (self.root / "bin").mkdir()
        for tool in NEEDED.split():
            self.stand_in(tool, "exit 0")

    def stand_in(self, name, body):
        (self.root / "bin" / name).write_text(f"#!/bin/sh\n{body}\n")
        (self.root / "bin" / name).chmod(0o755)

    def setup(self):
        environment = dict(os.environ, PATH=f"{self.root / 'bin'}:{os.environ['PATH']}",
                           ANY2ODYSSEY_DATA_DIR=str(self.root / "data"))
        return subprocess.run(["sh", str(SETUP), str(self.root / "win")], env=environment, stdin=subprocess.DEVNULL,
                              capture_output=True, text=True, timeout=60)

    def test_a_pgrep_without_u_is_refused_before_anything_is_made(self):
        # BusyBox's: any option it does not know ends it with a failure.
        self.stand_in("pgrep", 'echo "pgrep: unrecognized option: u" >&2; exit 1')
        done = self.setup()
        self.assertEqual(done.returncode, 1)
        self.assertIn("pgrep that takes -u", done.stderr)
        self.assertFalse((self.root / "data").exists())

    @unittest.skipIf(os.path.exists("/dev/odyssey-lens"), "with the lens board present setup.sh would go on")
    def test_the_pgrep_of_procps_is_accepted(self):
        if subprocess.run(["pgrep", "-u", str(os.getuid()), "-f", "."], capture_output=True).returncode:
            self.skipTest("this machine's pgrep has no -u")
        done = self.setup()
        self.assertEqual(done.returncode, 1)
        self.assertNotIn("pgrep", done.stderr)
        self.assertIn("/dev/odyssey-lens is missing", done.stderr)

    @unittest.skipIf(os.path.exists("/dev/odyssey-lens"), "with the lens board present setup.sh would go on")
    def test_a_relative_xdg_data_home_counts_as_unset_as_the_program_reads_it(self):
        # XDG_DATA_HOME=~/.local/share set where nothing expands the ~, or any other relative path, is invalid
        # by the XDG specification and the program takes the usual folder. Taken as written, the Wine side was
        # built under the folder setup.sh was run from, where the program does not look for the lens helper.
        # setup.sh makes nothing before it asks for the lens board, so the folder is read from the shell's
        # own trace of its assignments.
        home = self.root / "home"
        home.mkdir()
        environment = {name: value for name, value in os.environ.items() if name != "ANY2ODYSSEY_DATA_DIR"}
        environment.update(PATH=f"{self.root / 'bin'}:{os.environ['PATH']}", HOME=str(home))
        usual = home / ".local/share"
        for value, base in (("relative/share", usual), ("~/.local/share", usual), ("", usual),
                            (str(self.root / "xdg"), self.root / "xdg")):
            with self.subTest(value=value):
                environment["XDG_DATA_HOME"] = value
                done = subprocess.run(["sh", "-x", str(SETUP), str(self.root / "win")], cwd=self.root,
                                      env=environment, stdin=subprocess.DEVNULL, capture_output=True, text=True,
                                      timeout=60)
                self.assertEqual(done.returncode, 1)
                self.assertIn(f"+ T={base}/any2odyssey/wine-lens\n", done.stderr.replace("'", ""))
        self.assertEqual(sorted(path.name for path in self.root.iterdir()), ["bin", "home", "win"])


if __name__ == "__main__":
    unittest.main()
