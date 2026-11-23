//! End to end: the command line launches a Greffier process when nobody answers, here the
//! test host (`greffier-test-server`, a core on the demo's fake adapters), on a temporary copy
//! of the sample meetings and a temporary app folder. Every process launched is killed at the
//! end of its test.

mod common;

use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use common::{Temp, fixtures, media, run_in};
use greffier_cli::Context;
use greffier_cli::client::{Launch, Process};
use greffier_cli::server;

const RELEASE: &str = "2026-11-20 10h00 - Revue de release";

/// A meetings folder and an app folder; the process launched there, if any, is killed at the
/// end (its pid is in the lock file).
struct Setup {
    folder: Temp,
    app: Temp,
}

impl Drop for Setup {
    fn drop(&mut self) {
        // `locked` may have taken the lock for this process: it writes its own pid there.
        if let Ok(pid) = std::fs::read_to_string(server::lock_path(&self.app.0))
            && pid.trim() != std::process::id().to_string()
        {
            let _ = Command::new("/bin/kill").arg(pid.trim()).status();
        }
    }
}

fn setup() -> Setup {
    Setup { folder: fixtures(), app: Temp::new() }
}

impl Setup {
    /// The command line, launching the test host with this speed and idle time.
    fn cx(&self, speed: f64, idle: f64) -> Context {
        let env = [
            ("GREFFIER_FOLDER", self.folder.0.as_os_str().to_owned()),
            ("GREFFIER_APP_FOLDER", self.app.0.as_os_str().to_owned()),
            ("GREFFIER_TEST_SPEED", speed.to_string().into()),
            ("GREFFIER_TEST_IDLE", idle.to_string().into()),
        ];
        let program = PathBuf::from(env!("CARGO_BIN_EXE_greffier-test-server"));
        let Launch::Program { program, .. } = Launch::rule(Some(program), false, None) else { panic!("a program") };
        let mut process = Process::new(&self.app.0, Launch::Program { program, env: env.into_iter().map(|(k, v)| (k.to_string(), v)).collect() });
        process.poll = Duration::from_millis(100);
        Context { meetings_folder: Some(self.folder.0.clone()), app_folder: None, live: None, process: Some(process) }
    }

    fn socket(&self) -> PathBuf {
        server::socket_path(&self.app.0)
    }

    fn answering(&self) -> bool {
        UnixStream::connect(self.socket()).is_ok()
    }

    /// Whether a process holds the app folder's lock.
    fn locked(&self) -> bool {
        server::lock_instance(&self.app.0).is_err()
    }
}

fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let start = Instant::now();
    while !done() {
        assert!(start.elapsed() < Duration::from_secs(15), "still not {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn a_write_launches_greffier_in_the_background() {
    let s = setup();
    assert!(!s.answering());
    let o = run_in(&s.cx(10.0, 60.0), &["rename", "2026-11-20", "Revue", "lancée"]);
    assert_eq!(o.code, 0, "{}", o.err);
    assert_eq!(o.out, "2026-11-20-1000 · renommée : Revue lancée\n");
    assert!(s.folder.0.join("2026-11-20 10h00 - Revue lancée").is_dir(), "renamed on disk");
    assert!(!s.folder.0.join(RELEASE).exists());
    assert!(s.answering() && s.locked(), "it keeps running");
    // The next command finds it.
    let o = run_in(&s.cx(10.0, 60.0), &["rename", "2026-11-20", "Revue"]);
    assert_eq!(o.code, 0, "{}", o.err);
}

#[test]
fn a_launch_that_meets_a_process_still_quitting_is_tried_again() {
    let s = setup();
    // A process quitting still holds the app folder's lock: the first test host launched finds
    // it held and exits at once, before the socket answers.
    let quitting = server::lock_instance(&s.app.0).expect("lock");
    let quit = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(500));
        drop(quitting);
    });
    let o = run_in(&s.cx(10.0, 60.0), &["rename", "2026-11-20", "Revue"]);
    quit.join().unwrap();
    assert_eq!(o.code, 0, "{}", o.err);
    assert!(s.folder.0.join("2026-11-20 10h00 - Revue").is_dir());
    assert!(s.answering() && s.locked());
}

#[test]
fn a_process_that_keeps_exiting_is_given_up_after_a_few_launches() {
    let s = setup();
    let _held = server::lock_instance(&s.app.0).expect("lock");
    let start = Instant::now();
    let o = run_in(&s.cx(10.0, 60.0), &["rename", "2026-11-20", "Revue"]);
    assert_eq!(o.code, 3, "{}", o.err);
    assert!(o.err.starts_with(&format!("greffier : Greffier ne répond pas sur {}.", s.socket().display())), "{}", o.err);
    assert!(start.elapsed() < Duration::from_secs(10), "without waiting for the 15 s: {:?}", start.elapsed());
    assert!(s.folder.0.join(RELEASE).is_dir());
}

