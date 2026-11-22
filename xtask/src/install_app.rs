//! `cargo xtask install-app`: builds Greffier.app, signs it as "Greffier Dev" and installs it in
//! /Applications.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use greffier_core::resources::hex;
use greffier_tools::mac::{UV_ARCHIVE, UV_ARCHIVE_SHA256, UV_VERSION};

use crate::{Failure, Result, TempDir, check_sha256, files_under, root, run, run_unchecked, step};

const NAME: &str = "Greffier";
const EXE: &str = "greffier";
/// Never changes: macOS ties permissions to this identifier and to the signing identity.
const BUNDLE_ID: &str = "io.github.fredealexandre.greffier";
const IDENTITY: &str = "Greffier Dev";
const KEYCHAIN: &str = "Library/Keychains/login.keychain-db";
const LSREGISTER: &str = "/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister";

pub fn main(args: &[String]) -> Result<()> {
    let mut binary = None;
    let mut no_launch = false;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--binary" => binary = Some(PathBuf::from(args.next().ok_or(Failure("--binary needs a path.".into()))?)),
            "--no-launch" => no_launch = true,
            other => return Err(Failure(format!("unknown option \"{other}\" (--binary PATH, --no-launch)."))),
        }
    }

    let keychain = keychain();
    let identity = match identity_hash(&keychain)? {
        Some(identity) => identity,
        None => create_identity(&keychain)?,
    };
    let binary = match binary {
        Some(binary) => binary,
        None => build_binary()?,
    };
    if std::fs::metadata(&binary).map(|m| m.permissions().mode() & 0o111 == 0).unwrap_or(true) {
        return Err(Failure(format!("no executable at {}", binary.display())));
    }
    let target = root().join("target");
    let uv = fetch_uv(&target)?;
    step("Building the bundle");
    let app = target.join(format!("{NAME}.app"));
    let entitlements = target.join(format!("{NAME}.entitlements"));
    let _ = std::fs::remove_dir_all(&app);
    write_bundle(&app, &binary, &project_version()?, Some(&uv))?;
    write_entitlements(&entitlements)?;
    sign(&app, &entitlements, &identity)?;
    install(&app)?;
    if !no_launch {
        launch()?;
    }
    println!("{} installed ({identity}).", live().display());
    Ok(())
}

// --- The bundle --------------------------------------------------------------------------------

enum Value {
    String(&'static str),
    Text(String),
    True,
}

/// An XML property list made of one dictionary.
fn plist(entries: &[(&str, Value)]) -> String {
    let escape = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    let mut xml = String::from(concat!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
        "<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n",
        "<plist version=\"1.0\">\n<dict>\n",
    ));
    for (key, value) in entries {
        xml += &format!("\t<key>{}</key>\n", escape(key));
        xml += &match value {
            Value::String(s) => format!("\t<string>{}</string>\n", escape(s)),
            Value::Text(s) => format!("\t<string>{}</string>\n", escape(s)),
            Value::True => "\t<true/>\n".to_string(),
        };
    }
    xml + "</dict>\n</plist>\n"
}

fn info_plist(version: &str) -> String {
    use Value::*;
    plist(&[
        ("CFBundleName", String(NAME)),
        ("CFBundleDisplayName", String(NAME)),
        ("CFBundleIdentifier", String(BUNDLE_ID)),
        ("CFBundleExecutable", String(EXE)),
        ("CFBundlePackageType", String("APPL")),
        ("CFBundleInfoDictionaryVersion", String("6.0")),
        ("CFBundleShortVersionString", Text(version.into())),
        ("CFBundleVersion", Text(version.into())),
        ("LSMinimumSystemVersion", String("13.0")),
        ("NSHighResolutionCapable", True),
        ("NSScreenCaptureUsageDescription", String("Greffier enregistre l'écran pendant une réunion pour en garder la vidéo.")),
        ("NSMicrophoneUsageDescription", String("Greffier enregistre votre micro pendant une réunion pour la transcrire.")),
        ("NSAudioCaptureUsageDescription", String("Greffier enregistre le son du Mac pendant une réunion pour entendre les autres participants.")),
    ])
}

