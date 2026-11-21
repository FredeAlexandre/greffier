"""Construit Greffier.app, la signe « Greffier Dev » et l'installe dans /Applications."""
import argparse
import json
import os
from pathlib import Path
import plistlib
import re
import secrets
import shutil
import subprocess
import sys
import tempfile
import time
import tomllib

NAME = "Greffier"
EXE = "greffier"
# Ne change jamais : macOS rattache les autorisations à cet identifiant et à l'identité de signature.
BUNDLE_ID = "io.github.fredealexandre.greffier"
IDENTITY = "Greffier Dev"


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
    # La capture d'écran et du son du système n'a pas d'entitlement : seule l'autorisation macOS
    # compte. Le micro, lui, est refusé en silence par le runtime durci sans celui-ci. Pas de bac à
    # sable : Greffier lance whisper, uv et la CLI `claude`, et écrit dans le dossier de réunions.
    with path.open("wb") as file:
        plistlib.dump({"com.apple.security.device.audio-input": True}, file)


def write_bundle(app, binary, cargo_toml):
    """Écrit le bundle non signé `app` autour de l'exécutable `binary`."""
    macos = app / "Contents/MacOS"
    macos.mkdir(parents=True)
    (app / "Contents/Resources").mkdir()
    shutil.copy2(binary, macos / EXE)
    with (app / "Contents/Info.plist").open("wb") as file:
        plistlib.dump(info_plist(project_version(cargo_toml)), file)


# --- Construire, signer, installer -------------------------------------------------------------

ROOT = Path(__file__).resolve().parent.parent
LIVE = Path("/Applications") / f"{NAME}.app"
LIVE_EXE = LIVE / "Contents/MacOS" / EXE
KEYCHAIN = Path.home() / "Library/Keychains/login.keychain-db"
LSREGISTER = ("/System/Library/Frameworks/CoreServices.framework/Frameworks/"
              "LaunchServices.framework/Support/lsregister")


class Failure(Exception):
    """Une étape a échoué ; le message dit quoi faire."""


def run(*args, check=True, **kwargs):
    result = subprocess.run([str(arg) for arg in args], capture_output=True, text=True, **kwargs)
    if check and result.returncode:
        raise Failure(f"{args[0]} a échoué :\n{result.stderr.strip() or result.stdout.strip()}")
    return result


def step(text):
    print(f"==> {text}", flush=True)


def signing_identity(listing):
    """L'empreinte SHA-1 de « Greffier Dev » dans une sortie de `security find-identity`, ou None."""
    pattern = re.compile(rf'^\s*\d+\) ([0-9A-F]{{40}}) "{re.escape(IDENTITY)}"( |$)')
    hashes = sorted({m[1] for line in listing.splitlines() if (m := pattern.match(line))})
    if len(hashes) > 1:
        raise Failure(f"plusieurs identités « {IDENTITY} » dans le trousseau ({', '.join(hashes)}) : "
                      "gardez celle qui a signé l'app installée, supprimez les autres dans Trousseaux d'accès.")
    return hashes[0] if hashes else None


def identity_hash():
    # Sans -v : l'identité n'a pas à être approuvée pour signer ; la désignation de l'app retient
    # l'empreinte du certificat, qui suffit à macOS pour reconnaître l'app d'une version à l'autre.
    return signing_identity(run("security", "find-identity", "-p", "codesigning", KEYCHAIN).stdout)


