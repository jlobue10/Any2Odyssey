#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
"""scripts/apt-install.sh: when it tries again, and when it gives up.

In a workflow it only ever shows that the packages were installed. Here apt-get, timeout and sudo are
stand-ins: apt-get notes how it was called and ends each call with the code the test has planned for it
(124 is what timeout answers for a call it had to stop). Nothing is installed and no network is used, and
the pause between two tries is set to nothing (a shell with a sleep of its own would take it in full).
A2O_TEST_SHELL runs the script under another shell (dash, "busybox sh").
"""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[2] / "scripts" / "apt-install.sh"
SHELL = os.environ.get("A2O_TEST_SHELL", "sh").split()
UPDATE = "update"
DOWNLOAD = "install -y --no-install-recommends --download-only one two"
INSTALL = "install -y --no-install-recommends one two"


class AptInstall(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        tools = self.root / "tools"
        tools.mkdir()
        stand_ins = {
            # Its call without the options, and the planned code of the call with this number.
            "apt-get": 'while [ "$1" = -o ]; do shift 2; done\necho "$*" >> "$A2O_TEST_ROOT/apt-get"\n'
                       'number=$(wc -l < "$A2O_TEST_ROOT/apt-get")\n'
                       'code=$(sed -n "${number}p" "$A2O_TEST_ROOT/plan")\nexit "${code:-0}"\n',
            "timeout": 'echo "$1 $2 $3" >> "$A2O_TEST_ROOT/timeout"\nshift 2\nexec "$@"\n',
            "sudo": 'exec "$@"\n',
        }
        for name, body in stand_ins.items():
            (tools / name).write_text("#!/bin/sh\n" + body)
            (tools / name).chmod(0o755)
        self.env = dict(os.environ, PATH=f"{tools}{os.pathsep}{os.environ['PATH']}", A2O_TEST_ROOT=str(self.root),
                        A2O_APT_PAUSE="0")

    def run_script(self, plan, *arguments):
        """Run with the exit codes of apt-get's calls planned, in order (a call beyond the plan succeeds)."""
        for name in ("apt-get", "timeout"):
            (self.root / name).unlink(missing_ok=True)
        (self.root / "plan").write_text("".join(f"{code}\n" for code in plan))
        return subprocess.run([*SHELL, str(SCRIPT), *arguments], env=self.env, stdin=subprocess.DEVNULL,
                              capture_output=True, text=True, timeout=60)

    def noted(self, name):
        note = self.root / name
        return note.read_text().splitlines() if note.exists() else []

    def test_lists_download_and_installation_each_once_when_nothing_fails(self):
        done = self.run_script([], "--update", "one", "two")
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertEqual(self.noted("apt-get"), [UPDATE, DOWNLOAD, INSTALL])
        # The two parts that use the network have a time limit; the installation itself has none.
        self.assertEqual(self.noted("timeout"), ["--kill-after=10 60 apt-get", "--kill-after=10 300 apt-get"])
        self.assertEqual(done.stderr, "")

    def test_the_lists_are_fetched_only_when_asked_for(self):
        done = self.run_script([], "one", "two")
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertEqual(self.noted("apt-get"), [DOWNLOAD, INSTALL])

    def test_a_download_that_was_stopped_or_failed_is_tried_again(self):
        # Stopped by timeout (a slow mirror), then failed by itself, then through.
        done = self.run_script([0, 124, 0, 100, 0], "--update", "one", "two")
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertEqual(self.noted("apt-get"), [UPDATE, DOWNLOAD, UPDATE, DOWNLOAD, UPDATE, DOWNLOAD, INSTALL])
        self.assertIn("downloading the packages was stopped after 300 seconds, try 1 of 3", done.stderr)
        self.assertIn("downloading the packages failed (exit 100), try 2 of 3", done.stderr)

    def test_the_lists_are_fetched_again_before_a_download_is_tried_again(self):
        # apt-get update ends with 0 when a mirror could not be reached, and the lists that are there
        # then name files the mirror may not have any more: the download fails with 404 at every try
        # unless the lists are fetched again. Whether --update was given or not.
        for arguments, before in ((["--update", "one", "two"], [UPDATE]), (["one", "two"], [])):
            with self.subTest(arguments=arguments):
                done = self.run_script([0] * len(before) + [100, 0, 0], *arguments)
                self.assertEqual(done.returncode, 0, done.stderr)
                self.assertEqual(self.noted("apt-get"), [*before, DOWNLOAD, UPDATE, DOWNLOAD, INSTALL])
                # With the lists' time limit, not the download's.
                self.assertEqual(self.noted("timeout")[len(before):],
                                 ["--kill-after=10 300 apt-get", "--kill-after=10 60 apt-get", "--kill-after=10 300 apt-get"])

    def test_lists_that_cannot_be_fetched_again_do_not_end_the_tries(self):
        # The download decides: the second fetch of the lists fails or is stopped, the try after it gets through.
        for code in (100, 124):
            with self.subTest(code=code):
                done = self.run_script([0, 100, code, 0], "--update", "one", "two")
                self.assertEqual(done.returncode, 0, done.stderr)
                self.assertEqual(self.noted("apt-get"), [UPDATE, DOWNLOAD, UPDATE, DOWNLOAD, INSTALL])
                self.assertIn("the package lists could not be fetched again", done.stderr)

    def test_a_download_that_never_finishes_fails_and_nothing_is_installed(self):
        for code in (124, 137, 100):
            with self.subTest(code=code):
                done = self.run_script([code, 0, code, 0, code], "one", "two")
                self.assertEqual(done.returncode, 1, done.stdout + done.stderr)
                self.assertEqual(self.noted("apt-get"), [DOWNLOAD, UPDATE, DOWNLOAD, UPDATE, DOWNLOAD])
                self.assertIn("try 3 of 3", done.stderr)
                self.assertIn("the packages could not be downloaded; nothing was installed", done.stderr)

    def test_lists_that_cannot_be_fetched_fail_before_anything_else(self):
        done = self.run_script([124, 124, 124], "--update", "one", "two")
        self.assertEqual(done.returncode, 1, done.stdout + done.stderr)
        self.assertEqual(self.noted("apt-get"), [UPDATE, UPDATE, UPDATE])
        self.assertIn("the package lists could not be fetched; nothing was installed", done.stderr)
        # Through at the last try: it goes on.
        done = self.run_script([124, 100, 0], "--update", "one", "two")
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertEqual(self.noted("apt-get"), [UPDATE, UPDATE, UPDATE, DOWNLOAD, INSTALL])

    def test_an_installation_that_fails_fails_the_script(self):
        done = self.run_script([0, 100], "one", "two")
        self.assertEqual(done.returncode, 100, done.stdout + done.stderr)
        self.assertEqual(self.noted("apt-get"), [DOWNLOAD, INSTALL])

    def test_without_a_package_it_says_how_it_is_called(self):
        for arguments in ([], ["--update"]):
            with self.subTest(arguments=arguments):
                done = self.run_script([], *arguments)
                self.assertEqual(done.returncode, 2, done.stdout + done.stderr)
                self.assertIn("usage:", done.stderr)
                self.assertEqual(self.noted("apt-get"), [])


if __name__ == "__main__":
    unittest.main()
