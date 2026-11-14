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
        # The minimum supported key length remains usable with CRLF line endings.
        cls.key = b"camera-test-key!"  # 16 bytes; test only
        cls.secret = cls.root / "secret"
        cls.secret.write_bytes(cls.key + b"\r\n")
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

    def assert_dropped(self, client):
        # The bridge closing while a byte of ours is still unread shows as a reset instead of an end.
        try:
            self.assertEqual(client.recv(1), b"")
        except ConnectionResetError:
            pass

    def authenticate(self, client):
        nonce = os.urandom(16).hex().encode()
        challenge = self.challenge(client, nonce)
        client.sendall(b"A " + self.proof(b"client", challenge, nonce) + b"\n")
        # The bridge proves in turn that it knows the secret, over both random numbers.
        self.assertEqual(self.packet(client), b"ok " + self.proof(b"bridge", nonce, challenge))

    def proof(self, label, first, second):
        return hmac.new(self.key, label + b":" + first + b":" + second, hashlib.sha256).hexdigest().encode()

    def challenge(self, client, nonce):
        client.sendall(b"H " + nonce + b"\n")
        challenge = self.packet(client)
        self.assertRegex(challenge, b"^[0-9a-f]{32}$")
        return challenge

    def test_valid_authentication_and_commands(self):
        self.authenticate(self.client)
        # /dev/null opens successfully but exposes no V4L2 formats.
        self.client.sendall(b"L\n")
        self.assertEqual(self.packet(self.client), b"")

    def test_short_secret_is_rejected_after_removing_line_endings(self):
        secret = self.root / "malformed-secret"
        for contents in [b"x" * 15 + b"\n", b"\r" + b"x" * 20, b"x\r" + b"y" * 20]:
            with self.subTest(contents=contents):
                secret.write_bytes(contents)
                process = subprocess.Popen([self.binary, "/dev/null", "0", str(secret)],
                                           stdout=subprocess.PIPE, stderr=subprocess.PIPE)
                try:
                    _, error = process.communicate(timeout=2)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.communicate()
                    self.fail("bridge started with a secret shorter than 16 bytes after trimming")
                self.assertEqual(process.returncode, 1)
                self.assertIn(b"no usable secret", error)

    def test_challenges_differ(self):
        nonce = b"0123456789abcdef" * 2
        first = self.challenge(self.client, nonce)
        self.client.close()
        with self.connect() as again:
            self.assertNotEqual(self.challenge(again, nonce), first)

    def test_wrong_answer_is_rejected(self):
        nonce = os.urandom(16).hex().encode()
        self.challenge(self.client, nonce)
        self.client.sendall(b"A " + b"0" * 64 + b"\n")
        # Dropped without a word: in particular without the bridge's own proof.
        self.assert_dropped(self.client)

    def test_bridge_proof_is_not_accepted_as_the_clients(self):
        # What the bridge would answer cannot be handed back to it as a client's answer.
        nonce = os.urandom(16).hex().encode()
        challenge = self.challenge(self.client, nonce)
        self.client.sendall(b"A " + self.proof(b"bridge", nonce, challenge) + b"\n")
        self.assert_dropped(self.client)

    def test_answer_is_bound_to_the_clients_number(self):
        nonce = os.urandom(16).hex().encode()
        challenge = self.challenge(self.client, nonce)
        self.client.sendall(b"A " + self.proof(b"client", challenge, b"f" * 32) + b"\n")
        self.assert_dropped(self.client)

    def test_malformed_greeting_is_rejected(self):
        self.client.close()  # one client at a time
        for greeting in [b"H\n", b"H \n", b"H " + b"g" * 32 + b"\n", b"H " + b"0" * 31 + b"\n",
                         b"H " + b"0" * 33 + b"\n", b"H " + b"A" * 32 + b"\n", b"L\n"]:
            with self.subTest(greeting=greeting), self.connect() as client:
                client.sendall(greeting)
                self.assert_dropped(client)

    def test_silent_client_expires(self):
        # Within a second or so: only one client is served at a time, and the real one must not be kept
        # waiting by connections that say nothing.
        self.client.settimeout(5)
        started = time.monotonic()
        self.assert_dropped(self.client)
        self.assertLess(time.monotonic() - started, 2.5)

    def test_partial_handshake_cannot_extend_deadline(self):
        started = time.monotonic()
        stopped = threading.Event()

        def trickle():
            # Delay the challenge, then dribble an unfinished answer. Neither
            # stage may extend the second measured from acceptance.
            for part in [b"H " + b"0" * 32, b"\n", b"A", b" ", b"0", b"0", b"0", b"0", b"0", b"0"]:
                if stopped.wait(0.2):
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
            self.client.settimeout(max(0.1, 2.5 - (time.monotonic() - started)))
            self.assert_dropped(self.client)
            self.assertLess(time.monotonic() - started, 2.5)
        finally:
            stopped.set()
            writer.join(timeout=2)
        # Expiry releases the single-client slot for a fresh handshake.
        with self.connect() as next_client:
            self.challenge(next_client, b"0" * 32)

    def test_client_that_stops_reading_is_dropped(self):
        # Ask for replies and take none. Once nothing more fits the bridge is stuck in a send; it has to
        # give this client up after a few seconds, or the camera would stay with a client that hangs.
        self.authenticate(self.client)
        self.client.setblocking(False)
        flood = b"L\n" * 32768
        started = time.monotonic()
        stuck = None
        while True:
            try:
                self.client.send(flood)
            except BlockingIOError:
                stuck = stuck or time.monotonic()
                time.sleep(0.05)
            except (BrokenPipeError, ConnectionResetError):
                break
            self.assertLess(time.monotonic() - started, 40, "the bridge kept a client that reads nothing")
        self.assertIsNotNone(stuck)
        # The single-client slot is free again.
        with self.connect() as next_client:
            self.authenticate(next_client)


if __name__ == "__main__":
    unittest.main()
