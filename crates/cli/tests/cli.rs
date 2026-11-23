//! The command line's reading side, end to end on a temporary meetings folder: the core's
//! sample meetings (`crates/core/tests/fixtures/meetings-folder`) plus meetings written by the
//! core in other states (failed, in the queue, interrupted, two in the same minute).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use greffier_cli::{Context, Live, LiveSource};
use greffier_core::{DateTime, Meeting, MeetingsFolder, Stage, Status, folder::folder_name};
use serde_json::Value;

/// A temporary meetings folder, deleted at the end.
struct Folder(PathBuf);

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn temp_folder() -> Folder {
    static N: AtomicU32 = AtomicU32::new(0);
    let d = std::env::temp_dir().join(format!("greffier-cli-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("temporary folder");
    Folder(d)
}

fn copy_to(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("copy");
    for e in std::fs::read_dir(from).expect("fixtures").flatten() {
        let target = to.join(e.file_name());
        if e.file_type().expect("type").is_dir() { copy_to(&e.path(), &target) } else { std::fs::copy(e.path(), target).map(|_| ()).expect("copy") }
    }
}

/// Writes a meeting with only its sheet (and an empty transcription once a stage is done).
fn write(folder: &Path, start: DateTime, title: &str, duration: f64, status: Status, done: Option<Stage>) {
    let mut m = Meeting::new(0, folder_name(start, title), start, duration);
    m.title = title.into();
    m.status = status;
    m.completed_stage = done;
    MeetingsFolder::new(folder).write(&m).expect("written");
}

/// The sample meetings, plus:
/// - 2026-11-22 08h00 « Démo »: recording interrupted, 10 min;
/// - 2026-11-21 09h39 « Point d'équipe »: review failed, 45 min;
/// - 2026-11-21 09h39 « Rétro »: in the queue for the transcription, 30 min.
fn meetings() -> Folder {
    let f = temp_folder();
    copy_to(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../core/tests/fixtures/meetings-folder"), &f.0);
    write(&f.0, DateTime::new(2026, 11, 22, 8, 0), "Démo", 600.0, Status::RecordingInterrupted, None);
    write(&f.0, DateTime::new(2026, 11, 21, 9, 39), "Point d'équipe", 2700.0, Status::Failed { stage: Stage::Review, message: "La CLI claude a échoué.".into() }, Some(Stage::Transcription));
    write(&f.0, DateTime::new(2026, 11, 21, 9, 39), "Rétro", 1800.0, Status::Pending(Stage::Transcription), None);
    f
}

const RELEASE: &str = "2026-11-20 10h00 - Revue de release";

struct Output {
    code: i32,
    out: String,
    err: String,
}

impl Output {
    fn json(&self) -> Value {
        assert_eq!(self.code, 0, "{}", self.err);
        serde_json::from_str(&self.out).unwrap_or_else(|e| panic!("JSON: {e}\n{}", self.out))
    }
}

fn run_in(cx: &Context, args: &[&str]) -> Output {
    let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = greffier_cli::run_with(cx, &args, &mut out, &mut err);
    Output { code, out: String::from_utf8(out).expect("UTF-8"), err: String::from_utf8(err).expect("UTF-8") }
}

fn greffier(folder: &Folder, args: &[&str]) -> Output {
    run_in(&Context { meetings_folder: Some(folder.0.clone()), app_folder: None, live: None, process: None }, args)
}

// --- Dispatch, help, usage ---

#[test]
fn words_are_for_the_command_line_and_options_for_the_window() {
    let is = |args: &[&str]| greffier_cli::is_command(&args.iter().map(|a| a.to_string()).collect::<Vec<_>>());
    for args in [&["list"][..], &["show", "x"], &["status"], &["search", "x"], &["help"], &["--help"], &["-h"], &["lsit"], &["shwo", "--json"]] {
        assert!(is(args), "{args:?}");
    }
    for args in [&[][..], &["--snap", "out.png", "--scene", "meeting"], &["--list-scenes"], &["--scene", "list"], &["--background"]] {
        assert!(!is(args), "{args:?} opens the app");
    }
}

#[test]
fn help_lists_the_commands_that_work() {
    let f = meetings();
    for args in [&["help"][..], &["--help"], &["-h"], &["list", "--help"]] {
        let o = greffier(&f, args);
        assert_eq!(o.code, 0);
        assert!(o.err.is_empty());
        for command in ["greffier search", "greffier list", "greffier show", "greffier status", "--json"] {
            assert!(o.out.contains(command), "{command}");
        }
        assert!(!o.out.contains("coming"), "only what works");
    }
}

#[test]
fn usage_errors_exit_with_2() {
    let f = meetings();
    for (args, message) in [
        (&["list", "--frobnicate"][..], "unknown option: --frobnicate"),
        (&["list", "extra"], "extra argument: \"extra\""),
        (&["list", "--limit"], "--limit needs a value"),
        (&["list", "--limit", "zero"], "--limit needs a number"),
        (&["list", "--date", "20/11/2026"], "invalid date: \"20/11/2026\" (YYYY-MM-DD or YYYY-MM)"),
        (&["show"], "missing the meeting"),
        (&["show", "2026-11-20", "--summary", "--sheet"], "one view at a time: --summary or --sheet"),
        (&["show", "2026-11-20", "--bilan"], "unknown option: --bilan"),
        (&["show", "2026-11-20", "--from", "0:10"], "--from and --to go with --transcription or --passages"),
        (&["show", "2026-11-20", "--speakers", "--numbers"], "--numbers goes with --transcription"),
        (&["show", "2026-11-20", "--transcription", "--from", "ten"], "invalid instant: \"ten\""),
        (&["status", "a", "b"], "extra argument: \"b\""),
    ] {
        let o = greffier(&f, args);
        assert_eq!(o.code, 2, "{args:?}");
        assert!(o.out.is_empty(), "{args:?}");
        assert!(o.err.starts_with("greffier: ") && o.err.contains(message) && o.err.ends_with(" (see greffier help)\n"), "{args:?}: {}", o.err);
    }
    let o = greffier(&f, &["lsit"]);
    assert_eq!((o.code, o.err.as_str()), (2, "greffier: unknown command: lsit (see greffier help)\n"));
}

#[test]
fn without_a_meetings_folder_it_says_how_to_choose_one() {
    let o = run_in(&Context { meetings_folder: None, app_folder: None, live: None, process: None }, &["list"]);
    assert_eq!(o.code, 1);
    assert_eq!(o.err, "greffier: no meetings folder: open Greffier once to choose one, or set GREFFIER_FOLDER.\n");
    let o = run_in(&Context { meetings_folder: Some(PathBuf::from("/nulle/part")), app_folder: None, live: None, process: None }, &["list"]);
    assert_eq!(o.code, 1);
    assert_eq!(o.err, "greffier: meetings folder not found: /nulle/part\n");
}

// --- list ---

#[test]
fn list_gives_one_line_per_meeting_most_recent_first() {
    let f = meetings();
    let o = greffier(&f, &["list"]);
    assert_eq!(o.code, 0, "{}", o.err);
    assert!(o.err.is_empty());
    assert_eq!(
        o.out,
        "2026-11-22-0800 · 0:10:00 · recording interrupted · Démo\n\
         2026-11-21-0939-2 · 0:30:00 · queued · Rétro\n\
         2026-11-21-0939 · 0:45:00 · failed: Review · Point d'équipe\n\
         2026-11-20-1000 · 0:00:12 · done, 1 to check · Revue de release\n\
         2026-11-19-1530 · 0:00:12 · to process · Réunion\n\
         2026-11-18-0915 · 0:00:12 · to process · Réunion\n"
    );
}

#[test]
fn list_filters_by_date_and_limits() {
    let f = meetings();
    let ids = |args: &[&str]| -> Vec<String> {
        let o = greffier(&f, args);
        assert_eq!(o.code, 0, "{}", o.err);
        o.out.lines().map(|l| l.split(" · ").next().unwrap().to_string()).collect()
    };
    assert_eq!(ids(&["list", "--date", "2026-11-21"]), ["2026-11-21-0939-2", "2026-11-21-0939"]);
    assert_eq!(ids(&["list", "--date", "2026-11"]).len(), 6, "a whole month");
    assert_eq!(ids(&["list", "--since", "2026-11-20", "--until", "2026-11-21"]), ["2026-11-21-0939-2", "2026-11-21-0939", "2026-11-20-1000"]);
    assert_eq!(ids(&["list", "--limit=2"]).len(), 2);
    assert_eq!(ids(&["list", "--since", "2026-11-22", "--limit", "5"]), ["2026-11-22-0800"]);
    assert_eq!(greffier(&f, &["list", "--date", "2025-01-01"]).out, "No meetings.\n");
}

#[test]
fn list_in_json_uses_the_glossary_s_english_terms() {
    let f = meetings();
    let v = greffier(&f, &["list", "--json"]).json();
    let all = v.as_array().expect("an array");
    assert_eq!(all.len(), 6);
    let failed = &all[2];
    let keys: Vec<&str> = failed.as_object().unwrap().keys().map(String::as_str).collect();
    let mut expected = ["identifier", "folder", "title", "date", "start", "duration", "duration_text", "status", "stage", "completed_stage", "error", "to_check"];
    expected.sort_unstable();
    assert_eq!(keys, expected);
    assert_eq!(failed["identifier"], "2026-11-21-0939");
    assert_eq!(failed["title"], "Point d'équipe");
    assert_eq!(failed["date"], "2026-11-21");
    assert_eq!(failed["start"], "09:39");
    assert_eq!(failed["duration"], 2700);
    assert_eq!(failed["duration_text"], "0:45:00");
    assert_eq!(failed["status"], "failed");
    assert_eq!(failed["stage"], "review");
    assert_eq!(failed["completed_stage"], "transcription");
    assert_eq!(failed["error"], "La CLI claude a échoué.", "the failure's message, as the sheet keeps it");
    assert_eq!(failed["to_check"], 0);
    assert_eq!(failed["folder"], f.0.join("2026-11-21 09h39 - Point d'équipe").to_string_lossy().as_ref());
    let statuses: Vec<&str> = all.iter().map(|m| m["status"].as_str().unwrap()).collect();
    assert_eq!(statuses, ["recording_interrupted", "queued", "failed", "done", "to_process", "to_process"]);
    let done = &all[3];
    assert_eq!((done["stage"].is_null(), done["error"].is_null(), done["to_check"].as_u64(), done["completed_stage"].as_str()), (true, true, Some(1), Some("summary")));
    assert_eq!(greffier(&f, &["list", "--date", "2025-01-01", "--json"]).out, "[]\n");
}

// --- show ---

#[test]
fn show_gives_a_header_then_an_outline_of_the_summary() {
    let f = meetings();
    for name in ["2026-11-20-1000", RELEASE, &f.0.join(RELEASE).to_string_lossy(), "2026-11-20"] {
        let o = greffier(&f, &["show", name]);
        assert_eq!(o.code, 0, "{name}: {}", o.err);
        assert_eq!(
            o.out,
            format!(
                "2026-11-20-1000 · Revue de release\n\
                 2026-11-20 10:00 · 0:00:12 · done, 1 to check\n\
                 Speakers: Alexandre, Khalid ?\n\
                 Folder: {}\n\
                 Brief: On publie la release lundi si la recette passe vendredi.\n\
                 Topics:\n  [0:00:00] Date de la release\n  [0:00:05] Support\n\
                 Decisions:\n  [0:00:02] Release lundi si la recette passe vendredi.\n\
                 Actions: 2\n\
                 Next: --summary for the full summary, --transcription --from h:mm:ss --to h:mm:ss for what was said.\n",
                f.0.join(RELEASE).display()
            ),
            "{name}"
        );
    }
}

#[test]
fn a_path_names_a_meeting_only_inside_the_meetings_folder() {
    let f = meetings();
    let o = greffier(&f, &["show", &format!("{}/", f.0.join(RELEASE).display()), "--sheet"]);
    assert_eq!(o.code, 0, "{}", o.err);
    // Through another path to the same folder.
    let o = greffier(&f, &["show", &f.0.join("Divers/..").join(RELEASE).to_string_lossy(), "--sheet"]);
    assert_eq!(o.code, 0, "{}", o.err);
    // The same folder name elsewhere: another meetings folder's meeting, or a copy.
    let elsewhere = temp_folder();
    let path = elsewhere.0.join(RELEASE);
    let o = greffier(&f, &["show", &path.to_string_lossy()]);
    assert_eq!(o.code, 1);
    assert_eq!(o.err, format!("greffier: meeting not found: \"{}\" (not in the meetings folder {})\n", path.display(), f.0.canonicalize().unwrap().display()));
}

#[test]
fn the_outline_says_when_there_is_no_decision() {
    let f = meetings();
    let file = f.0.join(RELEASE).join(format!("{RELEASE} - bilan.md"));
    let summary = std::fs::read_to_string(&file).unwrap().replace("- (0:00:02) Release lundi si la recette passe vendredi.", "- Aucune décision.");
    std::fs::write(&file, summary).unwrap();
    let o = greffier(&f, &["show", "2026-11-20-1000"]);
    assert!(o.out.contains("\nDecisions: none\nActions: 2\n"), "{}", o.out);
}

#[test]
fn the_outline_counts_the_actions_as_check_action_numbers_them() {
    let f = meetings();
    let file = f.0.join(RELEASE).join(format!("{RELEASE} - bilan.md"));
    // A checkbox outside the Actions section: `--actions` and `check-action` number it too.
    let summary = std::fs::read_to_string(&file).unwrap().replace("## Questions ouvertes\n\n- Aucune.", "## Questions ouvertes\n\n- [ ] Qui prévient le support ?");
    std::fs::write(&file, summary).unwrap();
    let numbered = greffier(&f, &["show", "2026-11-20-1000", "--actions"]).out;
    assert_eq!(numbered.lines().count(), 3, "{numbered}");
    assert!(greffier(&f, &["show", "2026-11-20-1000"]).out.contains("\nActions: 3\n"));
    assert_eq!(greffier(&f, &["show", "2026-11-20-1000", "--json"]).json()["actions"], 3);
}

#[test]
fn show_without_a_summary_says_where_the_meeting_stands() {
    let f = meetings();
    let o = greffier(&f, &["show", "2026-11-21-0939"]);
    assert_eq!(o.code, 0, "{}", o.err);
    assert_eq!(
        o.out,
        format!(
            "2026-11-21-0939 · Point d'équipe\n2026-11-21 09:39 · 0:45:00 · failed: Review\nError: La CLI claude a échoué.\nFolder: {}\nNo summary yet (completed stage: Transcription).\n",
            f.0.join("2026-11-21 09h39 - Point d'équipe").display()
        )
    );
    let o = greffier(&f, &["show", "2026-11-19"]);
    assert!(o.out.ends_with("No summary yet (no stage completed).\n"), "{}", o.out);
    let o = greffier(&f, &["show", "2026-11-18"]);
    assert!(o.out.ends_with("No summary yet (completed stage: Transcription).\nNext: --transcription [--from h:mm:ss --to h:mm:ss] for what was said.\n"), "{}", o.out);
}

#[test]
fn an_unknown_or_ambiguous_meeting_fails_with_the_candidates() {
    let f = meetings();
    let o = greffier(&f, &["show", "2026-12-01"]);
    assert_eq!((o.code, o.err.as_str()), (1, "greffier: meeting not found: \"2026-12-01\"\n"));
    let o = greffier(&f, &["show", "2026-11-21"]);
    assert_eq!(o.code, 1);
    assert_eq!(o.err, "greffier: several meetings match \"2026-11-21\":\n2026-11-21-0939-2 · Rétro\n2026-11-21-0939 · Point d'équipe\n");
    assert!(o.out.is_empty());
}

/// Two meetings on 2026-11-23 whose titles have a colon (a dash in the folder name), one of
/// them with two speakers proposed under the same name.
fn colons() -> Folder {
    use greffier_core::{Speaker, Utterance};
    let f = temp_folder();
    let start = DateTime::new(2026, 11, 23, 9, 32);
    let mut m = Meeting::new(0, folder_name(start, "Revue : permissions"), start, 60.0);
    m.title = "Revue : permissions".into();
    m.completed_stage = Some(Stage::Transcription);
    m.speakers = (0..3).map(|i| Speaker::new(Speaker::numbered_label(i), 0.0)).collect();
    m.speakers[0].proposal = Some("Jérôme".into());
    m.speakers[1].proposal = Some("Jérôme".into());
    m.speakers[2].name = Some("Alexandre".into());
    m.utterances = (0..3).map(|i| Utterance { speaker: i, start: i as f64 * 10.0, end: i as f64 * 10.0 + 5.0, text: "Oui.".into(), passages: vec![] }).collect();
    MeetingsFolder::new(&f.0).write(&m).expect("written");
    write(&f.0, DateTime::new(2026, 11, 23, 14, 0), "Démo : bookmarks", 600.0, Status::ToProcess, None);
    f
}

#[test]
fn meetings_are_shown_with_their_titles_and_speakers_told_apart() {
    let f = colons();
    let o = greffier(&f, &["show", "2026-11-23"]);
    assert_eq!(o.err, "greffier: several meetings match \"2026-11-23\":\n2026-11-23-1400 · Démo : bookmarks\n2026-11-23-0932 · Revue : permissions\n", "the sheet's titles, as in list");
    let o = greffier(&f, &["show", "2026-11-23-0932"]);
    assert!(o.out.contains("\nSpeakers: Jérôme ? (SPEAKER_00), Jérôme ? (SPEAKER_01), Alexandre\n"), "{}", o.out);
}

#[test]
fn show_summary_and_sheet_give_the_files() {
    let f = meetings();
    let o = greffier(&f, &["show", "2026-11-20-1000", "--summary"]);
    assert_eq!(o.out, std::fs::read_to_string(f.0.join(RELEASE).join(format!("{RELEASE} - bilan.md"))).unwrap());
    let o = greffier(&f, &["show", "--sheet", "2026-11-20-1000"]);
    assert_eq!(o.out, std::fs::read_to_string(f.0.join(RELEASE).join(format!("{RELEASE} - réunion.md"))).unwrap());
    let o = greffier(&f, &["show", "2026-11-21-0939", "--summary"]);
    assert_eq!((o.code, o.err.as_str()), (1, "greffier: no summary yet for 2026-11-21-0939 (completed stage: Transcription)\n"));
    let o = greffier(&f, &["show", "2026-11-19", "--sheet"]);
    assert_eq!((o.code, o.err.as_str()), (1, "greffier: no sheet for 2026-11-19-1530\n"));
}

#[test]
fn the_summary_is_shown_without_its_front_matter() {
    let f = meetings();
    let file = f.0.join(RELEASE).join(format!("{RELEASE} - bilan.md"));
    let summary = std::fs::read_to_string(&file).unwrap();
    std::fs::write(&file, format!("---\ntype: bilan\n---\n\n{summary}")).unwrap();
    assert_eq!(greffier(&f, &["show", "2026-11-20-1000", "--summary"]).out, summary);
    assert!(greffier(&f, &["show", "2026-11-20-1000"]).out.contains("Folder: ") && !greffier(&f, &["show", "2026-11-20-1000"]).out.contains("type: bilan"));
}

#[test]
fn show_transcription_uses_the_displayed_names() {
    let f = meetings();
    let o = greffier(&f, &["show", "2026-11-20-1000", "--transcription"]);
    assert_eq!(
        o.out,
        "[0:00:00] Alexandre: Bonjour Khalid, on publie la release lundi ?\n\
         [0:00:02] Khalid ?: Oui, si la recette passe vendredi.\n\
         [0:00:05] Alexandre: Je préviens le support demain.\n"
    );
    let o = greffier(&f, &["show", "2026-11-20-1000", "--transcription", "--numbers", "--from", "0:03", "--to", "6"]);
    assert_eq!(o.out, "#2 [0:00:02] Khalid ?: Oui, si la recette passe vendredi.\n#3 [0:00:05] Alexandre: Je préviens le support demain.\n");
    let o = greffier(&f, &["show", "2026-11-20-1000", "--transcription", "--from", "0:00:10"]);
    assert_eq!(o.out, "No utterances in this range.\n");
    let o = greffier(&f, &["show", "2026-11-19", "--transcription"]);
    assert_eq!((o.code, o.err.as_str()), (1, "greffier: no transcription yet for 2026-11-19-1530\n"));
}

#[test]
fn show_transcription_in_json() {
    let f = meetings();
    let v = greffier(&f, &["show", "2026-11-20-1000", "--transcription", "--json", "--from", "2.5"]).json();
    assert_eq!(v["identifier"], "2026-11-20-1000");
    let r = v["utterances"].as_array().unwrap();
    assert_eq!(r.len(), 2);
    assert_eq!(r[0]["number"], 2);
    assert_eq!(r[0]["start"], 2.0);
    assert_eq!(r[0]["end"], 3.5);
    assert_eq!(r[0]["timestamp"], "0:00:02");
    assert_eq!(r[0]["speaker"], "Khalid ?");
    assert_eq!(r[0]["label"], "SPEAKER_01");
    assert_eq!(r[0]["text"], "Oui, si la recette passe vendredi.");
}

#[test]
fn show_speakers_actions_and_passages() {
    let f = meetings();
    let o = greffier(&f, &["show", "2026-11-20-1000", "--speakers"]);
    assert_eq!(
        o.out,
        "0 · SPEAKER_00 · Alexandre · 0:00:03 · 68 %\n\
         1 · SPEAKER_01 · Khalid ? · 0:00:01 · 32 %\n\
         2 · SPEAKER_02 · Alexandre · merged into SPEAKER_00\n"
    );
    let o = greffier(&f, &["show", "2026-11-20-1000", "--actions"]);
    assert_eq!(o.out, "#1 [ ] Alexandre — publier la release — lundi\n#2 [x] Khalid — lancer la recette — vendredi\n");
    let o = greffier(&f, &["show", "2026-11-20-1000", "--passages"]);
    assert_eq!(o.out, "#1 [0:00:05] to check: \"le support\" (hypothesis: \"l'équipe support\") · utterance #3\n");
    let o = greffier(&f, &["show", "2026-11-20-1000", "--passages", "--to", "0:00:04"]);
    assert_eq!(o.out, "No passages to check in this range.\n");
}

#[test]
fn show_views_in_json() {
    let f = meetings();
    let v = greffier(&f, &["show", "2026-11-20-1000", "--json"]).json();
    assert_eq!(v["identifier"], "2026-11-20-1000");
    assert_eq!(v["status"], "done");
    assert!(v.get("summary").is_none(), "the whole summary is --summary's");
    assert_eq!(v["brief"], "On publie la release lundi si la recette passe vendredi.");
    assert_eq!(v["topics"][1], serde_json::json!({ "start": 5.0, "timestamp": "0:00:05", "title": "Support" }));
    assert_eq!(v["decisions"][0], serde_json::json!({ "start": 2.0, "timestamp": "0:00:02", "text": "Release lundi si la recette passe vendredi." }));
    assert_eq!(v["actions"], 2);
    let speakers = v["speakers"].as_array().unwrap();
    assert_eq!(speakers.len(), 3);
    assert_eq!(speakers[1]["label"], "SPEAKER_01");
    assert_eq!(speakers[1]["name"], Value::Null);
    assert_eq!(speakers[1]["proposal"], "Khalid");
    assert_eq!(speakers[1]["displayed"], "Khalid ?");
    assert_eq!(speakers[2]["merged_into"], "SPEAKER_00");
    assert_eq!(speakers[0]["speaking_time"], 3);
    let v = greffier(&f, &["show", "2026-11-21-0939", "--json"]).json();
    assert_eq!((v["brief"].is_null(), v["topics"].is_null(), v["decisions"].is_null(), v["actions"].is_null()), (true, true, true, true), "no summary yet");

    let v = greffier(&f, &["show", "2026-11-20-1000", "--actions", "--json"]).json();
    assert_eq!(v["actions"][1]["number"], 2);
    assert_eq!(v["actions"][1]["done"], true);
    assert_eq!(v["actions"][1]["text"], "Khalid — lancer la recette — vendredi");

    let v = greffier(&f, &["show", "2026-11-20-1000", "--passages", "--json"]).json();
    let p = &v["passages"][0];
    assert_eq!((p["number"].as_u64(), p["utterance"].as_u64(), p["start"].as_f64()), (Some(1), Some(3), Some(5.0)));
    assert_eq!((p["status"].as_str(), p["excerpt"].as_str(), p["hypothesis"].as_str()), (Some("to_check"), Some("le support"), Some("l'équipe support")));
    assert!(p["correction"].is_null());

    let v = greffier(&f, &["show", "2026-11-20-1000", "--summary", "--json"]).json();
    assert!(v["summary"].as_str().unwrap().starts_with("## En bref"), "the file, as Greffier writes it");
    let v = greffier(&f, &["show", "2026-11-20-1000", "--sheet", "--json"]).json();
    assert!(v["sheet"].as_str().unwrap().starts_with("---\ntitre: \"Revue de release\""));
    let v = greffier(&f, &["show", "2026-11-20-1000", "--speakers", "--json"]).json();
    assert_eq!(v["speakers"][0]["name"], "Alexandre");
    // The glossary's Identifier, in every view.
    for view in ["--summary", "--sheet", "--transcription", "--speakers", "--actions", "--passages"] {
        let v = greffier(&f, &["show", "2026-11-20-1000", view, "--json"]).json();
        assert_eq!((v["identifier"].as_str(), v.get("id")), (Some("2026-11-20-1000"), None), "{view}");
    }
}

// --- status ---

#[test]
fn status_lists_the_meetings_that_are_not_done() {
    let f = meetings();
    let o = greffier(&f, &["status"]);
    assert_eq!(o.code, 0, "{}", o.err);
    assert_eq!(
        o.out,
        "2026-11-21-0939-2 · queued · Rétro\n\
         2026-11-21-0939 · failed: Review · Point d'équipe · La CLI claude a échoué.\n\
         2026-11-22-0800 · recording interrupted · Démo\n\
         2026-11-19-1530 · to process · Réunion\n\
         2026-11-18-0915 · to process · Réunion\n"
    );
    let empty = temp_folder();
    assert_eq!(greffier(&empty, &["status"]).out, "Nothing in progress.\n");
    assert_eq!(greffier(&empty, &["status", "--json"]).out, "[]\n");
    assert_eq!(greffier(&empty, &["list"]).out, "No meetings.\n");
}

#[test]
fn status_of_a_meeting() {
    let f = meetings();
    let o = greffier(&f, &["status", "2026-11-21-0939"]);
    assert_eq!(o.out, "2026-11-21-0939 · Point d'équipe\nStatus: failed: Review\nCompleted stage: Transcription\nError: La CLI claude a échoué.\n");
    let o = greffier(&f, &["status", "2026-11-20-1000"]);
    assert_eq!(o.out, "2026-11-20-1000 · Revue de release\nStatus: done, 1 to check\nCompleted stage: Summary\n");
    let o = greffier(&f, &["status", "2026-11-19"]);
    assert_eq!(o.out, "2026-11-19-1530 · Réunion\nStatus: to process\nCompleted stage: none\n");
    let v = greffier(&f, &["status", "2026-11-21-0939", "--json"]).json();
    assert_eq!((v["identifier"].as_str(), v["status"].as_str(), v["stage"].as_str(), v["completed_stage"].as_str()), (Some("2026-11-21-0939"), Some("failed"), Some("review"), Some("transcription")));
    assert_eq!(v["error"], "La CLI claude a échoué.");
    for live in ["rank", "progress", "time_remaining"] {
        assert!(v[live].is_null(), "{live}: from disk, nothing live");
    }
}

/// The running Greffier process, as the command line will see it.
struct Queue;

impl LiveSource for Queue {
    fn live(&self, folder_name: &str) -> Option<Live> {
        match folder_name {
            "2026-11-19 15h30 - Réunion" => Some(Live { status: Status::Running { stage: Stage::Transcription, progress: 0.4 }, rank: Some(1), time_remaining: Some(420.0) }),
            "2026-11-21 09h39 - Rétro" => Some(Live { status: Status::Pending(Stage::Transcription), rank: Some(2), time_remaining: Some(900.0) }),
            _ => None,
        }
    }
}

#[test]
fn live_data_from_the_process_takes_over_the_disk() {
    let f = meetings();
    let cx = Context { meetings_folder: Some(f.0.clone()), app_folder: None, live: Some(Box::new(Queue)), process: None };
    let o = run_in(&cx, &["status"]);
    assert!(o.out.starts_with("2026-11-19-1530 · Transcription 40 % · 7 min · Réunion\n2026-11-21-0939-2 · 2nd in queue · 15 min · Rétro\n"), "{}", o.out);
    let v = run_in(&cx, &["status", "2026-11-19", "--json"]).json();
    assert_eq!((v["status"].as_str(), v["stage"].as_str(), v["rank"].as_u64(), v["progress"].as_f64(), v["time_remaining"].as_u64()), (Some("running"), Some("transcription"), Some(1), Some(0.4), Some(420)));
}

// --- Reading writes nothing ---

/// Every file under `root`, with its size and modification time.
fn snapshot(root: &Path) -> Vec<(PathBuf, u64, std::time::SystemTime)> {
    let mut all = vec![];
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let m = e.metadata().unwrap();
            if m.is_dir() {
                stack.push(e.path());
            }
            all.push((e.path(), m.len(), m.modified().unwrap()));
        }
    }
    all.sort();
    all
}

#[test]
fn reading_writes_nothing() {
    let f = meetings();
    let before = snapshot(&f.0);
    for args in [
        &["list"][..],
        &["list", "--json"],
        &["show", "2026-11-20-1000"],
        &["show", "2026-11-20-1000", "--json"],
        &["show", "2026-11-20-1000", "--transcription"],
        &["show", "2026-11-20-1000", "--speakers"],
        &["show", "2026-11-20-1000", "--actions"],
        &["show", "2026-11-20-1000", "--passages"],
        &["show", "2026-11-20-1000", "--sheet"],
        &["status"],
        &["status", "2026-11-18"],
    ] {
        assert_eq!(greffier(&f, args).code, 0, "{args:?}");
    }
    assert_eq!(snapshot(&f.0), before);
}