/// Screen and system audio capture have no entitlement: only the macOS permission counts. The
/// microphone, though, is silently denied by the hardened runtime without this one. No sandbox:
/// Greffier runs whisper, uv and the `claude` CLI, and writes into the meetings folder.
fn write_entitlements(path: &Path) -> Result<()> {
    Ok(std::fs::write(path, plist(&[("com.apple.security.device.audio-input", Value::True)]))?)
}

/// Writes the unsigned bundle `app` around the executable `binary` (and the bundled uv).
fn write_bundle(app: &Path, binary: &Path, version: &str, uv: Option<&Path>) -> Result<()> {
    let macos = app.join("Contents/MacOS");
    std::fs::create_dir_all(&macos)?;
    std::fs::create_dir(app.join("Contents/Resources"))?;
    std::fs::copy(binary, macos.join(EXE))?;
    if let Some(uv) = uv {
        std::fs::copy(uv, macos.join("uv"))?;
    }
    Ok(std::fs::write(app.join("Contents/Info.plist"), info_plist(version))?)
}

/// The version of the `greffier` package, as `cargo metadata` gives it.
fn project_version() -> Result<String> {
    let manifest = root().join("Cargo.toml");
    let output = run(cargo(), &["metadata".as_ref(), "--no-deps".as_ref(), "--format-version=1".as_ref(), "--manifest-path".as_ref(), manifest.as_os_str()])?;
    version_in_metadata(&String::from_utf8_lossy(&output.stdout)).ok_or(Failure("cargo metadata does not list the greffier package.".into()))
}

fn version_in_metadata(metadata: &str) -> Option<String> {
    let metadata: serde_json::Value = serde_json::from_str(metadata).ok()?;
    let package = metadata["packages"].as_array()?.iter().find(|p| p["name"] == EXE)?;
    Some(package["version"].as_str()?.to_string())
}

// --- The bundled uv ----------------------------------------------------------------------------

/// Checks the sha256 of the uv archive and extracts the `uv` executable from it into `dest`.
fn unpack_uv(archive: &Path, sha256: &str, dest: &Path) -> Result<PathBuf> {
    check_sha256(archive, sha256)?;
    let tmp = TempDir::new("greffier-uv-")?;
    run("tar", &["-xzf".as_ref(), archive.as_os_str(), "-C".as_ref(), tmp.path().as_os_str()])?;
    let name = archive.file_name().unwrap_or_default().to_string_lossy();
    let uv = files_under(tmp.path())?.into_iter().find(|p| p.file_name().is_some_and(|n| n == "uv")).ok_or(Failure(format!("no uv executable in {name}.")))?;
    std::fs::copy(tmp.path().join(uv), dest)?;
    std::fs::set_permissions(dest, std::fs::Permissions::from_mode(0o755))?;
    Ok(dest.to_path_buf())
}

/// The bundled uv, downloaded once into target/uv-<version>/ (where `cargo run` finds it too).
fn fetch_uv(target: &Path) -> Result<PathBuf> {
    let folder = target.join(format!("uv-{UV_VERSION}"));
    let uv = folder.join("uv");
    if uv.is_file() {
        return Ok(uv);
    }
    step(&format!("Downloading uv {UV_VERSION}"));
    std::fs::create_dir_all(&folder)?;
    let archive = folder.join(UV_ARCHIVE);
    let url = format!("https://github.com/astral-sh/uv/releases/download/{UV_VERSION}/{UV_ARCHIVE}");
    let unpacked = run("curl", &["-fsSL".as_ref(), "-o".as_ref(), archive.as_os_str(), url.as_ref()]).and_then(|_| unpack_uv(&archive, UV_ARCHIVE_SHA256, &uv));
    let _ = std::fs::remove_file(&archive);
    unpacked
}

