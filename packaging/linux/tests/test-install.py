#!/usr/bin/env python3
"""Exercise release-archive installation without root, downloads or hardware.

Requires gio (libglib2.0-bin) and desktop-file-validate (desktop-file-utils).
The installed fixture only records its argv; it never opens an image or a UI.
"""

import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time
import unittest


LINUX = Path(__file__).resolve().parents[1]


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
        (self.archive / "wine-lens" / "setup.sh").write_text("#!/bin/sh\nexit 0\n")
        self.log = self.root / "launched-argv"
        self.env = dict(os.environ, ANY2ODYSSEY_DATA_DIR=str(self.root / "data"),
                        A2O_INSTALL_TEST_LOG=str(self.log))

    def install(self, prefix, *extra):
        return subprocess.run(
            ["sh", str(self.archive / "install.sh"), "--prefix", str(prefix),
             "--no-deps", "--no-udev", "--no-model", "--no-odyssey", *extra],
            cwd=self.root, env=self.env, text=True, capture_output=True, timeout=30,
        )

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

    def test_empty_or_multiline_prefix_is_rejected_before_installing(self):
        for prefix in ["", str(self.root / "bad\nline"), str(self.root / "bad\rline")]:
            with self.subTest(prefix=prefix):
                result = self.install(prefix, "--dry-run")
                self.assertEqual(result.returncode, 2, result.stdout + result.stderr)
                self.assertIn("prefix", result.stderr)


if __name__ == "__main__":
    unittest.main()