#[test]
fn import_waits_until_the_meeting_is_done() {
    let s = setup();
    let (_outside, file) = media();
    let o = run_in(&s.cx(20.0, 60.0), &["import", file.to_str().unwrap(), "--wait", "--timeout", "60"]);
    assert_eq!(o.code, 0, "{}", o.err);
    let lines: Vec<&str> = o.out.lines().collect();
    assert_eq!(lines.len(), 2, "{}", o.out);
    assert_eq!(lines[1], format!("{} · terminée · Point produit : maquettes bookmarks, staging", lines[0]));
    let shown = run_in(&Context { meetings_folder: Some(s.folder.0.clone()), app_folder: None, live: None, process: None }, &["show", lines[0], "--bilan"]);
    assert_eq!(shown.code, 0, "{}", shown.err);
    assert!(shown.out.starts_with("## En bref"), "{}", shown.out);
}

#[test]
fn status_shows_the_live_progress_while_processing() {
    let s = setup();
    let cx = s.cx(1.0, 60.0);
    let (_outside, file) = media();
    let o = run_in(&cx, &["import", file.to_str().unwrap()]);
    assert_eq!(o.code, 0, "{}", o.err);
    let id = o.out.trim().to_string();
    std::thread::sleep(Duration::from_millis(300));
    let o = run_in(&cx, &["status"]);
    assert_eq!(o.code, 0, "{}", o.err);
    let line = o.out.lines().find(|l| l.starts_with(&id)).unwrap_or_else(|| panic!("{id} in {}", o.out));
    assert!(line.starts_with(&format!("{id} · Transcription ")) && line.contains(" % · "), "{line}");
    let o = run_in(&cx, &["status", &id, "--json"]);
    let v: serde_json::Value = serde_json::from_str(&o.out).unwrap_or_else(|e| panic!("{e}: {} {}", o.out, o.err));
    assert_eq!(v["statut"], "en_cours");
    assert_eq!(v["rang"], 1);
}

#[test]
fn status_never_launches_greffier() {
    let s = setup();
    let o = run_in(&s.cx(10.0, 60.0), &["status"]);
    assert_eq!(o.code, 0, "{}", o.err);
    std::thread::sleep(Duration::from_millis(300));
    assert!(!s.answering() && !s.locked(), "nothing launched");
}

#[test]
fn the_background_process_quits_when_idle() {
    let s = setup();
    let o = run_in(&s.cx(10.0, 0.5), &["stop", "2026-11-20"]);
    assert_eq!(o.code, 1, "it answered: {}", o.err);
    assert!(s.locked());
    wait_until("quit", || !s.locked());
    assert!(!s.socket().exists(), "its socket is removed");
}

#[test]
fn a_second_process_on_the_same_app_folder_refuses_to_start() {
    let s = setup();
    let o = run_in(&s.cx(10.0, 60.0), &["rename", "2026-11-20", "Revue"]);
    assert_eq!(o.code, 0, "{}", o.err);
    let second = Command::new(env!("CARGO_BIN_EXE_greffier-test-server"))
        .arg("--background")
        .env("GREFFIER_FOLDER", &s.folder.0)
        .env("GREFFIER_APP_FOLDER", &s.app.0)
        .output()
        .expect("runs");
    assert_eq!(second.status.code(), Some(1));
    assert_eq!(String::from_utf8_lossy(&second.stderr), "greffier-test-server : Greffier tourne déjà pour ce dossier de l'app\n");
    assert!(s.answering(), "the first one still answers");
}

#[test]
fn the_test_host_never_runs_on_the_real_folders() {
    let o = Command::new(env!("CARGO_BIN_EXE_greffier-test-server")).env_remove("GREFFIER_FOLDER").env_remove("GREFFIER_APP_FOLDER").output().expect("runs");
    assert_eq!(o.status.code(), Some(1));
    let app = Temp::new();
    let o = Command::new(env!("CARGO_BIN_EXE_greffier-test-server")).env("GREFFIER_FOLDER", Path::new(&app.0)).env_remove("GREFFIER_APP_FOLDER").output().expect("runs");
    assert_eq!(String::from_utf8_lossy(&o.stderr), "greffier-test-server : GREFFIER_APP_FOLDER manque\n");
}