// --- Signing -----------------------------------------------------------------------------------

fn keychain() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(KEYCHAIN)
}

/// The SHA-1 hash of "Greffier Dev" in a `security find-identity` output, or None.
fn signing_identity(listing: &str) -> Result<Option<String>> {
    let quoted = format!("\"{IDENTITY}\"");
    let mut hashes = std::collections::BTreeSet::new();
    for line in listing.lines() {
        let Some((number, rest)) = line.trim_start().split_once(") ") else { continue };
        let Some((hash, name)) = rest.split_once(' ') else { continue };
        let numbered = !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit());
        let sha1 = hash.len() == 40 && hash.bytes().all(|b| b.is_ascii_digit() || (b'A'..=b'F').contains(&b));
        if numbered && sha1 && (name == quoted || name.starts_with(&format!("{quoted} "))) {
            hashes.insert(hash.to_string());
        }
    }
    if hashes.len() > 1 {
        let list = hashes.into_iter().collect::<Vec<_>>().join(", ");
        return Err(Failure(format!(
            "several \"{IDENTITY}\" identities in the keychain ({list}): keep the one that signed the installed app, delete the others in Keychain Access."
        )));
    }
    Ok(hashes.pop_first())
}

/// No -v: the identity does not need to be trusted to sign; the app's designated requirement
/// keeps the certificate hash, which is enough for macOS to recognize the app across versions.
fn identity_hash(keychain: &Path) -> Result<Option<String>> {
    let output = run("security", &["find-identity".as_ref(), "-p".as_ref(), "codesigning".as_ref(), keychain.as_os_str()])?;
    signing_identity(&String::from_utf8_lossy(&output.stdout))
}

/// Creates a self-signed code signing certificate and imports it into the login keychain.
fn create_identity(keychain: &Path) -> Result<String> {
    step(&format!("Creating the signing identity \"{IDENTITY}\" in the login keychain"));
    println!("    If the keychain is locked, macOS will ask for your login password.");
    let tmp = TempDir::new("greffier-identity-")?;
    let (key, cert, p12) = (tmp.path().join("key.pem"), tmp.path().join("cert.pem"), tmp.path().join("id.p12"));
    let mut random = [0u8; 16];
    std::io::Read::read_exact(&mut std::fs::File::open("/dev/urandom")?, &mut random)?;
    let password = hex(&random);
    let s = |p: &Path| p.to_string_lossy().into_owned();
    // The system openssl (LibreSSL) writes a .p12 that `security import` can read.
    run("/usr/bin/openssl", &[
        "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "3650", "-keyout", &s(&key), "-out", &s(&cert),
        "-subj", &format!("/CN={IDENTITY}"),
        "-addext", "basicConstraints=critical,CA:FALSE",
        "-addext", "keyUsage=critical,digitalSignature",
        "-addext", "extendedKeyUsage=critical,codeSigning",
    ])?;
    run("/usr/bin/openssl", &["pkcs12", "-export", "-inkey", &s(&key), "-in", &s(&cert), "-out", &s(&p12), "-name", IDENTITY, "-passout", &format!("pass:{password}")])?;
    // -T: codesign can use the key without asking for access at every signature.
    let imported = run_unchecked("security", &["import", &s(&p12), "-k", &s(keychain), "-P", &password, "-T", "/usr/bin/codesign"])?;
    if !imported.status.success() {
        return Err(Failure(format!(
            "macOS refused to import the identity \"{IDENTITY}\" ({}).\nThe login keychain is probably locked. Unlock it (login password):\n    security unlock-keychain {}\nthen run again: cargo xtask install-app",
            String::from_utf8_lossy(&imported.stderr).trim(),
            keychain.display()
        )));
    }
    let found = identity_hash(keychain)?.ok_or(Failure(format!("the identity \"{IDENTITY}\" was imported but codesign does not see it.")))?;
    println!("    Identity created: {found}");
    Ok(found)
}

