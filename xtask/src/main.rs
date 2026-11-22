//! Greffier's development tasks, in Rust like the rest of the repository:
//!
//!     cargo xtask install-app [--binary PATH] [--no-launch]
//!     cargo xtask vendor-whisper
//!     cargo xtask publish-pyannote <pyannote-community-1 folder>
//!
//! (`cargo xtask` is an alias defined in `.cargo/config.toml`.) The pinned versions and sha256s are
//! read from the crates that use them, never copied here.

mod install_app;
mod publish_pyannote;
mod vendor_whisper;

use std::ffi::OsStr;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Output};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const USAGE: &str = "usage:
    cargo xtask install-app [--binary PATH] [--no-launch]   build, sign and install Greffier.app
    cargo xtask vendor-whisper                              copy the pinned whisper.cpp sources into engines/whisper.cpp/
    cargo xtask publish-pyannote <folder>                   publish the pyannote-community-1 release";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some((task, rest)) = args.split_first() else {
        eprintln!("{USAGE}");
        return ExitCode::FAILURE;
    };
    let result = match task.as_str() {
        "install-app" => install_app::main(rest),
        "vendor-whisper" => vendor_whisper::main(rest),
        "publish-pyannote" => publish_pyannote::main(rest),
        "-h" | "--help" | "help" => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        other => Err(Failure(format!("unknown task \"{other}\".\n{USAGE}"))),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{task}: {error}");
            ExitCode::FAILURE
        }
    }
}

/// A step failed; the message says what to do.
#[derive(Debug)]
pub struct Failure(pub String);

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<std::io::Error> for Failure {
    fn from(error: std::io::Error) -> Failure {
        Failure(error.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Failure>;

/// The repository's root.
pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf()
}

pub fn step(text: &str) {
    println!("==> {text}");
}

/// Runs a program and returns its output, whatever its exit status.
pub fn run_unchecked<S: AsRef<OsStr>>(program: impl AsRef<OsStr>, args: &[S]) -> Result<Output> {
    let program = program.as_ref();
    Command::new(program)
        .args(args)
        .output()
        .map_err(|e| Failure(format!("{} could not be started: {e}", program.to_string_lossy())))
}

/// Runs a program; fails with its error output if it fails.
pub fn run<S: AsRef<OsStr>>(program: impl AsRef<OsStr>, args: &[S]) -> Result<Output> {
    let output = run_unchecked(&program, args)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let message = if stderr.trim().is_empty() { stdout } else { stderr };
        return Err(Failure(format!("{} failed:\n{}", program.as_ref().to_string_lossy(), message.trim())));
    }
    Ok(output)
}

/// Downloads `url` into `dest` and checks its sha256 (the file is removed if it does not match).
pub fn download(url: &str, dest: &Path, sha256: &str) -> Result<()> {
    run("curl", &[OsStr::new("-fsSL"), OsStr::new("-o"), dest.as_os_str(), OsStr::new(url)])?;
    check_sha256(dest, sha256).inspect_err(|_| {
        let _ = std::fs::remove_file(dest);
    })
}

pub fn check_sha256(path: &Path, expected: &str) -> Result<()> {
    let found = greffier_core::resources::sha256(&std::fs::read(path)?);
    if found != expected {
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        return Err(Failure(format!("unexpected sha256 for {name}: {found} (expected {expected}).")));
    }
    Ok(())
}

/// The files under `dir`, recursively, relative to it and sorted.
pub fn files_under(dir: &Path) -> Result<Vec<PathBuf>> {
    fn walk(base: &Path, dir: &Path, found: &mut Vec<PathBuf>) -> std::io::Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if entry.file_type()?.is_dir() {
                walk(base, &path, found)?;
            } else {
                found.push(path.strip_prefix(base).unwrap().to_path_buf());
            }
        }
        Ok(())
    }
    let mut found = Vec::new();
    walk(dir, dir, &mut found)?;
    found.sort();
    Ok(found)
}

/// A directory removed with its content when dropped.
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new(prefix: &str) -> Result<TempDir> {
        TempDir::new_in(&std::env::temp_dir(), prefix)
    }

    pub fn new_in(parent: &Path, prefix: &str) -> Result<TempDir> {
        static COUNT: AtomicU32 = AtomicU32::new(0);
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().subsec_nanos();
        let n = COUNT.fetch_add(1, Ordering::Relaxed);
        let path = parent.join(format!("{prefix}{}-{nanos}-{n}", std::process::id()));
        std::fs::create_dir(&path)?;
        Ok(TempDir(path))
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
