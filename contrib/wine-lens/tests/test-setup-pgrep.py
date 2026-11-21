#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""setup.sh refuses a pgrep that cannot look among one user's processes: no Wine, lens board or root.

Also here, to be run with it: the list of a Wine library's exports is the same in every language, and
Wine's libraries are taken from the Wine that runs.
"""
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import unittest

SETUP = Path(__file__).resolve().parents[1] / "setup.sh"
START = Path(__file__).resolve().parents[1] / "tools/start-stack.sh"
# The scripts are POSIX ones; another shell can be named to run the newer tests here under it
# (for example "dash" or "busybox sh").
SHELL = os.environ.get("A2O_SETUP_TEST_SHELL", "sh").split()
NEEDED = "wine wineserver x86_64-w64-mingw32-gcc x86_64-w64-mingw32-objdump python3 xrandr rsync stty flock setsid"
# The first Wine 11.x the set-up's cfgmgr32 stand-in can work with (setup.sh names it in its refusal).
WINE_MINOR = 6


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

    def test_a_wine_that_is_too_old_is_refused_before_anything_is_made(self):
        # Seen with Wine 11.0 and 11.5 (and so with the 8.0 to 10.0 that Debian and Ubuntu ship): the set-up
        # ended with "done", and every request then failed after 80 seconds.
        self.stand_in("wine", 'echo "wine-10.0 (Ubuntu 10.0~repack-6ubuntu1)"')
        done = self.setup()
        self.assertEqual(done.returncode, 1)
        self.assertIn("too old", done.stderr)
        self.assertIn("wine-10.0 (Ubuntu 10.0~repack-6ubuntu1)", done.stderr)
        self.assertIn(f"Wine 11.{WINE_MINOR} or newer", done.stderr)
        # WineHQ's packages that put a wine on the PATH; wine-devel and wine-staging install under /opt alone.
        self.assertIn("winehq-devel or winehq-staging", done.stderr)
        self.assertNotRegex(done.stderr, r"(?<!/)\bwine-(devel|staging)\b")
        self.assertEqual(len(done.stderr.splitlines()), 1, done.stderr)
        self.assertFalse((self.root / "data").exists())

    def test_which_wines_count_as_too_old(self):
        function = re.search(r"^wine_too_old\(\) \{.*?^\}$", SETUP.read_text(), re.S | re.M).group(0)
        script = self.root / "version.sh"
        script.write_text(f'set -eu\n{function}\nif wine_too_old "$1"; then echo old; else echo fine; fi\n')
        cases = {
            "wine-6.0.3 (Ubuntu 6.0.3~repack-1)": "old", "wine-8.0 (Debian 8.0~repack-4)": "old",
            "wine-9.0 (Ubuntu 9.0~repack-4build3)": "old", "wine-10.0": "old", "wine-10.20": "old",
            "wine-11.0": "old", "wine-11.0-rc3": "old", "wine-11.0.1": "old", "wine-11.5 (Staging)": "old",
            f"wine-11.{WINE_MINOR - 1}": "old", f"wine-11.{WINE_MINOR}": "fine", "wine-11.21": "fine",
            "wine-11.22 (Staging)": "fine", "wine-12.0": "fine", "wine-12.0-rc1": "fine", "wine-12.3": "fine",
            "wine-100.1": "fine",
            # What cannot be read is let through: a build with a name of its own, or no answer at all.
            "": "fine", "wine-staging-11.0": "fine", "Wine 10.0": "fine", "wine-cachyos-11.2": "fine",
            "wine-11": "fine", "wine-x.y": "fine",
        }
        for says, expected in cases.items():
            with self.subTest(says=says):
                done = subprocess.run(["sh", str(script), says], stdin=subprocess.DEVNULL, capture_output=True,
                                      text=True, timeout=30)
                self.assertEqual((done.returncode, done.stdout.strip(), done.stderr), (0, expected, ""))

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


class SetupHelpTests(unittest.TestCase):
    """--help was taken for the Windows drive: "no Leia SR runtime in --help/Program Files/LeiaSR/Platform"."""

    def test_help_prints_the_usage_and_looks_for_nothing(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            # Through a link too, which is how a package's command reaches the script.
            (root / "any2odyssey-lens-setup").symlink_to(SETUP)
            environment = dict(os.environ, ANY2ODYSSEY_DATA_DIR=str(root / "data"))
            for script in (SETUP, root / "any2odyssey-lens-setup"):
                for option in ("--help", "-h"):
                    with self.subTest(script=script.name, option=option):
                        done = subprocess.run(["sh", str(script), option], cwd=root, env=environment,
                                              stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=60)
                        self.assertEqual(done.returncode, 0, done.stderr)
                        self.assertEqual(done.stderr, "")
                        lines = done.stdout.splitlines()
                        self.assertIn("usage: setup.sh <windows-root>", lines)
                        self.assertTrue(lines[0].startswith("Prepare the Wine route"), lines[0])
                        self.assertTrue(lines[-1].startswith("Run it again after a Wine upgrade"), lines[-1])
                        self.assertFalse(any(line.startswith("#") or "SPDX" in line for line in lines), lines)
            self.assertEqual(sorted(path.name for path in root.iterdir()), ["any2odyssey-lens-setup"])

    def test_no_argument_and_two_arguments_still_name_the_usage_and_fail(self):
        for arguments in ([], ["a", "b"], ["a", "--help"]):
            with self.subTest(arguments=arguments):
                done = subprocess.run(["sh", str(SETUP), *arguments], stdin=subprocess.DEVNULL, capture_output=True,
                                      text=True, timeout=60)
                self.assertEqual(done.returncode, 1)
                self.assertIn("usage: setup.sh <windows-root>", done.stderr)
                self.assertEqual(done.stdout, "")


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


class WineLibrariesTests(unittest.TestCase):
    """Where setup.sh and tools/start-stack.sh take Wine's own libraries from: folders that stand in for /usr, /opt."""

    FUNCTION = r"^wine_libraries\(\) \{.*?^\}$"

    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name).resolve()
        self.function = re.search(self.FUNCTION, SETUP.read_text(), re.S | re.M).group(0)
        (self.root / "ask.sh").write_text(f"set -eu\n{self.function}\nwine_libraries\n")
        # The one program the function runs, and nothing else of this machine: no wine it may have.
        (self.root / "tools").mkdir()
        (self.root / "tools/readlink").symlink_to(shutil.which("readlink"))
        self.shell = [shutil.which(SHELL[0]), *SHELL[1:]]

    def test_both_scripts_look_in_the_same_places(self):
        self.assertEqual(re.search(self.FUNCTION, START.read_text(), re.S | re.M).group(0), self.function)

    def wine(self, path, script=True):
        """A wine at this place below the folder: a program, or a link to another place."""
        path = self.root / path
        path.parent.mkdir(parents=True, exist_ok=True)
        if script is True:
            path.write_text("#!/bin/sh\nexit 0\n")
            path.chmod(0o755)
        else:
            path.symlink_to(self.root / script)

    def libraries(self, *folders):
        for folder in folders:
            (self.root / folder / "x86_64-windows").mkdir(parents=True)
            (self.root / folder / "x86_64-windows/cfgmgr32.dll").touch()

    def asked(self, *path):
        """What the function says with these folders (below the test's) on the PATH: (it found one, which)."""
        environment = dict(os.environ, PATH=os.pathsep.join([*(str(self.root / folder) for folder in path),
                                                              str(self.root / "tools")]))
        done = subprocess.run([*self.shell, str(self.root / "ask.sh")], env=environment, stdin=subprocess.DEVNULL,
                              capture_output=True, text=True, timeout=30)
        self.assertEqual(done.stderr, "")
        self.assertIn(done.returncode, (0, 1))
        self.assertEqual(done.returncode == 0, bool(done.stdout))
        return done.stdout.rstrip("\n")

    def usual(self):
        """The first of the usual folders this machine has, which is where the function ends without a hint."""
        listed = re.findall(r"(?<=[ \t])/(?:usr|opt)/\S+/x86_64-windows", self.function)
        self.assertEqual(len(listed), 7, listed)
        return next((folder for folder in listed if os.path.isfile(f"{folder}/cfgmgr32.dll")), "")

    def test_the_libraries_are_the_ones_beside_the_wine_that_runs(self):
        # Where distributions keep them: Arch and openSUSE, Fedora, Debian and Ubuntu, Fedora's other one.
        for number, folder in enumerate(("lib/wine", "lib64/wine", "lib/x86_64-linux-gnu/wine", "lib64/wine-wow64/wine")):
            with self.subTest(folder=folder):
                self.wine(f"{number}/usr/bin/wine")
                self.libraries(f"{number}/usr/{folder}")
                self.assertEqual(self.asked(f"{number}/usr/bin"), f"{self.root}/{number}/usr/{folder}/x86_64-windows")

    def test_winehqs_wine_under_opt_is_not_given_the_distributions_libraries(self):
        # WineHQ's packages: the programs and libraries under /opt/wine-devel, and winehq-devel's link to
        # the program in /usr/bin, beside libraries the distribution's own package has in /usr.
        for folder in ("lib/wine", "lib64/wine"):
            with self.subTest(folder=folder):
                top = folder.replace("/", "-")
                self.wine(f"{top}/opt/wine-devel/bin/wine")
                self.libraries(f"{top}/opt/wine-devel/{folder}", f"{top}/usr/lib/wine", f"{top}/usr/lib/x86_64-linux-gnu/wine")
                self.wine(f"{top}/usr/bin/wine", script=f"{top}/opt/wine-devel/bin/wine")
                wanted = f"{self.root}/{top}/opt/wine-devel/{folder}/x86_64-windows"
                self.assertEqual(self.asked(f"{top}/usr/bin"), wanted)
                # Or its folder put on the PATH by hand.
                self.assertEqual(self.asked(f"{top}/opt/wine-devel/bin", f"{top}/usr/bin"), wanted)

    def test_of_two_wines_the_first_on_the_path_is_the_one(self):
        self.wine("usr/bin/wine")
        self.wine("usr/local/bin/wine")
        self.libraries("usr/lib/wine", "usr/local/lib64/wine")
        self.assertEqual(self.asked("usr/bin", "usr/local/bin"), f"{self.root}/usr/lib/wine/x86_64-windows")
        self.assertEqual(self.asked("usr/local/bin", "usr/bin"), f"{self.root}/usr/local/lib64/wine/x86_64-windows")

    def test_links_are_followed_to_the_program(self):
        # Debian and Ubuntu: /usr/bin/wine is a link to a link (the alternatives) to a script, wine-stable.
        self.wine("usr/bin/wine-stable")
        self.wine("etc/alternatives/wine", script="usr/bin/wine-stable")
        self.wine("usr/bin/wine", script="etc/alternatives/wine")
        self.libraries("usr/lib/x86_64-linux-gnu/wine", "etc/lib/wine")
        self.assertEqual(self.asked("usr/bin"), f"{self.root}/usr/lib/x86_64-linux-gnu/wine/x86_64-windows")
        # A link in a folder of the user's own, to a Wine built into a folder of its own.
        self.wine("built/wine-11.22/bin/wine")
        self.libraries("built/wine-11.22/lib/wine")
        self.wine("home/bin/wine", script="built/wine-11.22/bin/wine")
        self.assertEqual(self.asked("home/bin", "usr/bin"), f"{self.root}/built/wine-11.22/lib/wine/x86_64-windows")

    def test_a_wine_kept_beside_its_libraries_is_found_there(self):
        self.wine("usr/lib/wine/wine")
        self.libraries("usr/lib/wine")
        self.assertEqual(self.asked("usr/lib/wine"), f"{self.root}/usr/lib/wine/x86_64-windows")

    def test_without_a_hint_it_is_the_usual_folders(self):
        # A script of the user's own that runs a Wine from somewhere else is not read; no wine at all (the
        # scripts ask for one before they come here); a link that leads nowhere.
        self.wine("home/bin/wine")
        self.assertEqual(self.asked("home/bin"), self.usual())
        self.assertEqual(self.asked(), self.usual())
        self.wine("broken/bin/wine", script="nowhere/bin/wine")
        self.assertEqual(self.asked("broken/bin"), self.usual())
        # Libraries without the one that is looked for are not Wine's 64-bit Windows ones.
        self.wine("empty/usr/bin/wine")
        (self.root / "empty/usr/lib/wine/x86_64-windows").mkdir(parents=True)
        self.assertEqual(self.asked("empty/usr/bin"), self.usual())


if __name__ == "__main__":
    unittest.main()