fn sign(app: &Path, entitlements: &Path, identity: &str) -> Result<()> {
    step(&format!("Signing with \"{IDENTITY}\" ({identity})"));
    let codesign = |extra: &[&std::ffi::OsStr], path: &Path| {
        let mut args: Vec<&std::ffi::OsStr> = vec!["--force".as_ref(), "--options".as_ref(), "runtime".as_ref(), "--timestamp=none".as_ref()];
        args.extend_from_slice(extra);
        args.extend(["--sign".as_ref(), identity.as_ref(), path.as_os_str()]);
        run("codesign", &args)
    };
    // Nested code first: codesign refuses to sign an app with an unsigned executable inside.
    let helper = app.join("Contents/MacOS/uv");
    if helper.exists() {
        codesign(&[], &helper)?;
    }
    codesign(&["--entitlements".as_ref(), entitlements.as_os_str()], app)?;
    run("codesign", &["--verify".as_ref(), "--strict".as_ref(), app.as_os_str()])?;
    let requirement = run("codesign", &["-d".as_ref(), "-r-".as_ref(), app.as_os_str()])?;
    let requirement = String::from_utf8_lossy(&requirement.stdout);
    println!("    {}", requirement.trim().lines().last().unwrap_or_default());
    Ok(())
}

// --- Build, install, launch --------------------------------------------------------------------

fn cargo() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".into())
}

fn build_binary() -> Result<PathBuf> {
    step("Building in release (cargo build --release -p greffier)");
    let status = Command::new(cargo()).args(["build", "--release", "-p", "greffier"]).current_dir(root()).status()?;
    if !status.success() {
        return Err(Failure("the build failed.".into()));
    }
    Ok(root().join("target/release").join(EXE))
}

fn live() -> PathBuf {
    Path::new("/Applications").join(format!("{NAME}.app"))
}

fn running_pids() -> Result<Vec<u32>> {
    let pattern = format!("^{}( |$)", live().join("Contents/MacOS").join(EXE).display());
    let found = run_unchecked("pgrep", &["-f", &pattern])?;
    Ok(String::from_utf8_lossy(&found.stdout).split_whitespace().filter_map(|pid| pid.parse().ok()).collect())
}

