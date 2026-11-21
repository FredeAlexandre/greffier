"""Tests de l'empaquetage de Greffier.app : `python3 -m unittest discover -s scripts`."""
import os
from pathlib import Path
import plistlib
import tempfile
import unittest

import installer_app

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
        installer_app.write_bundle(self.app, self.binary, CARGO_TOML)

    def info(self):
        with (self.app / "Contents/Info.plist").open("rb") as file:
            return plistlib.load(file)

    def test_l_executable_est_dans_le_bundle(self):
        executable = self.app / "Contents/MacOS/greffier"
        self.assertEqual(executable.read_bytes(), b"#!/bin/sh\n")
        self.assertTrue(os.access(executable, os.X_OK))
        self.assertEqual(self.info()["CFBundleExecutable"], "greffier")

    def test_identite_stable_et_version_du_projet(self):
        info = self.info()
        self.assertEqual(info["CFBundleIdentifier"], "io.github.fredealexandre.greffier")
        self.assertEqual(info["CFBundleName"], "Greffier")
        self.assertEqual(info["CFBundlePackageType"], "APPL")
        self.assertEqual(info["CFBundleShortVersionString"], "1.2.3")
        self.assertEqual(info["CFBundleVersion"], "1.2.3")

    def test_explique_l_usage_de_l_ecran_du_micro_et_du_son_du_systeme(self):
        info = self.info()
        for key in ("NSScreenCaptureUsageDescription", "NSMicrophoneUsageDescription",
                    "NSAudioCaptureUsageDescription"):
            with self.subTest(key):
                self.assertIn("Greffier", info.get(key, ""))


class EntitlementsTest(unittest.TestCase):
    def test_le_micro_passe_le_runtime_durci_hors_bac_a_sable(self):
        path = Path(tempfile.mkdtemp()) / "Greffier.entitlements"
        installer_app.write_entitlements(path)
        with path.open("rb") as file:
            entitlements = plistlib.load(file)
        self.assertIs(entitlements["com.apple.security.device.audio-input"], True)
        self.assertNotIn("com.apple.security.app-sandbox", entitlements)


# Sortie réelle de `security find-identity -p codesigning` : une identité autosignée non approuvée
# est listée avec un suffixe, et chaque identité apparaît deux fois si elle est valide.
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
    def test_trouve_greffier_dev_meme_non_approuvee(self):
        self.assertEqual(installer_app.signing_identity(FIND_IDENTITY),
                         "FC2C7014A4F8CED15F2419D8A9E4A530BB914DBE")

    def test_absente(self):
        listing = FIND_IDENTITY.replace('"Greffier Dev" (CSSMERR_TP_NOT_TRUSTED)', '"Autre"')
        self.assertIsNone(installer_app.signing_identity(listing))


if __name__ == "__main__":
    unittest.main()
