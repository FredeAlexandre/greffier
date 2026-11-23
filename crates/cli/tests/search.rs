//! `greffier search`, end to end on a temporary meetings folder: the core's sample meetings
//! (`crates/core/tests/fixtures/meetings-folder`) plus a meeting about bookmarks with a summary
//! and fourteen utterances, and, for the cache, a temporary app folder.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use greffier_cli::Context;
use greffier_core::{Command, DateTime, Greffier, Meeting, MeetingsFolder, Role, Speaker, Stage, Status, Utterance, folder::folder_name};
use serde_json::{Value, json};

/// A temporary folder, deleted at the end.
struct Temp(PathBuf);

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn temp() -> Temp {
    static N: AtomicU32 = AtomicU32::new(0);
    let d = std::env::temp_dir().join(format!("greffier-cli-search-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("temporary folder");
    Temp(d)
}

fn copy_to(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("copy");
    for e in std::fs::read_dir(from).expect("fixtures").flatten() {
        let target = to.join(e.file_name());
        if e.file_type().expect("type").is_dir() { copy_to(&e.path(), &target) } else { std::fs::copy(e.path(), target).map(|_| ()).expect("copy") }
    }
}

/// The sample meetings, plus 2026-11-23 09h32 « Démo bookmarks », done: utterance `i` starts at
/// `i` minutes; Alexandre says « Les bookmarks, point i. » on the even ones, « Khalid ? »
/// answers « Et les labels ? (i) » on the odd ones.
fn meetings() -> Temp {
    let f = temp();
    copy_to(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../core/tests/fixtures/meetings-folder"), &f.0);
    let start = DateTime::new(2026, 11, 23, 9, 32);
    let mut m = Meeting::new(0, folder_name(start, "Démo bookmarks"), start, 900.0);
    m.title = "Démo bookmarks".into();
    m.status = Status::Done;
    m.completed_stage = Some(Stage::Summary);
    m.speakers = vec![Speaker::new(Speaker::numbered_label(0), 0.0), Speaker::new(Speaker::numbered_label(1), 0.0)];
    m.speakers[0].name = Some("Alexandre".into());
    m.speakers[1].proposal = Some("Khalid".into());
    m.utterances = (0..14)
        .map(|i| {
            let text = if i % 2 == 0 { format!("Les bookmarks, point {i}.") } else { format!("Et les labels ? ({i})") };
            Utterance { speaker: i % 2, start: i as f64 * 60.0, end: i as f64 * 60.0 + 30.0, text, passages: vec![] }
        })
        .collect();
    let folder = MeetingsFolder::new(&f.0);
    folder.write(&m).expect("written");
    let summary = "## En bref\n\nOn a montré les bookmarks.\n\n## Sujets abordés\n\n### Bookmarks (0:00:00)\n\n- Les bookmarks plaisent.\n\n## Décisions\n\n- (0:02:00) On garde les bookmarks.\n\n## Actions\n\n- [ ] **Alexandre** — refaire les bookmarks — lundi\n";
    std::fs::write(folder.file(&m.folder_name, Role::Summary), summary).expect("summary");
    f
}

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

fn run(meetings: &Path, app: Option<&Path>, args: &[&str]) -> Output {
    let cx = Context { meetings_folder: Some(meetings.to_path_buf()), app_folder: app.map(Path::to_path_buf), live: None, process: None };
    let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = greffier_cli::run_with(&cx, &args, &mut out, &mut err);
    Output { code, out: String::from_utf8(out).expect("UTF-8"), err: String::from_utf8(err).expect("UTF-8") }
}

fn search(f: &Temp, args: &[&str]) -> Output {
    let mut all = vec!["search"];
    all.extend(args);
    run(&f.0, None, &all)
}

#[test]
fn search_gives_the_lines_found_by_meeting_summary_first() {
    let f = meetings();
    let o = search(&f, &["recette"]);
    assert_eq!((o.code, o.err.as_str()), (0, ""));
    assert_eq!(
        o.out,
        "2026-11-20-1000 · Revue de release\n\
         \x20 summary · On publie la release lundi si la recette passe vendredi.\n\
         \x20 [0:00:00] summary · Lundi, si la recette passe.\n\
         \x20 [0:00:02] summary · Release lundi si la recette passe vendredi.\n\
         \x20 summary · Khalid — lancer la recette — vendredi\n\
         \x20 [0:00:02] Khalid ?: Oui, si la recette passe vendredi.\n"
    );
}

#[test]
fn the_most_recent_meeting_comes_first_and_the_title_counts() {
    let f = meetings();
    let o = search(&f, &["bookmarks", "--in", "title"]);
    assert_eq!(o.out, "2026-11-23-0932 · Démo bookmarks\n  title\n");
    let o = search(&f, &["re", "--in", "title,transcription", "--limit", "5"]);
    let headers: Vec<&str> = o.out.lines().filter(|l| !l.starts_with(' ') && !l.starts_with('…')).collect();
    assert_eq!(headers, ["2026-11-20-1000 · Revue de release", "2026-11-18-0915 · Réunion"], "{}", o.out);
}

#[test]
fn words_need_not_follow_each_other_but_phrases_must() {
    let f = meetings();
    let found = |args: &[&str]| search(&f, args).out.lines().filter(|l| l.starts_with("  ")).count();
    assert_eq!(found(&["vendredi", "recette"]), 4);
    assert_eq!(found(&["\"recette passe vendredi\""]), 3);
    assert_eq!(found(&["\"recette", "passe", "vendredi\""]), 3, "the arguments are joined");
    assert_eq!(found(&["\"vendredi recette\""]), 0);
    assert_eq!(found(&["RECETTE"]), 5, "case and accents ignored");
}

#[test]
fn nothing_found_is_not_a_failure() {
    let f = meetings();
    let o = search(&f, &["licorne", "rose"]);
    let hint = "No results for \"licorne rose\": all the words must be on the same line (a summary line or an utterance); search fewer words, or one.\n";
    assert_eq!((o.code, o.out.as_str(), o.err.as_str()), (0, hint, ""));
    // Each word is found, but never on the same line.
    assert!(search(&f, &["recette", "support"]).out.starts_with("No results for \"recette support\": all the words"));
    assert_eq!(search(&f, &["licorne"]).out, "No results for \"licorne\".\n", "one word: nothing to add");
    assert_eq!(search(&f, &["\"licorne rose\""]).out, "No results for \"\"licorne rose\"\".\n", "one phrase neither");
    let v = search(&f, &["licorne", "--json"]).json();
    assert_eq!(v, json!({ "results": [], "total": 0, "meetings": 0 }));
}

#[test]
fn search_usage_errors() {
    let f = meetings();
    for (args, message) in [
        (&[][..], "missing the query"),
        (&["\"\""], "missing the query"),
        (&["  "], "missing the query"),
        (&["x", "--in", "everywhere"], "--in: \"everywhere\" (summary, transcription or title)"),
        (&["x", "--in", "bilan"], "--in: \"bilan\" (summary, transcription or title)"),
        (&["x", "--in", "title", "--speaker", "Khalid"], "--speaker only searches the transcription"),
        (&["x", "--limit", "0"], "--limit needs a number"),
        (&["x", "--context", "lots"], "--context needs a number"),
        (&["x", "--since", "yesterday"], "invalid date: \"yesterday\""),
        (&["x", "--frobnicate"], "unknown option: --frobnicate"),
    ] {
        let o = search(&f, args);
        assert_eq!(o.code, 2, "{args:?}: {}", o.out);
        assert!(o.err.starts_with("greffier: ") && o.err.contains(message), "{args:?}: {}", o.err);
    }
}

#[test]
fn search_filters_by_place_speaker_and_date() {
    let f = meetings();
    let o = search(&f, &["recette", "--in", "transcription"]);
    assert_eq!(o.out, "2026-11-20-1000 · Revue de release\n  [0:00:02] Khalid ?: Oui, si la recette passe vendredi.\n");
    assert_eq!(search(&f, &["recette", "--in", "summary", "--in", "transcription"]).out, search(&f, &["recette"]).out);
    let o = search(&f, &["les", "--speaker", "khalid", "--limit", "2"]);
    assert_eq!(
        o.out,
        "2026-11-23-0932 · Démo bookmarks\n\
         \x20 [0:01:00] Khalid ?: Et les labels ? (1)\n\
         \x20 [0:03:00] Khalid ?: Et les labels ? (3)\n\
         … 5 more results in 1 meeting: 2026-11-23-0932 (5)\n\
         Narrow down (--date, --since, --until, --speaker, --in) or raise --limit.\n"
    );
    assert!(search(&f, &["recette", "--date", "2026-11"]).out.starts_with("2026-11-20-1000"));
    assert!(search(&f, &["recette", "--date", "2026-11-20"]).out.starts_with("2026-11-20-1000"));
    assert_eq!(search(&f, &["recette", "--since", "2026-11-21"]).out, "No results for \"recette\".\n");
    assert_eq!(search(&f, &["bookmarks", "--until", "2026-11-22"]).out, "No results for \"bookmarks\".\n");
}

#[test]
fn one_meeting_found_shows_all_its_lines_within_the_limit() {
    let f = meetings();
    // 7 utterances in a single meeting: more than the 5 per meeting when results spread.
    let o = search(&f, &["bookmarks", "--in", "transcription"]);
    assert_eq!(o.out.lines().count(), 8, "{}", o.out);
    let o = search(&f, &["bookmarks"]);
    assert_eq!(
        o.out,
        "2026-11-23-0932 · Démo bookmarks\n\
         \x20 title\n\
         \x20 summary · On a montré les bookmarks.\n\
         \x20 [0:00:00] summary · Bookmarks\n\
         \x20 [0:00:00] summary · Les bookmarks plaisent.\n\
         \x20 [0:02:00] summary · On garde les bookmarks.\n\
         \x20 summary · Alexandre — refaire les bookmarks — lundi\n\
         \x20 [0:00:00] Alexandre: Les bookmarks, point 0.\n\
         \x20 [0:02:00] Alexandre: Les bookmarks, point 2.\n\
         \x20 [0:04:00] Alexandre: Les bookmarks, point 4.\n\
         \x20 [0:06:00] Alexandre: Les bookmarks, point 6.\n\
         \x20 [0:08:00] Alexandre: Les bookmarks, point 8.\n\
         \x20 [0:10:00] Alexandre: Les bookmarks, point 10.\n\
         \x20 [0:12:00] Alexandre: Les bookmarks, point 12.\n"
    );
    let o = search(&f, &["bookmarks", "--limit", "8"]);
    assert_eq!(o.out.lines().count(), 11, "{}", o.out);
    assert!(o.out.ends_with("\n… 5 more results in 1 meeting: 2026-11-23-0932 (5)\nNarrow down (--date, --since, --until, --speaker, --in) or raise --limit.\n"), "{}", o.out);
}

#[test]
fn everything_found_is_shown_when_it_fits_in_the_limit() {
    let f = meetings();
    // 7 utterances on 2026-11-23, 1 on 2026-11-18: 8 lines fit in 8, even though 8 / 2 < 7.
    let o = search(&f, &["point", "--in", "transcription", "--limit", "8"]);
    assert_eq!(o.out.lines().filter(|l| l.starts_with("  [")).count(), 8, "{}", o.out);
    assert!(!o.out.contains("more"), "{}", o.out);
    // When it does not fit, the cap per meeting is the largest that does: 6 + 1.
    let o = search(&f, &["point", "--in", "transcription", "--limit", "7"]);
    assert_eq!(o.out.lines().filter(|l| l.starts_with("  [")).count(), 7, "{}", o.out);
    assert!(o.out.contains("\n2026-11-18-0915 · Réunion\n"), "the older meeting is not crowded out: {}", o.out);
    assert!(o.out.ends_with("… 1 more result in 1 meeting: 2026-11-23-0932 (1)\nNarrow down (--date, --since, --until, --speaker, --in) or raise --limit.\n"), "{}", o.out);
}

#[test]
fn context_gives_the_utterances_around_once() {
    let f = meetings();
    let o = search(&f, &["point", "4", "--context", "1"]);
    assert_eq!(
        o.out,
        "2026-11-23-0932 · Démo bookmarks\n\
         \x20   [0:03:00] Khalid ?: Et les labels ? (3)\n\
         \x20 [0:04:00] Alexandre: Les bookmarks, point 4.\n\
         \x20   [0:05:00] Khalid ?: Et les labels ? (5)\n"
    );
    // An utterance found is shown once, as found, even when it is also around another one.
    let o = search(&f, &["point", "--context", "2", "--in", "transcription", "--date", "2026-11-23", "--limit", "2"]);
    assert_eq!(
        o.out,
        "2026-11-23-0932 · Démo bookmarks\n\
         \x20 [0:00:00] Alexandre: Les bookmarks, point 0.\n\
         \x20   [0:01:00] Khalid ?: Et les labels ? (1)\n\
         \x20 [0:02:00] Alexandre: Les bookmarks, point 2.\n\
         \x20   [0:03:00] Khalid ?: Et les labels ? (3)\n\
         \x20   [0:04:00] Alexandre: Les bookmarks, point 4.\n\
         … 5 more results in 1 meeting: 2026-11-23-0932 (5)\n\
         Narrow down (--date, --since, --until, --speaker, --in) or raise --limit.\n"
    );
    // « point » is also said on 2026-11-18, a meeting not shown.
    let o = search(&f, &["point", "--context", "1", "--limit", "2"]);
    assert_eq!(
        o.out,
        "2026-11-23-0932 · Démo bookmarks\n\
         \x20 [0:00:00] Alexandre: Les bookmarks, point 0.\n\
         \x20   [0:01:00] Khalid ?: Et les labels ? (1)\n\
         \x20 [0:02:00] Alexandre: Les bookmarks, point 2.\n\
         \x20   [0:03:00] Khalid ?: Et les labels ? (3)\n\
         … 6 more results in 2 meetings: 2026-11-23-0932 (5), 2026-11-18-0915 (1)\n\
         Narrow down (--date, --since, --until, --speaker, --in) or raise --limit.\n"
    );
}

#[test]
fn search_in_json() {
    let f = meetings();
    let v = search(&f, &["recette", "--json"]).json();
    assert_eq!((v["total"].as_u64(), v["meetings"].as_u64()), (Some(5), Some(1)));
    let r = v["results"].as_array().unwrap();
    assert_eq!(r.len(), 5);
    let highlighted = |r: &Value| -> Vec<String> {
        let excerpt = r["excerpt"].as_str().unwrap();
        r["highlights"].as_array().unwrap().iter().map(|s| excerpt[s[0].as_u64().unwrap() as usize..s[1].as_u64().unwrap() as usize].to_string()).collect()
    };
    assert_eq!(r[0], json!({
        "identifier": "2026-11-20-1000", "title": "Revue de release", "location": "summary", "instant": null, "timestamp": null,
        "speaker": null, "utterance": null, "excerpt": "On publie la release lundi si la recette passe vendredi.", "highlights": [[33, 40]], "before": [], "after": [],
    }));
    assert_eq!((r[2]["excerpt"].as_str(), r[2]["instant"].as_f64(), highlighted(&r[2])), (Some("Release lundi si la recette passe vendredi."), Some(2.0), vec!["recette".to_string()]), "its own timestamp left out");
    assert_eq!(
        (r[4]["location"].as_str(), r[4]["timestamp"].as_str(), r[4]["speaker"].as_str(), r[4]["utterance"].as_u64()),
        (Some("transcription"), Some("0:00:02"), Some("Khalid ?"), Some(2))
    );
    let v = search(&f, &["point 4", "--context", "1", "--json"]).json();
    let r = &v["results"][0];
    assert_eq!(r["before"], json!([{ "utterance": 4, "start": 180.0, "timestamp": "0:03:00", "speaker": "Khalid ?", "text": "Et les labels ? (3)" }]));
    assert_eq!(r["after"][0]["utterance"], 6);
    let v = search(&f, &["bookmarks", "--in", "title", "--json"]).json();
    assert_eq!((v["results"][0]["location"].as_str(), v["results"][0]["excerpt"].as_str()), (Some("title"), Some("Démo bookmarks")));
}

// --- The search cache, read only ---

/// Every file under `root`, with its bytes and modification time.
fn snapshot(root: &Path) -> Vec<(PathBuf, Vec<u8>, std::time::SystemTime)> {
    let mut all = vec![];
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let m = e.metadata().unwrap();
            if m.is_dir() {
                stack.push(e.path());
            }
            all.push((e.path(), if m.is_file() { std::fs::read(e.path()).unwrap() } else { vec![] }, m.modified().unwrap()));
        }
    }
    all.sort();
    all
}

/// The app folder with the search cache Greffier keeps for these meetings.
fn app_with_cache(meetings: &Temp) -> Temp {
    let app = temp();
    let mut g = Greffier::open(&meetings.0, greffier_core::demo::adapters());
    let settings = greffier_core::Settings { search_cache: Some(greffier_core::settings::search_cache(&app.0)), ..g.settings().clone() };
    g.execute(Command::ChangeSettings(settings)).expect("cache");
    app
}

#[test]
fn search_reads_the_apps_cache_and_writes_nothing() {
    let f = meetings();
    let app = app_with_cache(&f);
    let cache = greffier_core::settings::search_cache(&app.0);
    // An entry changed here only to see that the cache is read (its fingerprints still match).
    let release = cache.join("2026-11-20 10h00 - Revue de release.json");
    let tampered = std::fs::read_to_string(&release).unwrap().replace("Je préviens le support demain.", "Je préviens le kangourou demain.");
    std::fs::write(&release, tampered).unwrap();
    // A stale entry: the meeting changed since.
    let transcription = f.0.join("2026-11-18 09h15 - Réunion/2026-11-18 09h15 - Réunion - transcription.json");
    std::fs::write(&transcription, std::fs::read_to_string(&transcription).unwrap().replace("deux mesures", "trois mesures")).unwrap();
    let (meetings_before, app_before) = (snapshot(&f.0), snapshot(&app.0));

    let found = |args: &[&str]| {
        let mut all = vec!["search"];
        all.extend(args);
        run(&f.0, Some(&app.0), &all)
    };
    assert!(found(&["kangourou"]).out.contains("Je préviens le kangourou demain."));
    assert!(found(&["trois", "mesures"]).out.contains("Il reste trois mesures à refaire."), "a stale entry is rebuilt in memory");
    for args in [&["recette"][..], &["bookmarks", "--json", "--context", "2"], &["les", "--speaker", "Khalid"], &["licorne"]] {
        assert_eq!(found(args).code, 0, "{args:?}");
    }
    assert_eq!(snapshot(&f.0), meetings_before, "the meetings folder is untouched");
    assert_eq!(snapshot(&app.0), app_before, "the app folder is untouched");

    // Without a cache yet, nothing is created.
    let empty = temp();
    assert_eq!(run(&f.0, Some(&empty.0), &["search", "recette"]).code, 0);
    assert!(std::fs::read_dir(&empty.0).unwrap().next().is_none(), "no cache written");
}