fn quit_running_app() -> Result<()> {
    if running_pids()?.is_empty() {
        return Ok(());
    }
    step("Greffier is open: asking it to quit");
    run_unchecked("osascript", &["-e", &format!("tell application id \"{BUNDLE_ID}\" to quit")])?;
    let deadline = Instant::now() + Duration::from_secs(15);
    while !running_pids()?.is_empty() {
        if Instant::now() > deadline {
            return Err(Failure("Greffier did not quit (a recording in progress?). Quit it, then run cargo xtask install-app again.".into()));
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    Ok(())
}

/// Replaces /Applications/Greffier.app with `app`, never leaving a moment with no app installed.
fn install(app: &Path) -> Result<()> {
    let live = live();
    step(&format!("Installing into {}", live.display()));
    quit_running_app()?;
    let stage = TempDir::new_in(live.parent().unwrap(), &format!(".{NAME}.app.install-"))?;
    let (new, old) = (stage.path().join(format!("{NAME}.app")), stage.path().join(format!("{NAME}.old.app")));
    run("ditto", &[app.as_os_str(), new.as_os_str()])?;
    if live.exists() {
        std::fs::rename(&live, &old)?;
    }
    if let Err(error) = std::fs::rename(&new, &live) {
        if old.exists() {
            std::fs::rename(&old, &live)?;
        }
        return Err(error.into());
    }
    drop(stage);
    run_unchecked(LSREGISTER, &["-f".as_ref(), live.as_os_str()])?;
    Ok(())
}

/// Opens the app like the Finder does and waits for Rune to report its first window ready.
fn launch() -> Result<()> {
    step("Launching the installed app");
    let ready = TempDir::new("greffier-ready-")?;
    run("open", &["--env".as_ref(), format!("RUNE_READY_DIR={}", ready.path().display()).as_ref(), live().as_os_str()])?;
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        for entry in std::fs::read_dir(ready.path())?.flatten() {
            if !entry.file_name().to_string_lossy().starts_with(&format!("{BUNDLE_ID}.")) {
                continue;
            }
            let Ok(data) = std::fs::read_to_string(entry.path()) else { continue };
            let Ok(data) = serde_json::from_str::<serde_json::Value>(&data) else { continue };
            println!("    Window ready in {} ms (pid {}).", data["ready_ms"], data["pid"]);
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    Err(Failure(format!(
        "the installed app did not show its window within 20 s. Try opening it from the Finder; log: Console.app, process \"{EXE}\"."
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A property list read back by macOS's own parser.
    fn read_plist(path: &Path) -> serde_json::Value {
        let output = run("plutil", &["-convert".as_ref(), "json".as_ref(), "-o".as_ref(), "-".as_ref(), path.as_os_str()]).unwrap();
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn executable(path: &Path, content: &[u8]) {
        std::fs::write(path, content).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn is_executable(path: &Path) -> bool {
        std::fs::metadata(path).unwrap().permissions().mode() & 0o111 != 0
    }

    /// A bundle around a fake executable, without uv.
    fn bundle() -> (TempDir, PathBuf) {
        let tmp = TempDir::new("xtask-test-").unwrap();
        let binary = tmp.path().join("greffier");
        executable(&binary, b"#!/bin/sh\n");
        let app = tmp.path().join("Greffier.app");
        write_bundle(&app, &binary, "1.2.3", None).unwrap();
        (tmp, app)
    }

    #[test]
    fn the_executable_is_in_the_bundle() {
        let (_tmp, app) = bundle();
        let exe = app.join("Contents/MacOS/greffier");
        assert_eq!(std::fs::read(&exe).unwrap(), b"#!/bin/sh\n");
        assert!(is_executable(&exe));
        assert_eq!(read_plist(&app.join("Contents/Info.plist"))["CFBundleExecutable"], "greffier");
    }

    #[test]
    fn stable_identity_and_project_version() {
        let (_tmp, app) = bundle();
        let info = read_plist(&app.join("Contents/Info.plist"));
        assert_eq!(info["CFBundleIdentifier"], "io.github.fredealexandre.greffier");
        assert_eq!(info["CFBundleName"], "Greffier");
        assert_eq!(info["CFBundlePackageType"], "APPL");
        assert_eq!(info["CFBundleShortVersionString"], "1.2.3");
        assert_eq!(info["CFBundleVersion"], "1.2.3");
        assert_eq!(info["NSHighResolutionCapable"], true);
    }

    #[test]
    fn explains_use_of_screen_microphone_and_system_audio() {
        let (_tmp, app) = bundle();
        let info = read_plist(&app.join("Contents/Info.plist"));
        for key in ["NSScreenCaptureUsageDescription", "NSMicrophoneUsageDescription", "NSAudioCaptureUsageDescription"] {
            assert!(info[key].as_str().unwrap_or_default().contains("Greffier"), "{key}");
        }
    }

    #[test]
    fn the_version_is_the_greffier_package_one() {
        let metadata = r#"{"packages": [{"name": "greffier-core", "version": "9.9.9"}, {"name": "greffier", "version": "1.2.3"}]}"#;
        assert_eq!(version_in_metadata(metadata).as_deref(), Some("1.2.3"));
        assert_eq!(version_in_metadata(r#"{"packages": []}"#), None);
    }

    #[test]
    fn the_project_version_comes_from_cargo() {
        let version = project_version().unwrap();
        assert!(!version.is_empty() && version.split('.').all(|n| n.parse::<u32>().is_ok()), "{version}");
    }

    fn uv_archive(tmp: &TempDir) -> (PathBuf, String) {
        let folder = tmp.path().join("uv-aarch64-apple-darwin");
        std::fs::create_dir(&folder).unwrap();
        executable(&folder.join("uv"), b"#!/bin/sh\necho uv\n");
        let archive = tmp.path().join("uv.tar.gz");
        run("tar", &["-czf".as_ref(), archive.as_os_str(), "-C".as_ref(), tmp.path().as_os_str(), "uv-aarch64-apple-darwin".as_ref()]).unwrap();
        let sha = greffier_core::resources::sha256(&std::fs::read(&archive).unwrap());
        (archive, sha)
    }

    #[test]
    fn extracts_uv_from_an_archive_with_the_right_sha256() {
        let tmp = TempDir::new("xtask-test-").unwrap();
        let (archive, sha) = uv_archive(&tmp);
        let uv = unpack_uv(&archive, &sha, &tmp.path().join("extracted")).unwrap();
        assert_eq!(std::fs::read(&uv).unwrap(), b"#!/bin/sh\necho uv\n");
        assert!(is_executable(&uv));
    }

    #[test]
    fn rejects_an_archive_with_the_wrong_sha256() {
        let tmp = TempDir::new("xtask-test-").unwrap();
        let (archive, _) = uv_archive(&tmp);
        assert!(unpack_uv(&archive, &"0".repeat(64), &tmp.path().join("extracted")).is_err());
        assert!(!tmp.path().join("extracted").exists());
    }

    #[test]
    fn uv_is_next_to_the_executable_in_the_bundle() {
        let tmp = TempDir::new("xtask-test-").unwrap();
        let (archive, sha) = uv_archive(&tmp);
        let uv = unpack_uv(&archive, &sha, &tmp.path().join("uv-extracted")).unwrap();
        let binary = tmp.path().join("greffier");
        executable(&binary, b"#!/bin/sh\n");
        let app = tmp.path().join("Greffier.app");
        write_bundle(&app, &binary, "1.2.3", Some(&uv)).unwrap();
        assert!(is_executable(&app.join("Contents/MacOS/uv")));
    }

    #[test]
    fn microphone_passes_the_hardened_runtime_without_sandbox() {
        let tmp = TempDir::new("xtask-test-").unwrap();
        let path = tmp.path().join("Greffier.entitlements");
        write_entitlements(&path).unwrap();
        let entitlements = read_plist(&path);
        assert_eq!(entitlements["com.apple.security.device.audio-input"], true);
        assert!(entitlements.get("com.apple.security.app-sandbox").is_none());
    }

    /// Real output of `security find-identity -p codesigning`: an untrusted self-signed identity
    /// is listed with a suffix, and each identity appears twice if it is valid.
    const FIND_IDENTITY: &str = r#"
Policy: Code Signing
  Matching identities
  1) B4DE18D418E97A6F781FA2A3529233162141CEA3 "Beamer Dev"
  2) FC2C7014A4F8CED15F2419D8A9E4A530BB914DBE "Greffier Dev" (CSSMERR_TP_NOT_TRUSTED)
  3) 0123456789ABCDEF0123456789ABCDEF01234567 "Greffier Dev Old"
     3 identities found

  Valid identities only
  1) B4DE18D418E97A6F781FA2A3529233162141CEA3 "Beamer Dev"
     1 valid identities found
"#;

    #[test]
    fn finds_greffier_dev_even_untrusted() {
        assert_eq!(signing_identity(FIND_IDENTITY).unwrap().as_deref(), Some("FC2C7014A4F8CED15F2419D8A9E4A530BB914DBE"));
    }

    #[test]
    fn missing_identity() {
        let listing = FIND_IDENTITY.replace("\"Greffier Dev\" (CSSMERR_TP_NOT_TRUSTED)", "\"Other\"");
        assert_eq!(signing_identity(&listing).unwrap(), None);
    }

    #[test]
    fn several_identities_are_refused() {
        let listing = FIND_IDENTITY.replace("\"Beamer Dev\"", "\"Greffier Dev\"");
        assert!(signing_identity(&listing).is_err());
    }
}
