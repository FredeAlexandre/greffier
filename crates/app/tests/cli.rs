//! The `greffier` binary hands a command to the command line (`greffier-cli`) instead of opening
//! the window, with its folders taken from `GREFFIER_FOLDER` and `GREFFIER_APP_FOLDER`.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

/// A temporary folder, deleted at the end.
struct Temp(PathBuf);

impl Temp {
    fn new() -> Temp {
        static N: AtomicU32 = AtomicU32::new(0);
        let d = std::env::temp_dir().join(format!("greffier-app-cli-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("temporary folder");
        Temp(d)
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn copy_to(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("copy");
    for e in std::fs::read_dir(from).expect("fixtures").flatten() {
        let target = to.join(e.file_name());
        if e.file_type().expect("type").is_dir() { copy_to(&e.path(), &target) } else { std::fs::copy(e.path(), target).map(|_| ()).expect("copy") }
    }
}

/// A copy of the core's sample meetings.
fn meetings() -> Temp {
    let t = Temp::new();
    copy_to(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../core/tests/fixtures/meetings-folder"), &t.0);
    t
}

/// Runs the binary with this app folder and, if given, this meetings folder.
fn greffier(app: &Temp, folder: Option<&Temp>, args: &[&str]) -> Output {
    let mut c = Command::new(env!("CARGO_BIN_EXE_greffier"));
    c.args(args).env("GREFFIER_APP_FOLDER", &app.0).env_remove("GREFFIER_FOLDER");
    if let Some(f) = folder {
        c.env("GREFFIER_FOLDER", &f.0);
    }
    c.output().expect("greffier runs")
}

fn text(b: &[u8]) -> String {
    String::from_utf8(b.to_vec()).expect("UTF-8")
}

#[test]
fn search_list_and_show_run_in_the_binary() {
    let (app, folder) = (Temp::new(), meetings());
    let o = greffier(&app, Some(&folder), &["list"]);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    assert!(text(&o.stdout).starts_with("2026-11-20-1000 · 0:00:12 · done, 1 to check · Revue de release\n"), "{}", text(&o.stdout));

    let o = greffier(&app, Some(&folder), &["show", "2026-11-20-1000", "--json"]);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).expect("JSON");
    assert_eq!(v["identifier"], "2026-11-20-1000");
    assert_eq!(v["title"], "Revue de release");

    let o = greffier(&app, Some(&folder), &["search", "recette", "--in", "transcription"]);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    assert_eq!(text(&o.stdout), "2026-11-20-1000 · Revue de release\n  [0:00:02] Khalid ?: Oui, si la recette passe vendredi.\n");
    assert!(std::fs::read_dir(&app.0).expect("app folder").next().is_none(), "nothing written in the app folder");
}

#[test]
fn the_meetings_folder_is_the_one_remembered_in_the_app_folder() {
    let (app, folder) = (Temp::new(), meetings());
    let o = greffier(&app, None, &["list"]);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(text(&o.stderr), "greffier: no meetings folder: open Greffier once to choose one, or set GREFFIER_FOLDER.\n");

    let settings = serde_json::json!({ "meetings_folder": folder.0 });
    std::fs::write(app.0.join("settings.json"), settings.to_string()).expect("settings");
    let o = greffier(&app, None, &["status", "2026-11-20-1000"]);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    assert!(text(&o.stdout).starts_with("2026-11-20-1000 · Revue de release\nStatus: done"), "{}", text(&o.stdout));
}

#[test]
fn help_and_usage_errors_come_from_the_command_line() {
    let app = Temp::new();
    for help in ["help", "--help", "-h"] {
        let o = greffier(&app, None, &[help]);
        assert_eq!(o.status.code(), Some(0));
        assert!(text(&o.stdout).contains("greffier show <meeting>"), "{help}");
    }
    let o = greffier(&app, None, &["list", "--frobnicate"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(text(&o.stderr).starts_with("greffier: unknown option: --frobnicate (see greffier help)"));
    // A mistyped command is not taken for the window's arguments.
    let o = greffier(&app, None, &["lsit"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(text(&o.stderr).starts_with("greffier: unknown command: lsit (see greffier help)"), "{}", text(&o.stderr));
    assert!(o.stdout.is_empty());
}
