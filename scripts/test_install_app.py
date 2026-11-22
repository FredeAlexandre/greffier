"""Tests for packaging Greffier.app: `python3 -m unittest discover -s scripts`."""
import hashlib
import os
from pathlib import Path
import plistlib
import tarfile
import tempfile
import unittest

import install_app

CARGO_TOML = """\
[package]
name = "greffier"
version = "1.2.3"
edition = "2024"

[dependencies]
rune = { git = "https://example.invalid/rune", version = "9.9.9" }
"""


class AppBundleTest(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp())
        self.binary = self.tmp / "greffier"
        self.binary.write_bytes(b"#!/bin/sh\n")
        self.binary.chmod(0o755)
        self.app = self.tmp / "Greffier.app"
        install_app.write_bundle(self.app, self.binary, CARGO_TOML)

    def info(self):
        with (self.app / "Contents/Info.plist").open("rb") as file:
            return plistlib.load(file)

    def test_the_executable_is_in_the_bundle(self):
        executable = self.app / "Contents/MacOS/greffier"
        self.assertEqual(executable.read_bytes(), b"#!/bin/sh\n")
        self.assertTrue(os.access(executable, os.X_OK))
        self.assertEqual(self.info()["CFBundleExecutable"], "greffier")

    def test_stable_identity_and_project_version(self):
        info = self.info()
        self.assertEqual(info["CFBundleIdentifier"], "io.github.fredealexandre.greffier")
        self.assertEqual(info["CFBundleName"], "Greffier")
        self.assertEqual(info["CFBundlePackageType"], "APPL")
        self.assertEqual(info["CFBundleShortVersionString"], "1.2.3")
        self.assertEqual(info["CFBundleVersion"], "1.2.3")

    def test_explains_use_of_screen_microphone_and_system_audio(self):
        info = self.info()
        for key in ("NSScreenCaptureUsageDescription", "NSMicrophoneUsageDescription",
                    "NSAudioCaptureUsageDescription"):
            with self.subTest(key):
                self.assertIn("Greffier", info.get(key, ""))


class BundledUvTest(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp())

    def archive(self, content=b"#!/bin/sh\necho uv\n"):
        source = self.tmp / "uv"
        source.write_bytes(content)
        archive = self.tmp / "uv.tar.gz"
        with tarfile.open(archive, "w:gz") as tar:
            tar.add(source, arcname="uv-aarch64-apple-darwin/uv")
        return archive, hashlib.sha256(archive.read_bytes()).hexdigest()

    def test_same_version_as_the_core(self):
        mac_rs = (Path(__file__).resolve().parent.parent / "tools/src/mac.rs").read_text()
        self.assertEqual(install_app.uv_version_in_core(mac_rs), install_app.UV_VERSION)

    def test_extracts_uv_from_an_archive_with_the_right_sha256(self):
        archive, sha = self.archive()
        uv = install_app.unpack_uv(archive, sha, self.tmp / "extracted")
        self.assertEqual(uv.read_bytes(), b"#!/bin/sh\necho uv\n")
        self.assertTrue(os.access(uv, os.X_OK))

    def test_rejects_an_archive_with_the_wrong_sha256(self):
        archive, _ = self.archive()
        with self.assertRaises(install_app.Failure):
            install_app.unpack_uv(archive, "0" * 64, self.tmp / "extracted")
        self.assertFalse((self.tmp / "extracted").exists())

    def test_uv_is_next_to_the_executable_in_the_bundle(self):
        archive, sha = self.archive()
        uv = install_app.unpack_uv(archive, sha, self.tmp / "uv-extracted")
        binary = self.tmp / "greffier"
        binary.write_bytes(b"#!/bin/sh\n")
        app = self.tmp / "Greffier.app"
        install_app.write_bundle(app, binary, CARGO_TOML, uv)
        self.assertTrue(os.access(app / "Contents/MacOS/uv", os.X_OK))


class EntitlementsTest(unittest.TestCase):
    def test_microphone_passes_the_hardened_runtime_without_sandbox(self):
        path = Path(tempfile.mkdtemp()) / "Greffier.entitlements"
        install_app.write_entitlements(path)
        with path.open("rb") as file:
            entitlements = plistlib.load(file)
        self.assertIs(entitlements["com.apple.security.device.audio-input"], True)
        self.assertNotIn("com.apple.security.app-sandbox", entitlements)


# Real output of `security find-identity -p codesigning`: an untrusted self-signed identity is
# listed with a suffix, and each identity appears twice if it is valid.
FIND_IDENTITY = """\

Policy: Code Signing
  Matching identities
  1) B4DE18D418E97A6F781FA2A3529233162141CEA3 "Beamer Dev"
  2) FC2C7014A4F8CED15F2419D8A9E4A530BB914DBE "Greffier Dev" (CSSMERR_TP_NOT_TRUSTED)
  3) 0123456789ABCDEF0123456789ABCDEF01234567 "Greffier Dev Old"
     3 identities found

  Valid identities only
  1) B4DE18D418E97A6F781FA2A3529233162141CEA3 "Beamer Dev"
     1 valid identities found
"""


class SigningIdentityTest(unittest.TestCase):
    def test_finds_greffier_dev_even_untrusted(self):
        self.assertEqual(install_app.signing_identity(FIND_IDENTITY),
                         "FC2C7014A4F8CED15F2419D8A9E4A530BB914DBE")

    def test_missing(self):
        listing = FIND_IDENTITY.replace('"Greffier Dev" (CSSMERR_TP_NOT_TRUSTED)', '"Other"')
        self.assertIsNone(install_app.signing_identity(listing))


if __name__ == "__main__":
    unittest.main()
