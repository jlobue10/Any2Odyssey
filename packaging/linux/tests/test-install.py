#!/usr/bin/env python3
"""Exercise release-archive installation without root, downloads or hardware.

Requires gio (libglib2.0-bin) and desktop-file-validate (desktop-file-utils).
The installed fixture only records its argv; it never opens an image or a UI.
"""

import os
from pathlib import Path
import pwd
import shlex
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
        # mount table and /sys. While it is set the installer runs nothing with sudo.
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

    def block_device(self, name, bus, removable="0", under=(), card=None):
        """A disk with one partition in the stand-in /sys; returns the partition's kernel name.

        `card` is what an MMC host says its card is: "SD" for a memory card, "MMC" for soldered eMMC.
        """
        disk = self.system / "sys/devices" / bus / "block" / name
        partition = disk / (name + "1")
        partition.mkdir(parents=True)
        (partition / "partition").write_text("1\n")
        (disk / "removable").write_text(removable + "\n")
        if card:
            (disk / "device").mkdir()
            (disk / "device" / "type").write_text(card + "\n")
        (disk / "slaves").mkdir()
        for lower in under:
            (disk / "slaves" / lower).symlink_to(self.system / "sys/class/block" / lower)
        (self.system / "sys/class/block" / name).symlink_to(disk)
        (self.system / "sys/class/block" / partition.name).symlink_to(partition)
        return partition.name

    def windows_drive(self, where, device=None, system_folder="Windows/System32", fstype="ntfs3"):
        """A mounted drive with Leia's runtime; `device` is the partition the mount table names."""
        drive = self.system / where
        (drive / SERVICE).parent.mkdir(parents=True)
        (drive / SERVICE).write_bytes(b"MZ")
        if system_folder:
            (drive / system_folder).mkdir(parents=True)
        if device:
            point = str(drive).replace("\\", "\\134").replace(" ", "\\040")
            self.mounts.append(f"/dev/{device} {point} {fstype} ro 0 0")
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
            "flagged": self.windows_drive("media/flagged", self.block_device("a2oflag", "pci0000:00/ata3", removable="1")),
            # A card in a built-in reader: the kernel calls the disk fixed, the card says what it is.
            "card": self.windows_drive("media/card", self.block_device("a2ocard", "pci0000:00/mmc0", card="SD")),
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

    def test_windows_on_soldered_emmc_is_a_drive_of_this_machine(self):
        drive = self.windows_drive("mnt/windows", self.block_device("a2oemmc", "platform/sdhci/mmc0", card="MMC"))
        result = self.prepare_odyssey("--yes")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(self.setup_ran_with(), [str(drive)], result.stdout + result.stderr)

    def test_ntfs_3g_mount_of_an_internal_drive_is_used(self):
        drive = self.windows_drive("mnt/windows", self.block_device("a2ofix", "pci0000:00/nvme0"), fstype="fuseblk")
        result = self.prepare_odyssey("--yes")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(self.setup_ran_with(), [str(drive)], result.stdout + result.stderr)

    def test_user_file_system_that_names_an_internal_device_is_not_believed(self):
        # Any user's FUSE program chooses the source the mount table shows, and the owners of its files.
        self.block_device("a2ofix", "pci0000:00/nvme0")
        for fstype in ["fuse.lookalike", "fuse"]:
            with self.subTest(fstype=fstype):
                self.mounts = ["rootfs / rootfs rw 0 0"]
                drive = self.windows_drive(f"mnt/{fstype}", "a2ofix1", fstype=fstype)
                result = self.prepare_odyssey("--yes")
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertIn(f"--windows-root '{drive}'", result.stdout)
                self.assertIsNone(self.setup_ran_with())
                shutil.rmtree(drive)

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

    def test_stand_in_root_never_reaches_a_command_run_as_root(self):
        # The variable comes from the environment: what it names must not be handed to sudo.
        tools = self.root / "tools"
        tools.mkdir()
        called = self.root / "sudo-argv"
        (tools / "sudo").write_text(f'#!/bin/sh\necho "$*" >> "{called}"\n')
        (tools / "sudo").chmod(0o755)
        self.env["PATH"] = f"{tools}{os.pathsep}{self.env['PATH']}"
        rules = self.system / "etc/udev/rules.d/70-any2odyssey-lens.rules"
        rules.parent.mkdir(parents=True)
        rules.write_text("# somebody else's file\n")
        (self.archive / "wine-lens" / "70-any2odyssey-lens.rules").write_text("# the rules\n")
        prefix = self.root / "prefix"
        result = self.run_installer("--prefix", str(prefix), "--yes", "--no-model", "--no-odyssey")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("A2O_INSTALL_TEST_ROOT is set", result.stdout)
        result = self.run_installer("--prefix", str(prefix), "--uninstall", "--yes")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertFalse(called.exists(), called.read_text() if called.exists() else "")
        self.assertEqual(rules.read_text(), "# somebody else's file\n")

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

    def two_installations(self, prefix):
        """One under `prefix` and one in the usual place of a stand-in home; returns the two outputs."""
        home = self.root / "home"
        home.mkdir()
        self.env["HOME"] = str(home)
        flags = ["--no-deps", "--no-udev", "--no-model", "--no-odyssey"]
        usual = self.run_installer(*flags)
        self.assertEqual(usual.returncode, 0, usual.stdout + usual.stderr)
        self.assertTrue((home / ".local/bin/any2odyssey").is_file())
        named = self.run_installer("--prefix", str(prefix), *flags)
        self.assertEqual(named.returncode, 0, named.stdout + named.stderr)
        self.assertTrue((prefix / "bin/any2odyssey").is_file())
        return named, usual

    def removal_command(self, result):
        lines = [line for line in result.stdout.splitlines() if line.startswith("To remove it later: ")]
        self.assertEqual(len(lines), 1, result.stdout)
        return lines[0].removeprefix("To remove it later: ")

    def assert_removed(self, prefix):
        for left in ["bin/any2odyssey", "share/any2odyssey", "share/doc/any2odyssey",
                     "share/applications/any2odyssey.desktop", "share/icons/hicolor/256x256/apps/any2odyssey.png"]:
            self.assertFalse((prefix / left).exists(), f"{prefix / left} is still there")

    def test_the_removal_command_it_prints_removes_the_installation_it_made(self):
        for name in ["opt", "apps & games with 'quotes\" $cash`and\\slashes"]:
            with self.subTest(prefix=name):
                prefix = self.root / name
                named, usual = self.two_installations(prefix)
                home = Path(self.env["HOME"])
                self.assertEqual(self.removal_command(usual), f"{home}/.local/share/any2odyssey/install.sh --uninstall")
                # As a shell reads the line, whatever the prefix has in it.
                words = shlex.split(self.removal_command(named))
                self.assertEqual(words, [f"{prefix}/share/any2odyssey/install.sh", "--uninstall", "--prefix", str(prefix)])
                result = subprocess.run([*SHELL, *words], cwd=self.root, env=self.env, text=True,
                                        capture_output=True, timeout=30, stdin=subprocess.DEVNULL)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assert_removed(prefix)
                self.assertTrue((home / ".local/bin/any2odyssey").is_file(), "the other installation was removed")
                shutil.rmtree(home)

    def test_the_installed_copy_removes_the_installation_it_is_part_of(self):
        # Without --prefix it used to look in ~/.local wherever it was: nothing of its own was removed,
        # and an installation there was.
        prefix = self.root / "opt with space"
        self.two_installations(prefix)
        home = Path(self.env["HOME"])
        for installed in [prefix, home / ".local"]:
            self.assertTrue((home / ".local/bin/any2odyssey").is_file(), "the other installation was removed")
            result = subprocess.run([*SHELL, str(installed / "share/any2odyssey/install.sh"), "--uninstall"],
                                    cwd=self.root, env=self.env, text=True, capture_output=True, timeout=30,
                                    stdin=subprocess.DEVNULL)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertIn(f"Removing Any2Odyssey from {installed}\n", result.stdout)
            self.assert_removed(installed)

    def test_a_packages_copy_run_by_its_own_name_removes_nothing_of_the_package(self):
        # A distribution's package has the script at <prefix>/share/any2odyssey too, and any2odyssey-setup
        # (which says --packaged) to run it. Run directly it took /usr for its own installation and, as
        # root, removed the package's program, desktop entry, icon, documents and itself.
        usr = self.root / "usr"
        kept = ["bin/any2odyssey", "bin/any2odyssey-setup", "share/any2odyssey/install.sh",
                "share/any2odyssey/wine-lens/setup.sh", "share/doc/any2odyssey/README.md",
                "share/applications/any2odyssey.desktop", "share/icons/hicolor/256x256/apps/any2odyssey.png"]
        for name in kept:
            (usr / name).parent.mkdir(parents=True, exist_ok=True)
            (usr / name).write_text("the package's\n")
        shutil.copyfile(LINUX / "install.sh", usr / "share/any2odyssey/install.sh")
        home = self.root / "home"
        home.mkdir()
        self.env["HOME"] = str(home)
        result = subprocess.run([*SHELL, str(usr / "share/any2odyssey/install.sh"), "--uninstall"],
                                cwd=self.root, env=self.env, text=True, capture_output=True, timeout=30,
                                stdin=subprocess.DEVNULL)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("remove it with the package manager", result.stdout)
        for name in kept:
            self.assertTrue((usr / name).is_file(), f"{usr / name} was removed")

    def test_the_installed_readme_has_everything_it_links_to(self):
        # 0.9.1 installed README.md with two of the four documents it links to and none of its pictures.
        linked = ["docs/LENS-SWITCH.md", "docs/TESTING.md", "docs/assets/controls-light.png",
                  "installer/README.md", "LICENSE"]
        for name in linked:
            (self.archive / name).parent.mkdir(parents=True, exist_ok=True)
            (self.archive / name).write_text("a document\n")
        (self.archive / "README.md").write_text(
            "".join(f"[{name}]({name})\n" for name in linked) + "[elsewhere](https://example.org/a.md)\n")
        prefix = self.root / "prefix"
        result = self.install(prefix)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        documents = prefix / "share/doc/any2odyssey"
        for name in ["README.md", *linked]:
            self.assertTrue((documents / name).is_file(), f"{name} was not installed")
        # The check the package steps run agrees, and notices a link that leads nowhere.
        check = [*SHELL, str(LINUX.parents[1] / "scripts" / "check-doc-links.sh"), str(documents)]
        result = subprocess.run(check, text=True, capture_output=True, timeout=30)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        (documents / "docs/TESTING.md").unlink()
        result = subprocess.run(check, text=True, capture_output=True, timeout=30)
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn("README.md links to docs/TESTING.md", result.stderr)
        result = self.run_installer("--prefix", str(prefix), "--uninstall")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assert_removed(prefix)

    def test_uninstall_refreshes_the_caches_the_installation_wrote(self):
        # The released 0.9.1 archive left share/applications/mimeinfo.cache naming any2odyssey.desktop
        # as the program for PNG, JPEG and BMP files after the entry itself was removed.
        if not shutil.which("update-desktop-database"):
            self.skipTest("update-desktop-database (desktop-file-utils) is not installed")
        prefix = self.root / "prefix"
        result = self.install(prefix)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        cache = prefix / "share/applications/mimeinfo.cache"
        self.assertIn("any2odyssey.desktop", cache.read_text())
        # A dry run changes nothing.
        result = self.run_installer("--prefix", str(prefix), "--uninstall", "--dry-run")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("any2odyssey.desktop", cache.read_text())
        result = self.run_installer("--prefix", str(prefix), "--uninstall")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assert_removed(prefix)
        self.assertNotIn("any2odyssey", cache.read_text())

    def usual_place(self):
        """A stand-in home with nothing naming another data folder: ~/.local/share/any2odyssey is then
        both where the installer keeps its own files and the program's data folder."""
        home = self.root / "home"
        home.mkdir()
        self.env["HOME"] = str(home)
        for name in ["ANY2ODYSSEY_DATA_DIR", "XDG_DATA_HOME", "XDG_RUNTIME_DIR"]:
            self.env.pop(name, None)
        return home, home / ".local/share/any2odyssey"

    def wine_side(self, data):
        """What setup.sh and the program leave in the data folder; returns the files that must stay."""
        kept = [data / "settings.json", data / "models/depth-anything-v2-small.onnx",
                data / "wine-lens/prefix/system.reg", data / "wine-lens/build/a2ousb.sys",
                data / "wine-lens/camera.secret", data / "wine-lens/tools/tracking.sh"]
        for file in kept:
            file.parent.mkdir(parents=True, exist_ok=True)
            file.write_text("kept\n")
        return kept

    def test_in_the_usual_place_the_setup_is_not_kept_where_the_wine_side_is_built(self):
        # 0.9.1 kept its copy of wine-lens in <data folder>/wine-lens, which is where setup.sh builds:
        # setup.sh then failed at the end copying its tools onto themselves, installing again (an update)
        # removed the Wine prefix with the copy of Leia's runtime, and --uninstall removed the whole data
        # folder while saying that the settings and the depth model stay.
        home, data = self.usual_place()
        self.windows_drive("mnt/windows", self.block_device("a2ofix", "pci0000:00/nvme0"))
        (self.system / "dev/odyssey-lens").touch()
        flags = ["--no-deps", "--no-udev", "--no-model"]
        result = self.run_installer(*flags, "--yes")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIsNotNone(self.setup_ran_with(), result.stdout + result.stderr)
        self.assertTrue((data / "wine-lens-setup/setup.sh").is_file())
        self.assertFalse((data / "wine-lens").exists(), "the setup's files are where the Wine side is built")
        self.assertIn(f"{data}/install.sh --uninstall", result.stdout)
        kept = self.wine_side(data)
        # An update: the same again from a newer archive, here without the Odyssey 3D step.
        result = self.run_installer(*flags, "--no-odyssey")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        for file in kept:
            self.assertTrue(file.is_file(), f"installing again removed {file}")
        # Nobody answers the question about the Wine side: it stays, and so does the rest of the data.
        result = subprocess.run([*SHELL, str(data / "install.sh"), "--uninstall"], cwd=self.root, env=self.env,
                                text=True, capture_output=True, timeout=30, stdin=subprocess.DEVNULL)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        for file in kept:
            self.assertTrue(file.is_file(), f"--uninstall removed {file}")
        self.assertFalse((data / "install.sh").exists())
        self.assertFalse((data / "wine-lens-setup").exists())
        self.assertFalse((home / ".local/bin/any2odyssey").exists())
        # Answered with yes, the Wine side goes and nothing else of the data does.
        result = self.run_installer("--uninstall", "--yes")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertFalse((data / "wine-lens").exists())
        for file in kept[:2]:
            self.assertTrue(file.is_file(), f"--uninstall --yes removed {file}")

    def test_the_setup_0_9_1_left_in_the_data_folder_goes_and_a_wine_side_built_there_stays(self):
        home, data = self.usual_place()
        old = data / "wine-lens"
        (old / "tools").mkdir(parents=True)
        (old / "setup.sh").write_text("#!/bin/sh\n")
        (old / "tools/tracking.sh").write_text("#!/bin/sh\n")
        flags = ["--no-deps", "--no-udev", "--no-model", "--no-odyssey"]
        result = self.run_installer(*flags)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertFalse(old.exists(), "the earlier version's copy of the setup is still there")
        # The same with a Wine side that 0.9.1's setup.sh built before it failed.
        (old / "prefix").mkdir(parents=True)
        (old / "setup.sh").write_text("#!/bin/sh\n")
        result = self.run_installer(*flags)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertTrue((old / "prefix").is_dir())
        result = self.run_installer("--uninstall")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertTrue((old / "prefix").is_dir())
        # Without anything else in it, the folder itself goes with the installation.
        shutil.rmtree(old)
        result = self.run_installer(*flags)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        result = self.run_installer("--uninstall")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertFalse(data.exists())

    def test_the_archive_in_a_folder_named_like_an_installation_still_installs_to_the_usual_place(self):
        # Only the installed copy, which has no program beside it, takes the prefix from where it is.
        archive = self.root / "share" / "any2odyssey"
        archive.parent.mkdir()
        self.archive.rename(archive)
        self.archive = archive
        home = self.root / "home"
        home.mkdir()
        self.env["HOME"] = str(home)
        result = self.run_installer("--no-deps", "--no-udev", "--no-model", "--no-odyssey")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertTrue((home / ".local/bin/any2odyssey").is_file())
        self.assertFalse((self.root / "bin").exists())

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

    def test_an_installed_library_is_not_reported_missing_when_another_folder_has_none(self):
        # Without ldconfig (not on an ordinary user's PATH on Debian) the two usual folders were listed in
        # one ls, which fails when either pattern matches nothing: every library was reported missing.
        tools = self.root / "tools"
        tools.mkdir()
        (tools / "pacman").write_text("#!/bin/sh\nexit 0\n")  # the Arch names, on any system
        (tools / "pacman").chmod(0o755)
        self.env["PATH"] = f"{tools}{os.pathsep}{self.env['PATH']}"
        for library in ["usr/lib/x86_64-linux-gnu/libxkbcommon.so.0", "usr/lib64/libGL.so.1.7.0"]:
            (self.system / library).parent.mkdir(parents=True, exist_ok=True)
            (self.system / library).touch()
        result = self.install(self.root / "prefix")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        lines = [line for line in result.stdout.splitlines() if line.startswith("   missing packages: ")]
        self.assertEqual(len(lines), 1, result.stdout)
        missing = lines[0].removeprefix("   missing packages: ").split()
        self.assertIn("libpipewire", missing)
        self.assertNotIn("libxkbcommon", missing)
        self.assertNotIn("libglvnd", missing)

    def test_a_desktop_entry_that_is_a_link_is_replaced_and_not_written_through(self):
        # The program and the icon replaced a link; the entry was written into whatever the link named
        # (as root in a user's home: any file of the system).
        prefix = self.root / "prefix"
        other = self.root / "another file"
        other.write_text("not the installer's\n")
        entry = prefix / "share/applications/any2odyssey.desktop"
        entry.parent.mkdir(parents=True)
        entry.symlink_to(other)
        result = self.install(prefix)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(other.read_text(), "not the installer's\n")
        self.assertFalse(entry.is_symlink())
        self.assertIn(f'Exec="{prefix}/bin/any2odyssey" %f\n', entry.read_text())
        self.assertEqual([path.name for path in entry.parent.glob("any2odyssey.desktop*")], ["any2odyssey.desktop"])


if __name__ == "__main__":
    unittest.main()
