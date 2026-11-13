#!/usr/bin/env python3
"""Exercise release-archive installation without root, downloads or hardware.

Requires gio (libglib2.0-bin) and desktop-file-validate (desktop-file-utils).
The installed fixture only records its argv; it never opens an image or a UI.
"""

import os
from pathlib import Path
import pwd
import shutil
import subprocess
import tempfile
import time
import unittest


LINUX = Path(__file__).resolve().parents[1]
# The installer is a POSIX script; another shell can be named to run the same tests under it
# (for example "dash" or "busybox sh").
SHELL = os.environ.get("A2O_INSTALL_TEST_SHELL", "sh").split()
SERVICE = Path("Program Files/LeiaSR/Platform/bin/SRService.exe")


class InstallerTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="any2odyssey-install-test-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.archive = self.root / "archive"
        self.archive.mkdir()
        shutil.copyfile(LINUX / "install.sh", self.archive / "install.sh")
        shutil.copyfile(LINUX / "any2odyssey.desktop", self.archive / "any2odyssey.desktop")
        (self.archive / "any2odyssey").write_text(
            '#!/bin/sh\nprintf "%s\\000" "$0" "$@" > "$A2O_INSTALL_TEST_LOG"\n'
        )
        (self.archive / "any2odyssey.png").write_bytes(b"test icon")
        (self.archive / "wine-lens").mkdir()
        setup = self.archive / "wine-lens" / "setup.sh"
        setup.write_text('#!/bin/sh\nprintf "%s\\000" "$@" > "$A2O_INSTALL_TEST_SETUP_LOG"\n')
        setup.chmod(0o755)
        self.log = self.root / "launched-argv"
        self.setup_log = self.root / "setup-argv"
        # A stand-in for /: the installer looks under it for the lens board, mounted drives, the
        # mount table and /sys, and for udev rules it would otherwise remove with sudo.
        self.system = self.root / "system"
        for folder in ["dev", "proc/self", "sys/class/block", "mnt", "media"]:
            (self.system / folder).mkdir(parents=True)
        self.mounts = ["rootfs / rootfs rw 0 0"]
        self.write_mounts()
        self.env = dict(os.environ, ANY2ODYSSEY_DATA_DIR=str(self.root / "data"),
                        A2O_INSTALL_TEST_LOG=str(self.log),
                        A2O_INSTALL_TEST_SETUP_LOG=str(self.setup_log),
                        A2O_INSTALL_TEST_ROOT=str(self.system))

    def install(self, prefix, *extra):
        return self.run_installer("--prefix", str(prefix), "--no-deps", "--no-udev", "--no-model",
                                  "--no-odyssey", *extra)

    def run_installer(self, *arguments):
        return subprocess.run(
            [*SHELL, str(self.archive / "install.sh"), *arguments],
            cwd=self.root, env=self.env, text=True, capture_output=True, timeout=30,
            stdin=subprocess.DEVNULL,
        )

    def write_mounts(self):
        (self.system / "proc/self/mounts").write_text("".join(line + "\n" for line in self.mounts))

    def block_device(self, name, bus, removable="0", under=()):
        """A disk with one partition in the stand-in /sys; returns the partition's kernel name."""
        disk = self.system / "sys/devices" / bus / "block" / name
        partition = disk / (name + "1")
        partition.mkdir(parents=True)
        (partition / "partition").write_text("1\n")
        (disk / "removable").write_text(removable + "\n")
        (disk / "slaves").mkdir()
        for lower in under:
            (disk / "slaves" / lower).symlink_to(self.system / "sys/class/block" / lower)
        (self.system / "sys/class/block" / name).symlink_to(disk)
        (self.system / "sys/class/block" / partition.name).symlink_to(partition)
        return partition.name

    def windows_drive(self, where, device=None, system_folder="Windows/System32"):
        """A mounted drive with Leia's runtime; `device` is the partition the mount table names."""
        drive = self.system / where
        (drive / SERVICE).parent.mkdir(parents=True)
        (drive / SERVICE).write_bytes(b"MZ")
        if system_folder:
            (drive / system_folder).mkdir(parents=True)
        if device:
            point = str(drive).replace("\\", "\\134").replace(" ", "\\040")
            self.mounts.append(f"/dev/{device} {point} ntfs3 ro 0 0")
            self.write_mounts()
        return drive

    def prepare_odyssey(self, *extra):
        """Install with the Odyssey 3D step, as if the monitor were connected."""
        (self.system / "dev/odyssey-lens").touch()
        return self.run_installer("--prefix", str(self.root / "prefix"), "--no-deps", "--no-udev",
                                  "--no-model", *extra)

    def setup_ran_with(self):
        if not self.setup_log.exists():
            return None
        return [os.fsdecode(part) for part in self.setup_log.read_bytes().split(b"\0")[:-1]]

    def check_launcher(self, prefix):
        prefix = Path(prefix)
        installed = prefix if prefix.is_absolute() else self.root / prefix
        result = self.install(prefix)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        binary = installed / "bin" / "any2odyssey"
        desktop = installed / "share" / "applications" / "any2odyssey.desktop"
        self.assertTrue(binary.is_file())
        valid = subprocess.run(["desktop-file-validate", str(desktop)],
                               text=True, capture_output=True, timeout=10)
        self.assertEqual(valid.returncode, 0, valid.stdout + valid.stderr)
        picture = self.root / "picture with spaces & %f.png"
        picture.write_bytes(b"test image")
        self.log.unlink(missing_ok=True)
        # Launch from outside the installer's working directory, as a menu does.
        launched = subprocess.run(["gio", "launch", str(desktop), str(picture)],
                                  cwd=self.archive, env=self.env, text=True,
                                  capture_output=True, timeout=10)
        self.assertEqual(launched.returncode, 0, launched.stdout + launched.stderr)
        expected = [os.fsencode(binary), os.fsencode(picture)]
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            if self.log.exists() and self.log.read_bytes().split(b"\0")[:-1] == expected:
                break
            time.sleep(0.01)
        self.assertTrue(self.log.exists(), "desktop launcher did not run the installed program")
        self.assertEqual(self.log.read_bytes().split(b"\0")[:-1], expected)
        self.assertEqual(list(installed.rglob("*.desktop")), [desktop])

    def test_launcher_in_an_ordinary_prefix(self):
        self.check_launcher(self.root / "ordinary")

    def test_launcher_with_shell_and_sed_characters_in_prefix(self):
        self.check_launcher(self.root / "apps & games|with 'quotes\" $cash`and\\slashes")

    def test_literal_percent_codes_in_prefix_do_not_consume_the_image_argument(self):
        self.check_launcher(self.root / "apps%f-%F-%%")

    def test_equal_sign_in_prefix_is_passed_as_a_command_argument(self):
        self.check_launcher(self.root / "apps=local")

    def test_relative_prefix_is_resolved_before_writing_the_launcher(self):
        self.check_launcher(Path("relative apps"))

    def test_wrapper_keeps_shell_syntax_and_unicode_literal(self):
        self.check_launcher(self.root / 'apps=\u00e9%f $(touch INJECTED);`touch INJECTED`"\\')
        self.assertFalse((self.archive / "INJECTED").exists())

    def test_explicit_root_prefix_is_preserved_in_dry_run(self):
        result = self.install("/", "--dry-run")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("Installing the program to /\n", result.stdout)

    def test_dry_run_creates_no_installation(self):
        prefix = self.root / "dry & run"
        result = self.install(prefix, "--dry-run")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertFalse(prefix.exists())

    def test_prefix_is_resolved_without_gnu_readlink(self):
        # BusyBox's readlink has -f but not -m.
        real = shutil.which("readlink")
        tools = self.root / "tools"
        tools.mkdir()
        (tools / "readlink").write_text(
            '#!/bin/sh\ncase $1 in -m) echo "readlink: unrecognized option: m" >&2; exit 1 ;; esac\n'
            f'exec "{real}" "$@"\n')
        (tools / "readlink").chmod(0o755)
        self.env["PATH"] = f"{tools}{os.pathsep}{self.env['PATH']}"
        self.check_launcher(Path("relative apps/not there yet/deeper/"))

    def test_desktop_entry_that_cannot_be_pointed_at_the_program_fails_the_install(self):
        entry = self.archive / "any2odyssey.desktop"
        entry.write_text(entry.read_text().replace("Exec=any2odyssey %f", "Exec=/usr/bin/any2odyssey %f"))
        prefix = self.root / "prefix"
        result = self.install(prefix)
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn("desktop entry", result.stderr)
        self.assertFalse((prefix / "share/applications/any2odyssey.desktop").exists())

    def test_the_one_internal_windows_drive_is_used_with_yes(self):
        login = pwd.getpwuid(os.getuid()).pw_name
        drive = self.windows_drive(f"run/media/{login}/Win Drive", self.block_device("a2ofix", "pci0000:00/ata1"))
        # Without USER the script must ask the system who is running it.
        self.env.pop("USER", None)
        result = self.prepare_odyssey("--yes")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(self.setup_ran_with(), [str(drive)], result.stdout + result.stderr)

    def test_usb_drive_does_not_displace_the_internal_one(self):
        login = pwd.getpwuid(os.getuid()).pw_name
        stick = self.windows_drive(f"run/media/{login}/STICK", self.block_device("a2ostick", "pci0000:00/usb2/2-1"))
        drive = self.windows_drive("mnt/windows", self.block_device("a2ofix", "pci0000:00/nvme0"))
        result = self.prepare_odyssey("--yes")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn(f"not used: {stick} (a removable drive)", result.stdout)
        self.assertEqual(self.setup_ran_with(), [str(drive)], result.stdout + result.stderr)

    def test_removable_drives_are_not_used_unasked(self):
        drives = {
            "usb": self.windows_drive("media/usb", self.block_device("a2ostick", "pci0000:00/usb2/2-1")),
            "card": self.windows_drive("media/card", self.block_device("a2ocard", "pci0000:00/mmc0", removable="1")),
            # An encrypted or mapped volume is what the device under it is.
            "mapped": self.windows_drive("mnt/mapped", self.block_device(
                "a2omap", "virtual", under=[self.block_device("a2olower", "pci0000:00/usb3/3-2")])),
        }
        result = self.prepare_odyssey("--yes")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        for drive in drives.values():
            self.assertIn(f"not used: {drive} (a removable drive)", result.stdout)
        self.assertIn("--windows-root", result.stdout)
        self.assertIsNone(self.setup_ran_with())

    def test_named_windows_root_is_taken_as_given(self):
        stick = self.windows_drive("media/usb", self.block_device("a2ostick", "pci0000:00/usb2/2-1"))
        self.windows_drive("mnt/windows", self.block_device("a2ofix", "pci0000:00/nvme0"))
        result = self.prepare_odyssey("--yes", "--windows-root", f"{stick}/")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(self.setup_ran_with(), [str(stick)], result.stdout + result.stderr)

    def test_several_windows_drives_are_listed_and_none_is_chosen(self):
        first = self.windows_drive("mnt/first", self.block_device("a2oone", "pci0000:00/ata1"))
        second = self.windows_drive("mnt/second", self.block_device("a2otwo", "pci0000:00/ata2"))
        result = self.prepare_odyssey("--yes")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        for drive in (first, second):
            self.assertIn(f"       {drive}\n", result.stdout)
        self.assertIn("--windows-root", result.stdout)
        self.assertIsNone(self.setup_ran_with())

    def test_folder_without_a_windows_system_folder_is_not_a_windows_drive(self):
        folder = self.windows_drive("mnt/backup", self.block_device("a2ofix", "pci0000:00/ata1"), system_folder=None)
        result = self.prepare_odyssey("--yes")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn(f"not used: {folder} (no Windows system folder", result.stdout)
        self.assertIsNone(self.setup_ran_with())

    def test_drive_of_unknown_origin_must_be_named(self):
        # Not in the mount table under a device: a network share, or a file system this cannot identify.
        drive = self.windows_drive("mnt/share")
        self.mounts.append(f"//server/share {drive} cifs rw 0 0")
        self.write_mounts()
        result = self.prepare_odyssey("--yes")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn(f"--windows-root '{drive}'", result.stdout)
        self.assertIsNone(self.setup_ran_with())
        # Asked instead of refused without --yes; nobody answers here, so it still does not run.
        result = self.prepare_odyssey()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn(f"could not tell that {drive} is on a drive inside this machine", result.stdout)
        self.assertIsNone(self.setup_ran_with())

    @unittest.skipUnless(os.geteuid() == 0, "needs root to give the files to another user")
    def test_drive_with_another_users_files_is_not_used(self):
        drive = self.windows_drive("mnt/theirs", self.block_device("a2ofix", "pci0000:00/ata1"))
        os.chown(drive / SERVICE, 65534, 65534)
        result = self.prepare_odyssey("--yes")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn(f"not used: {drive} (its files belong to another user)", result.stdout)
        self.assertIsNone(self.setup_ran_with())

    def test_empty_windows_root_is_rejected(self):
        for root in ["", "/"]:
            with self.subTest(root=root):
                result = self.prepare_odyssey("--yes", "--windows-root", root)
                self.assertEqual(result.returncode, 2, result.stdout + result.stderr)
                self.assertIn("--windows-root", result.stderr)

    def test_uninstall_stops_the_serial_port_keeper_and_only_in_its_own_wine_prefix(self):
        wine_lens = Path(self.env["ANY2ODYSSEY_DATA_DIR"]) / "wine-lens"
        (wine_lens / "tools").mkdir(parents=True)
        keeper_script = wine_lens / "tools" / "noxon.sh"
        keeper_script.write_text("#!/bin/sh\nwhile :; do sleep 0.05; done\n")
        keeper_script.chmod(0o755)
        keeper = subprocess.Popen([str(keeper_script)], start_new_session=True)
        self.addCleanup(keeper.wait)
        self.addCleanup(keeper.kill)
        # A number noted before a restart can belong to anything by now.
        bystander = subprocess.Popen(["sleep", "30"])
        self.addCleanup(bystander.wait)
        self.addCleanup(bystander.kill)
        runtime = self.root / "runtime"
        runtime.mkdir()
        (runtime / "any2odyssey-noxon.pid").write_text(f"{keeper.pid}\n")
        (wine_lens / "any2odyssey-noxon.pid").write_text(f"{bystander.pid}\n")
        # wineserver -k without the prefix would end the programs of the user's default prefix.
        tools = self.root / "tools"
        tools.mkdir()
        served = self.root / "wineserver-prefix"
        (tools / "wineserver").write_text(f'#!/bin/sh\nprintf "%s %s" "$WINEPREFIX" "$*" > "{served}"\n')
        (tools / "wineserver").chmod(0o755)
        self.env.update(XDG_RUNTIME_DIR=str(runtime), PATH=f"{tools}{os.pathsep}{self.env['PATH']}")
        result = self.run_installer("--prefix", str(self.root / "prefix"), "--uninstall", "--yes")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(keeper.wait(timeout=5), -15)
        self.assertIsNone(bystander.poll())
        self.assertFalse((runtime / "any2odyssey-noxon.pid").exists())
        self.assertFalse(wine_lens.exists())
        self.assertEqual(served.read_text(), f"{wine_lens / 'prefix'} -k")

    def test_help_lists_every_option(self):
        result = self.run_installer("--help")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertTrue(result.stdout.startswith("Install Any2Odyssey for Linux"), result.stdout)
        for option in ["--yes", "--no-deps", "--no-udev", "--no-model", "--no-odyssey", "--windows-root DIR",
                       "--prefix DIR", "--packaged", "--dry-run", "--uninstall "]:
            self.assertIn(f"\n  {option}", result.stdout)
        self.assertNotIn("set -eu", result.stdout)

    def test_empty_or_multiline_prefix_is_rejected_before_installing(self):
        for prefix in ["", str(self.root / "bad\nline"), str(self.root / "bad\rline")]:
            with self.subTest(prefix=prefix):
                result = self.install(prefix, "--dry-run")
                self.assertEqual(result.returncode, 2, result.stdout + result.stderr)
                self.assertIn("prefix", result.stderr)


if __name__ == "__main__":
    unittest.main()
