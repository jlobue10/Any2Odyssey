#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""setup.sh refuses a pgrep that cannot look among one user's processes: no Wine, lens board or root.

Also here, to be run with it: the list of a Wine library's exports is the same in every language.
"""
import os
from pathlib import Path
import re
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


class SetupExportListTests(unittest.TestCase):
    """setup.sh reads a Wine library's exports from objdump's listing, whatever language the user's locale is."""

    # GNU objdump -p around a PE file's export names. One built with translations (the default of binutils)
    # writes the heading in the language of the locale: this is the French of binutils 2.47.
    OBJDUMP = """case ${LC_ALL:-${LC_MESSAGES:-${LANG:-C}}} in
    C | POSIX | C.* | en_*) heading='[Ordinal/Name Pointer] Table -- Ordinal Base 1' ;;
    *) heading='Table [Ordinal/Nom de pointeur] -- Base Ordinal 1' ;;
esac
cat <<EOF
Export Address Table -- Ordinal Base 1
    [   0] +base[   1] 00001330 Export RVA
    [   1] +base[   2] 0000133b Export RVA

$heading
              Ordinal   Hint Name
    [   0] +base[   1]  0000 CM_Get_Child
    [   1] +base[   2]  0001 CM_Get_Parent

The Function Table (interpreted .pdata section contents)
EOF"""

    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        (self.root / "bin").mkdir()
        (self.root / "bin/x86_64-w64-mingw32-objdump").write_text(f"#!/bin/sh\n{self.OBJDUMP}\n")
        (self.root / "bin/x86_64-w64-mingw32-objdump").chmod(0o755)
        # The function as setup.sh has it; the rest of that script needs Wine and the lens board.
        function = re.search(r"^forwarding_def\(\) \{.*?^\}$", SETUP.read_text(), re.S | re.M).group(0)
        (self.root / "exports.sh").write_text(
            f"set -eu\nWINELIB=/nowhere\n{function}\nforwarding_def cfgmgr32 cfgmgw CM_Get_Child\n")

    def exports(self, **locale):
        environment = {name: value for name, value in os.environ.items()
                       if name not in ("LANG", "LANGUAGE") and not name.startswith("LC_")}
        environment.update(locale, PATH=f"{self.root / 'bin'}:{os.environ['PATH']}")
        done = subprocess.run(["sh", str(self.root / "exports.sh")], env=environment, stdin=subprocess.DEVNULL,
                              capture_output=True, text=True, timeout=30)
        self.assertEqual(done.returncode, 0, done.stderr)
        return done.stdout.splitlines()

    FORWARDS = ["LIBRARY cfgmgr32.dll", "EXPORTS", "    CM_Get_Child = shim_CM_Get_Child",
                "    CM_Get_Parent = cfgmgw.CM_Get_Parent"]

    def test_the_exports_are_listed_in_english(self):
        self.assertEqual(self.exports(LC_ALL="C"), self.FORWARDS)

    def test_the_exports_are_listed_in_a_locale_objdump_has_a_translation_for(self):
        # Without them the stand-in library forwards nothing, and no program that needs the real one starts.
        self.assertEqual(self.exports(LANG="fr_FR.UTF-8"), self.FORWARDS)


if __name__ == "__main__":
    unittest.main()
