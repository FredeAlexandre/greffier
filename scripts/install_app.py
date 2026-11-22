"""Builds Greffier.app, signs it as "Greffier Dev" and installs it in /Applications."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import plistlib
import re
import secrets
import shutil
import subprocess
import sys
import tarfile
import tempfile
import time
import tomllib
import urllib.request

NAME = "Greffier"
EXE = "greffier"
# Never changes: macOS ties permissions to this identifier and to the signing identity.
BUNDLE_ID = "io.github.fredealexandre.greffier"
IDENTITY = "Greffier Dev"
# The bundled uv, which installs pyannote's private Python (ADR 0003). Same version as
# `UV_VERSION` in tools/src/mac.rs, which looks for it next to the executable.
UV_VERSION = "0.13.7"
UV_ARCHIVE = "uv-aarch64-apple-darwin.tar.gz"
UV_SHA256 = "81b811b57e4624df7ed3aac148e0359100d9962f3d9dce00aec533dc2e086731"
UV_URL = f"https://github.com/astral-sh/uv/releases/download/{UV_VERSION}/{UV_ARCHIVE}"


def project_version(cargo_toml):
    return tomllib.loads(cargo_toml)["package"]["version"]


def info_plist(version):
    return dict(
        CFBundleName=NAME,
        CFBundleDisplayName=NAME,
        CFBundleIdentifier=BUNDLE_ID,
        CFBundleExecutable=EXE,
        CFBundlePackageType="APPL",
        CFBundleInfoDictionaryVersion="6.0",
        CFBundleShortVersionString=version,
        CFBundleVersion=version,
        LSMinimumSystemVersion="13.0",
        NSHighResolutionCapable=True,
        NSScreenCaptureUsageDescription=
            "Greffier enregistre l'écran pendant une réunion pour en garder la vidéo.",
        NSMicrophoneUsageDescription=
            "Greffier enregistre votre micro pendant une réunion pour la transcrire.",
        NSAudioCaptureUsageDescription=
            "Greffier enregistre le son du Mac pendant une réunion pour entendre les autres participants.",
    )


def write_entitlements(path):
    # Screen and system audio capture have no entitlement: only the macOS permission counts. The
    # microphone, though, is silently denied by the hardened runtime without this one. No sandbox:
    # Greffier runs whisper, uv and the `claude` CLI, and writes into the meetings folder.
    with path.open("wb") as file:
        plistlib.dump({"com.apple.security.device.audio-input": True}, file)


def uv_version_in_core(mac_rs):
    """The uv version the core expects (`UV_VERSION` in tools/src/mac.rs)."""
    found = re.search(r'UV_VERSION: &str = "([^"]+)"', mac_rs)
    return found[1] if found else None


def unpack_uv(archive, sha256, dest):
    """Checks the sha256 of the uv archive and extracts the `uv` executable from it into `dest`."""
    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    if digest != sha256:
        raise Failure(f"unexpected sha256 for {archive.name}: {digest} (expected {sha256}).")
    with tarfile.open(archive) as tar:
        member = next((m for m in tar.getmembers() if m.isfile() and Path(m.name).name == "uv"), None)
        if member is None:
            raise Failure(f"no uv executable in {archive.name}.")
        with tar.extractfile(member) as source, open(dest, "wb") as out:
            shutil.copyfileobj(source, out)
    dest.chmod(0o755)
    return dest


def fetch_uv(target):
    """The bundled uv, downloaded once into target/uv-<version>/ (where `cargo run` finds it too)."""
    folder = target / f"uv-{UV_VERSION}"
    uv = folder / "uv"
    if uv.is_file():
        return uv
    step(f"Downloading uv {UV_VERSION}")
    folder.mkdir(parents=True, exist_ok=True)
    archive = folder / UV_ARCHIVE
    with urllib.request.urlopen(UV_URL) as response, archive.open("wb") as out:
        shutil.copyfileobj(response, out)
    try:
        return unpack_uv(archive, UV_SHA256, uv)
    finally:
        archive.unlink(missing_ok=True)


def write_bundle(app, binary, cargo_toml, uv=None):
    """Writes the unsigned bundle `app` around the executable `binary` (and the bundled uv)."""
    macos = app / "Contents/MacOS"
    macos.mkdir(parents=True)
    (app / "Contents/Resources").mkdir()
    shutil.copy2(binary, macos / EXE)
    if uv is not None:
        shutil.copy2(uv, macos / "uv")
    with (app / "Contents/Info.plist").open("wb") as file:
        plistlib.dump(info_plist(project_version(cargo_toml)), file)


# --- Build, sign, install -----------------------------------------------------------------------

ROOT = Path(__file__).resolve().parent.parent
LIVE = Path("/Applications") / f"{NAME}.app"
LIVE_EXE = LIVE / "Contents/MacOS" / EXE
KEYCHAIN = Path.home() / "Library/Keychains/login.keychain-db"
LSREGISTER = ("/System/Library/Frameworks/CoreServices.framework/Frameworks/"
              "LaunchServices.framework/Support/lsregister")


class Failure(Exception):
    """A step failed; the message says what to do."""


def run(*args, check=True, **kwargs):
    result = subprocess.run([str(arg) for arg in args], capture_output=True, text=True, **kwargs)
    if check and result.returncode:
        raise Failure(f"{args[0]} failed:\n{result.stderr.strip() or result.stdout.strip()}")
    return result


def step(text):
    print(f"==> {text}", flush=True)


def signing_identity(listing):
    """The SHA-1 hash of "Greffier Dev" in a `security find-identity` output, or None."""
    pattern = re.compile(rf'^\s*\d+\) ([0-9A-F]{{40}}) "{re.escape(IDENTITY)}"( |$)')
    hashes = sorted({m[1] for line in listing.splitlines() if (m := pattern.match(line))})
    if len(hashes) > 1:
        raise Failure(f"several \"{IDENTITY}\" identities in the keychain ({', '.join(hashes)}): "
                      "keep the one that signed the installed app, delete the others in Keychain Access.")
    return hashes[0] if hashes else None


def identity_hash():
    # No -v: the identity does not need to be trusted to sign; the app's designated requirement
    # keeps the certificate hash, which is enough for macOS to recognize the app across versions.
    return signing_identity(run("security", "find-identity", "-p", "codesigning", KEYCHAIN).stdout)


def create_identity():
    """Creates a self-signed code signing certificate and imports it into the login keychain."""
    step(f"Creating the signing identity \"{IDENTITY}\" in the login keychain")
    print("    If the keychain is locked, macOS will ask for your login password.")
    with tempfile.TemporaryDirectory() as tmp:
        key, cert, p12 = (Path(tmp) / name for name in ("key.pem", "cert.pem", "id.p12"))
        password = secrets.token_hex(16)
        # The system openssl (LibreSSL) writes a .p12 that `security import` can read.
        run("/usr/bin/openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "3650",
            "-keyout", key, "-out", cert, "-subj", f"/CN={IDENTITY}",
            "-addext", "basicConstraints=critical,CA:FALSE",
            "-addext", "keyUsage=critical,digitalSignature",
            "-addext", "extendedKeyUsage=critical,codeSigning")
        run("/usr/bin/openssl", "pkcs12", "-export", "-inkey", key, "-in", cert, "-out", p12,
            "-name", IDENTITY, "-passout", f"pass:{password}")
        # -T: codesign can use the key without asking for access at every signature.
        imported = run("security", "import", p12, "-k", KEYCHAIN, "-P", password,
                       "-T", "/usr/bin/codesign", check=False)
    if imported.returncode:
        raise Failure(
            f"macOS refused to import the identity \"{IDENTITY}\" ({imported.stderr.strip()}).\n"
            "The login keychain is probably locked. Unlock it (login password):\n"
            f"    security unlock-keychain {KEYCHAIN}\n"
            "then run again: scripts/install-app")
    found = identity_hash()
    if not found:
        raise Failure(f"the identity \"{IDENTITY}\" was imported but codesign does not see it.")
    print(f"    Identity created: {found}")
    return found


def cargo():
    return shutil.which("cargo") or str(Path.home() / ".cargo/bin/cargo")


def build_binary():
    step("Building in release (cargo build --release)")
    if subprocess.run([cargo(), "build", "--release"], cwd=ROOT).returncode:
        raise Failure("the build failed.")
    return ROOT / "target/release" / EXE


def sign(app, entitlements, identity):
    step(f"Signing with \"{IDENTITY}\" ({identity})")
    # Nested code first: codesign refuses to sign an app with an unsigned executable inside.
    helper = app / "Contents/MacOS/uv"
    if helper.exists():
        run("codesign", "--force", "--options", "runtime", "--timestamp=none", "--sign", identity, helper)
    run("codesign", "--force", "--options", "runtime", "--timestamp=none",
        "--entitlements", entitlements, "--sign", identity, app)
    run("codesign", "--verify", "--strict", app)
    requirement = run("codesign", "-d", "-r-", app).stdout.strip().splitlines()[-1]
    print(f"    {requirement}")


def running_pids():
    found = run("pgrep", "-f", f"^{LIVE_EXE}( |$)", check=False).stdout.split()
    return [int(pid) for pid in found]


def quit_running_app():
    if not running_pids():
        return
    step("Greffier is open: asking it to quit")
    run("osascript", "-e", f'tell application id "{BUNDLE_ID}" to quit', check=False)
    deadline = time.monotonic() + 15
    while running_pids():
        if time.monotonic() > deadline:
            raise Failure("Greffier did not quit (a recording in progress?). "
                          "Quit it, then run scripts/install-app again.")
        time.sleep(0.2)


def install(app):
    """Replaces /Applications/Greffier.app with `app`, never leaving a moment with no app installed."""
    step(f"Installing into {LIVE}")
    quit_running_app()
    stage = Path(tempfile.mkdtemp(prefix=f".{NAME}.app.install-", dir=LIVE.parent))
    try:
        new, old = stage / f"{NAME}.app", stage / f"{NAME}.old.app"
        run("ditto", app, new)
        if LIVE.exists():
            os.rename(LIVE, old)
        try:
            os.rename(new, LIVE)
        except OSError:
            if old.exists():
                os.rename(old, LIVE)
            raise
    finally:
        shutil.rmtree(stage, ignore_errors=True)
    run(LSREGISTER, "-f", LIVE, check=False)


def launch():
    """Opens the app like the Finder does and waits for Rune to report its first window ready."""
    step("Launching the installed app")
    with tempfile.TemporaryDirectory() as ready:
        run("open", "--env", f"RUNE_READY_DIR={ready}", LIVE)
        deadline = time.monotonic() + 20
        while time.monotonic() < deadline:
            for receipt in Path(ready).glob(f"{BUNDLE_ID}.*"):
                try:
                    data = json.loads(receipt.read_text())
                except (OSError, ValueError):
                    continue
                print(f"    Window ready in {data.get('ready_ms', '?')} ms (pid {data.get('pid')}).")
                return
            time.sleep(0.2)
    raise Failure("the installed app did not show its window within 20 s. "
                  f"Try opening it from the Finder; log: Console.app, process \"{EXE}\".")


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, help="package this executable instead of building")
    parser.add_argument("--no-launch", action="store_true", help="do not open the installed app")
    args = parser.parse_args(argv)
    try:
        identity = identity_hash() or create_identity()
        binary = args.binary or build_binary()
        if not os.access(binary, os.X_OK):
            raise Failure(f"no executable at {binary}")
        target = ROOT / "target"
        if uv_version_in_core((ROOT / "tools/src/mac.rs").read_text()) != UV_VERSION:
            raise Failure(f"tools/src/mac.rs does not expect uv {UV_VERSION}: align UV_VERSION in both files.")
        uv = fetch_uv(target)
        step("Building the bundle")
        app, entitlements = target / f"{NAME}.app", target / f"{NAME}.entitlements"
        shutil.rmtree(app, ignore_errors=True)
        write_bundle(app, binary, (ROOT / "Cargo.toml").read_text(), uv)
        write_entitlements(entitlements)
        sign(app, entitlements, identity)
        install(app)
        if not args.no_launch:
            launch()
    except Failure as error:
        print(f"install-app: {error}", file=sys.stderr)
        return 1
    print(f"{LIVE} installed ({identity}).")
    return 0


if __name__ == "__main__":
    sys.exit(main())
