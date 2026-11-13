#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Exercise the real loopback bridge without a camera, Wine, or root."""
import hashlib
import hmac
import os
from pathlib import Path
import socket
import struct
import subprocess
import tempfile
import threading
import time
import unittest


class CameraBridgeTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.directory = tempfile.TemporaryDirectory()
        cls.root = Path(cls.directory.name)
        cls.binary = os.environ.get("CAMERA_BRIDGE_TEST_BINARY", str(cls.root / "bridge"))
        if "CAMERA_BRIDGE_TEST_BINARY" not in os.environ:
            source = Path(__file__).resolve().parents[1] / "src/camera-bridge.c"
            subprocess.run(["cc", "-O2", "-Wall", "-Wextra", "-Werror", "-o", cls.binary, str(source)], check=True)
        cls.key = b"camera-test-secret-do-not-use-in-production"
        cls.secret = cls.root / "secret"
        cls.secret.write_bytes(cls.key)
        cls.secret.chmod(0o600)

    @classmethod
    def tearDownClass(cls):
        cls.directory.cleanup()

    def setUp(self):
        with socket.socket() as reservation:
            reservation.bind(("127.0.0.1", 0))
            self.port = reservation.getsockname()[1]
        self.process = subprocess.Popen([self.binary, "/dev/null", str(self.port), str(self.secret)])
        self.addCleanup(self.stop)
        deadline = time.monotonic() + 5
        while True:
            try:
                self.client = self.connect()
                self.addCleanup(self.client.close)
                break
            except ConnectionRefusedError:
                if self.process.poll() is not None or time.monotonic() >= deadline:
                    self.fail("bridge did not start")
                time.sleep(0.01)

    def stop(self):
        self.process.terminate()
        self.process.wait(timeout=5)

    def connect(self):
        return socket.create_connection(("127.0.0.1", self.port), timeout=2)

    def packet(self, client):
        def exact(length):
            result = b""
            while len(result) < length:
                part = client.recv(length - len(result))
                self.assertTrue(part, "unexpected EOF")
                result += part
            return result
        kind, length, _ = struct.unpack("<c3xIQ", exact(16))
        self.assertEqual(kind, b"R")
        self.assertLessEqual(length, 8192)
        return exact(length)

    def test_valid_authentication_and_commands(self):
        self.client.sendall(b"H\n")
        challenge = self.packet(self.client)
        self.assertEqual(len(challenge), 32)
        answer = hmac.new(self.key, challenge, hashlib.sha256).hexdigest().encode()
        self.client.sendall(b"A " + answer + b"\n")
        self.assertEqual(self.packet(self.client), b"ok")
        # /dev/null opens successfully but exposes no V4L2 formats.
        self.client.sendall(b"L\n")
        self.assertEqual(self.packet(self.client), b"")

    def test_wrong_answer_is_rejected(self):
        self.client.sendall(b"H\n")
        self.packet(self.client)
        self.client.sendall(b"A " + b"0" * 64 + b"\n")
        self.assertEqual(self.client.recv(1), b"")

    def test_silent_client_expires(self):
        self.client.settimeout(7)
        started = time.monotonic()
        self.assertEqual(self.client.recv(1), b"")
        self.assertLess(time.monotonic() - started, 6.5)

    def test_partial_handshake_cannot_extend_deadline(self):
        started = time.monotonic()
        stopped = threading.Event()

        def trickle():
            # Delay the challenge, then dribble an unfinished answer. Neither
            # stage may extend the five seconds measured from acceptance.
            for part in [b"H", b"\n", b"A", b" ", b"0", b"0", b"0"]:
                if stopped.wait(1):
                    return
                try:
                    self.client.sendall(part)
                except OSError:
                    return

        writer = threading.Thread(target=trickle)
        writer.start()
        try:
            self.client.settimeout(4)
            self.assertEqual(len(self.packet(self.client)), 32)
            self.client.settimeout(max(0.1, 6.5 - (time.monotonic() - started)))
            self.assertEqual(self.client.recv(1), b"")
            self.assertLess(time.monotonic() - started, 6.5)
        finally:
            stopped.set()
            writer.join(timeout=2)
        # Expiry releases the single-client slot for a fresh handshake.
        with self.connect() as next_client:
            next_client.sendall(b"H\n")
            self.assertEqual(len(self.packet(next_client)), 32)


if __name__ == "__main__":
    unittest.main()