def create_identity():
    """Crée un certificat autosigné de signature de code et l'importe dans le trousseau de session."""
    step(f"Création de l'identité de signature « {IDENTITY} » dans le trousseau de session")
    print("    Si le trousseau est verrouillé, macOS demandera votre mot de passe de session.")
    with tempfile.TemporaryDirectory() as tmp:
        key, cert, p12 = (Path(tmp) / name for name in ("key.pem", "cert.pem", "id.p12"))
        password = secrets.token_hex(16)
        # L'openssl du système (LibreSSL) écrit un .p12 que `security import` sait lire.
        run("/usr/bin/openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "3650",
            "-keyout", key, "-out", cert, "-subj", f"/CN={IDENTITY}",
            "-addext", "basicConstraints=critical,CA:FALSE",
            "-addext", "keyUsage=critical,digitalSignature",
            "-addext", "extendedKeyUsage=critical,codeSigning")
        run("/usr/bin/openssl", "pkcs12", "-export", "-inkey", key, "-in", cert, "-out", p12,
            "-name", IDENTITY, "-passout", f"pass:{password}")
        # -T : codesign peut se servir de la clé sans demander l'accès à chaque signature.
        imported = run("security", "import", p12, "-k", KEYCHAIN, "-P", password,
                       "-T", "/usr/bin/codesign", check=False)
    if imported.returncode:
        raise Failure(
            f"macOS a refusé d'importer l'identité « {IDENTITY} » ({imported.stderr.strip()}).\n"
            "Le trousseau de session est sans doute verrouillé. Déverrouillez-le (mot de passe de session) :\n"
            f"    security unlock-keychain {KEYCHAIN}\n"
            "puis relancez : scripts/installer-app")
    found = identity_hash()
    if not found:
        raise Failure(f"l'identité « {IDENTITY} » a été importée mais codesign ne la voit pas.")
    print(f"    Identité créée : {found}")
    return found


def cargo():
    return shutil.which("cargo") or str(Path.home() / ".cargo/bin/cargo")


def build_binary():
    step("Compilation en release (cargo build --release)")
    if subprocess.run([cargo(), "build", "--release"], cwd=ROOT).returncode:
        raise Failure("la compilation a échoué.")
    return ROOT / "target/release" / EXE


def sign(app, entitlements, identity):
    step(f"Signature avec « {IDENTITY} » ({identity})")
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
    step("Greffier est ouvert : demande de fermeture")
    run("osascript", "-e", f'tell application id "{BUNDLE_ID}" to quit', check=False)
    deadline = time.monotonic() + 15
    while running_pids():
        if time.monotonic() > deadline:
            raise Failure("Greffier ne s'est pas fermé (un enregistrement en cours ?). "
                          "Fermez-le puis relancez scripts/installer-app.")
        time.sleep(0.2)


def install(app):
    """Remplace /Applications/Greffier.app par `app`, sans laisser de moment sans app installée."""
    step(f"Installation dans {LIVE}")
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
    """Ouvre l'app comme le Finder et attend que Rune signale sa première fenêtre prête."""
    step("Lancement de l'app installée")
    with tempfile.TemporaryDirectory() as ready:
        run("open", "--env", f"RUNE_READY_DIR={ready}", LIVE)
        deadline = time.monotonic() + 20
        while time.monotonic() < deadline:
            for receipt in Path(ready).glob(f"{BUNDLE_ID}.*"):
                try:
                    data = json.loads(receipt.read_text())
                except (OSError, ValueError):
                    continue
                print(f"    Fenêtre prête en {data.get('ready_ms', '?')} ms (pid {data.get('pid')}).")
                return
            time.sleep(0.2)
    raise Failure("l'app installée n'a pas affiché sa fenêtre en 20 s. "
                  f"Essayez de l'ouvrir depuis le Finder ; journal : Console.app, processus « {EXE} ».")


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binaire", type=Path, help="empaqueter cet exécutable au lieu de compiler")
    parser.add_argument("--sans-lancer", action="store_true", help="ne pas ouvrir l'app installée")
    args = parser.parse_args(argv)
    try:
        identity = identity_hash() or create_identity()
        binary = args.binaire or build_binary()
        if not os.access(binary, os.X_OK):
            raise Failure(f"pas d'exécutable à {binary}")
        step("Construction du bundle")
        target = ROOT / "target"
        app, entitlements = target / f"{NAME}.app", target / f"{NAME}.entitlements"
        shutil.rmtree(app, ignore_errors=True)
        write_bundle(app, binary, (ROOT / "Cargo.toml").read_text())
        write_entitlements(entitlements)
        sign(app, entitlements, identity)
        install(app)
        if not args.sans_lancer:
            launch()
    except Failure as error:
        print(f"installer-app : {error}", file=sys.stderr)
        return 1
    print(f"{LIVE} installée ({identity}).")
    return 0


if __name__ == "__main__":
    sys.exit(main())
