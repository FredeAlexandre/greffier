//! The commands that write, from the command line to a Greffier process running in a thread of
//! the test (a core on fake adapters, a real socket): what each sends, what it prints, its exit
//! code; reaching the process, and refusing one on another meetings folder.

mod common;

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use common::{Output, Served, Temp, adapters, failing_adapters, fixtures, media, run_in, serve};
use greffier_cli::Context;
use greffier_cli::client::{BUNDLE_ID, Launch, Process};

const RELEASE: &str = "2026-11-20 10h00 - Revue de release";

/// The command line on `served`'s folder, reaching its socket; nothing is launched.
fn cx(served: &Served) -> Context {
    on(&served.folder.0, &served.app.0)
}

fn on(folder: &Path, app: &Path) -> Context {
    let mut process = Process::new(app, Launch::Nothing);
    process.patience = Duration::from_millis(200);
    process.poll = Duration::from_millis(20);
    Context { meetings_folder: Some(folder.to_path_buf()), app_folder: None, live: None, process: Some(process) }
}

fn ok(cx: &Context, args: &[&str]) -> String {
    let o = run_in(cx, args);
    assert_eq!(o.code, 0, "{args:?}: {}", o.err);
    assert!(o.err.is_empty(), "{args:?}: {}", o.err);
    o.out
}

fn failed(cx: &Context, args: &[&str], code: i32) -> Output {
    let o = run_in(cx, args);
    assert_eq!(o.code, code, "{args:?}: out {} err {}", o.out, o.err);
    assert!(o.out.is_empty(), "{args:?}: {}", o.out);
    o
}

#[test]
fn rename_goes_through_the_process() {
    let s = serve(fixtures(), adapters, 0.0);
    let cx = cx(&s);
    assert_eq!(ok(&cx, &["rename", "2026-11-20", "Revue", "de", "la", "1.4"]), "2026-11-20-1000 · renamed: Revue de la 1.4\n");
    assert!(s.folder.0.join("2026-11-20 10h00 - Revue de la 1.4").is_dir());
    assert!(!s.folder.0.join(RELEASE).exists());
}

#[test]
fn every_command_that_writes_says_what_it_did_in_one_line() {
    let s = serve(fixtures(), adapters, 0.0);
    let cx = cx(&s);
    for (args, said) in [
        (&["speaker", "name", "2026-11-20", "1", "Khalid", "B."][..], "2026-11-20-1000 · SPEAKER_01: Khalid B.\n"),
        (&["speaker", "merge", "2026-11-20", "SPEAKER_01", "0"], "2026-11-20-1000 · SPEAKER_01 merged into SPEAKER_00\n"),
        (&["speaker", "split", "2026-11-20", "SPEAKER_01"], "2026-11-20-1000 · SPEAKER_01 split\n"),
        (&["correct", "2026-11-20", "#2", "Oui,", "si", "la", "recette", "passe", "jeudi."], "2026-11-20-1000 · utterance #2 corrected\n"),
        (&["resolve", "2026-11-20", "1", "correct", "l'équipe", "support"], "2026-11-20-1000 · passage #1: corrected\n"),
        (&["resolve", "2026-11-20", "#1", "reopen"], "2026-11-20-1000 · passage #1: reopened\n"),
        (&["resolve", "2026-11-20", "1", "accept"], "2026-11-20-1000 · passage #1: hypothesis accepted\n"),
        (&["resolve", "2026-11-20", "1", "keep"], "2026-11-20-1000 · passage #1: original kept\n"),
        (&["check-action", "2026-11-20", "1"], "2026-11-20-1000 · action #1 checked\n"),
        (&["check-action", "2026-11-20", "1", "--undo"], "2026-11-20-1000 · action #1 unchecked\n"),
        (&["redo", "2026-11-20", "summary"], "2026-11-20-1000 · queued: Summary\n"),
        (&["stop", "2026-11-20"], "2026-11-20-1000 · processing stopped\n"),
        (&["process", "2026-11-19", "--speakers", "4"], "2026-11-19-1530 · queued: Transcription\n"),
        (&["delete", "2026-11-18", "--yes"], "2026-11-18-0915 · moved to the trash\n"),
    ] {
        assert_eq!(ok(&cx, args), said, "{args:?}");
    }
    let transcription = std::fs::read_to_string(s.folder.0.join(RELEASE).join(format!("{RELEASE} - transcription.md"))).unwrap();
    assert!(transcription.contains("passe jeudi."), "{transcription}");
    assert!(!s.folder.0.join("2026-11-18 09h15 - Réunion").exists());
    let sheet = std::fs::read_to_string(s.folder.0.join("2026-11-19 15h30 - Réunion/2026-11-19 15h30 - Réunion - réunion.md")).unwrap();
    assert!(sheet.contains("nombre_de_locuteurs: 4"), "{sheet}");
}

#[test]
fn after_a_double_dash_everything_is_text() {
    let s = serve(fixtures(), adapters, 0.0);
    let cx = cx(&s);
    assert_eq!(ok(&cx, &["correct", "2026-11-20", "2", "--", "-5", "degrés", "--json"]), "2026-11-20-1000 · utterance #2 corrected\n");
    let transcription = std::fs::read_to_string(s.folder.0.join(RELEASE).join(format!("{RELEASE} - transcription.md"))).unwrap();
    assert!(transcription.contains("-5 degrés --json"), "{transcription}");
    assert_eq!(failed(&cx, &["correct", "2026-11-20", "2", "-5", "degrés"], 2).err, "greffier: unknown option: -5 (see greffier help)\n");
}

#[test]
fn json_gives_the_identifier_and_the_folder() {
    let s = serve(fixtures(), adapters, 0.0);
    let out = ok(&cx(&s), &["rename", "2026-11-20", "Revue", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["identifier"], "2026-11-20-1000");
    assert!(v.get("id").is_none());
    assert_eq!(v["folder"], s.folder.0.join("2026-11-20 10h00 - Revue").to_string_lossy().as_ref());
    assert_eq!(v["message"], "2026-11-20-1000 · renamed: Revue");
}

#[test]
fn what_the_process_refuses_exits_with_1() {
    let s = serve(fixtures(), adapters, 0.0);
    let cx = cx(&s);
    for (args, message) in [
        (&["rename", "2025", "Title"][..], "greffier: meeting not found: \"2025\"\n"),
        (&["speaker", "name", "2026-11-20", "Bob", "X"], "greffier: speaker not found: \"Bob\" (index, label SPEAKER_00 or displayed name)\n"),
        (&["stop", "2026-11-20"], "greffier: the meeting is not being processed\n"),
        (&["check-action", "2026-11-20", "9"], "greffier: action not found: \"9\" (number from 1 to 2)\n"),
    ] {
        assert_eq!(failed(&cx, args, 1).err, message, "{args:?}");
    }
}

#[test]
fn the_core_s_refusals_come_back_in_english() {
    let s = serve(fixtures(), adapters, 0.0);
    let cx = cx(&s);
    let notes = s.folder.0.join("notes.txt");
    for (args, message) in [
        (&["process", "2026-11-20"][..], "the meeting is already processed (greffier redo to redo a stage)"),
        (&["redo", "2026-11-19", "review"], "the earlier stages are not done"),
        (&["stop", "2026-11-19"], "the meeting is not being processed"),
        (&["rename", "2026-11-20", " "], "the title is empty"),
        (&["speaker", "name", "2026-11-20", "0", " "], "the name is empty"),
        (&["speaker", "merge", "2026-11-20", "0", "2"], "these two speakers are already the same voice"),
        (&["speaker", "split", "2026-11-20", "0"], "this speaker is not merged"),
        (&["correct", "2026-11-20", "3", "Je", "préviens", "la", "hotline", "demain."], "this text is in a passage to check: resolve the passage instead"),
        (&["resolve", "2026-11-20", "1", "correct", " "], "the correction is empty"),
        (&["import", notes.to_str().unwrap()], "\"notes.txt\": Greffier imports a medium (video or audio), a transcription (.vtt, .srt, .json) and a summary (.md)."),
    ] {
        assert_eq!(failed(&cx, args, 1).err, format!("greffier: {message}\n"), "{args:?}");
    }
}

#[test]
fn usage_errors_exit_with_2_and_send_nothing() {
    // No process anywhere: anything sent would exit with 3.
    let f = fixtures();
    let app = Temp::new();
    let cx = on(&f.0, &app.0);
    for (args, message) in [
        (&["delete", "2026-11-20"][..], "greffier delete moves the meeting to the trash: confirm with greffier delete 2026-11-20 --yes"),
        (&["redo", "2026-11-20", "bilan"], "unknown stage: \"bilan\" (transcription, review or summary)"),
        (&["redo", "2026-11-20"], "missing the stage (transcription, review or summary)"),
        (&["rename", "2026-11-20"], "missing the title"),
        (&["rename"], "missing the meeting"),
        (&["speaker", "rename", "2026-11-20", "0"], "greffier speaker rename: name, merge or split"),
        (&["speaker", "merge", "2026-11-20", "0"], "missing the speaker to merge into"),
        (&["resolve", "2026-11-20", "1", "correct"], "missing the correction's text"),
        (&["resolve", "2026-11-20", "1", "accept", "yes"], "extra argument: \"yes\""),
        (&["resolve", "2026-11-20", "1", "accepter"], "\"accepter\": accept, keep, reopen or correct <text>"),
        (&["process", "2026-11-19", "--speakers", "0"], "--speakers needs a number of speakers: \"0\""),
        (&["process", "2026-11-19", "--timeout", "5"], "--timeout goes with --wait"),
        (&["process", "2026-11-19", "--wait", "--timeout", "soon"], "--timeout needs seconds: \"soon\""),
        (&["stop", "2026-11-19", "--wait"], "unknown option: --wait"),
        (&["import"], "missing the file to import"),
        (&["import", "x.mp4", "--no-process", "--wait"], "--wait does not go with --no-process"),
        (&["check-action", "2026-11-20"], "missing the action's number"),
    ] {
        assert_eq!(failed(&cx, args, 2).err, format!("greffier: {message} (see greffier help)\n"), "{args:?}");
    }
}

#[test]
fn help_lists_the_commands_that_write() {
    let s = serve(fixtures(), adapters, 0.0);
    let out = ok(&cx(&s), &["delete", "--help"]);
    for command in ["greffier import", "greffier process", "greffier redo", "greffier stop", "greffier rename", "greffier speaker", "greffier correct", "greffier resolve", "greffier check-action", "greffier delete", "3 Greffier out of reach"] {
        assert!(out.contains(command), "{command}");
    }
}

#[test]
fn a_process_on_another_meetings_folder_is_refused() {
    let s = serve(fixtures(), adapters, 0.0);
    let other = fixtures();
    let o = failed(&on(&other.0, &s.app.0), &["rename", "2026-11-20", "Autre"], 3);
    let theirs = s.folder.0.canonicalize().unwrap();
    assert!(o.err.starts_with(&format!("greffier: Greffier runs on another meetings folder: {}", theirs.display())), "{}", o.err);
    assert!(other.0.join(RELEASE).exists() && s.folder.0.join(RELEASE).exists(), "nothing renamed");
}

#[test]
fn nobody_answering_exits_with_3() {
    let f = fixtures();
    let app = Temp::new();
    let o = failed(&on(&f.0, &app.0), &["stop", "2026-11-20"], 3);
    let socket = greffier_cli::server::socket_path(&app.0);
    // Whatever else runs on this Mac: one message.
    assert_eq!(
        o.err,
        format!("greffier: Greffier does not answer on {}. If an older Greffier is open, quit or update it (cargo xtask install-app), then run the command again.\n", socket.display())
    );
    let mut cx = on(&f.0, &app.0);
    cx.process = None;
    assert_eq!(failed(&cx, &["stop", "2026-11-20"], 3).err, "greffier: Greffier cannot be reached from here.\n");
}

#[test]
fn an_app_folder_too_deep_for_a_socket_fails_at_once() {
    let f = fixtures();
    let app = Temp::new();
    let deep = app.0.join("x".repeat(120));
    let mut cx = on(&f.0, &deep);
    // Nothing must be launched: this program does not exist.
    cx.process.as_mut().unwrap().launch = Launch::Program { program: PathBuf::from("/nulle/part/greffier"), env: vec![] };
    cx.process.as_mut().unwrap().patience = Duration::from_secs(10);
    let start = std::time::Instant::now();
    let o = failed(&cx, &["stop", "2026-11-20"], 3);
    assert!(start.elapsed() < Duration::from_secs(2), "at once");
    let socket = greffier_cli::server::socket_path(&deep);
    assert_eq!(
        o.err,
        format!(
            "greffier: Greffier's socket path is too long ({} bytes, {} at most): {}. Choose a shorter app folder (GREFFIER_APP_FOLDER).\n",
            socket.as_os_str().len(),
            greffier_cli::server::MAX_SOCKET_PATH,
            socket.display()
        )
    );
}

/// A process that greets, then takes its time over everything else (an import converting a
/// long media): it reads the request and answers after `delay`.
fn busy_process(folder: &Path, app: &Path, delay: Duration) -> std::thread::JoinHandle<()> {
    use greffier_cli::protocol::{Hello, Response, VERSION, encode};
    use std::io::{BufRead, BufReader, Write};
    let listener = std::os::unix::net::UnixListener::bind(greffier_cli::server::socket_path(app)).expect("socket");
    let folder = folder.canonicalize().unwrap();
    std::thread::spawn(move || {
        let (stream, _) = listener.accept().expect("a connection");
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut writer = stream;
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        let hello = Response::Hello(Hello { version: VERSION, pid: 1, meetings_folder: Some(folder), background: true });
        writer.write_all(encode(&hello).as_bytes()).unwrap();
        line.clear();
        reader.read_line(&mut line).unwrap();
        std::thread::sleep(delay);
        let _ = writer.write_all(encode(&Response::error("too late")).as_bytes());
    })
}

#[test]
fn an_action_that_takes_long_may_still_complete() {
    let f = fixtures();
    let app = Temp::new();
    let busy = busy_process(&f.0, &app.0, Duration::from_millis(600));
    let mut cx = on(&f.0, &app.0);
    cx.process.as_mut().unwrap().timeouts.action = Duration::from_millis(200);
    let o = failed(&cx, &["rename", "2026-11-20", "Revue"], 3);
    assert_eq!(
        o.err,
        "greffier: Greffier did not answer within 0.2 s: it may still be at work, and the action may still complete. Check with greffier list or greffier status before running it again.\n"
    );
    busy.join().unwrap();
}

#[test]
fn an_import_has_minutes_to_answer_the_rest_seconds() {
    let t = Process::new(Path::new("/tmp/app"), Launch::Nothing).timeouts;
    assert!(t.import >= Duration::from_secs(600), "a long media is copied and converted before the answer");
    assert!(t.action > Duration::from_secs(60) && t.action < t.import);
    assert!(t.quick <= Duration::from_secs(30));
}

#[test]
fn a_launch_that_fails_says_why() {
    let f = fixtures();
    let app = Temp::new();
    let mut cx = on(&f.0, &app.0);
    cx.process.as_mut().unwrap().launch = Launch::Program { program: PathBuf::from("/nulle/part/greffier"), env: vec![] };
    let o = failed(&cx, &["stop", "2026-11-20"], 3);
    assert!(o.err.starts_with("greffier: could not launch /nulle/part/greffier: "), "{}", o.err);
}

#[test]
fn import_prints_the_identifier_then_waits_for_the_end() {
    let s = serve(fixtures(), adapters, 50.0);
    let (_outside, file) = media();
    let o = run_in(&cx(&s), &["import", file.to_str().unwrap(), "--title", "Appel client", "--wait", "--timeout", "30"]);
    assert_eq!(o.code, 0, "{}", o.err);
    let lines: Vec<&str> = o.out.lines().collect();
    assert_eq!(lines.len(), 2, "{}", o.out);
    let id = lines[0];
    assert!(id.len() >= 15 && id.as_bytes()[4] == b'-', "an identifier: {id}");
    assert_eq!(lines[1], format!("{id} · done · Point produit : maquettes bookmarks, staging"));
    for stage in ["Transcription", "Review", "Summary"] {
        assert!(o.err.contains(&format!("{id} · {stage}\n")), "{stage}: {}", o.err);
    }
    assert!(o.err.lines().count() <= 4, "one line per stage: {}", o.err);
    // The summary renamed the folder; the identifier stays.
    let listed = run_in(&Context { meetings_folder: Some(s.folder.0.clone()), app_folder: None, live: None, process: None }, &["show", id, "--summary"]);
    assert_eq!(listed.code, 0, "{}", listed.err);
}

#[test]
fn import_in_json_and_without_processing() {
    let s = serve(fixtures(), adapters, 0.0);
    let (_outside, file) = media();
    let out = ok(&cx(&s), &["import", file.to_str().unwrap(), "--no-process", "--title", "Point client", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert!(v["folder"].as_str().unwrap().ends_with(" - Point client"), "{v}");
    let id = v["identifier"].as_str().unwrap();
    assert_eq!(v["message"], format!("{id} · imported"));
    assert!(Path::new(v["folder"].as_str().unwrap()).is_dir());
}

#[test]
fn import_takes_paths_relative_to_where_it_runs() {
    let s = serve(fixtures(), adapters, 0.0);
    let o = failed(&cx(&s), &["import", "nulle-part.mp4"], 1);
    assert_eq!(o.err, "greffier: file not found: nulle-part.mp4\n");
}

#[test]
fn a_failed_processing_exits_with_1_and_its_stage() {
    let s = serve(fixtures(), failing_adapters, 50.0);
    let o = run_in(&cx(&s), &["process", "2026-11-19", "--wait"]);
    assert_eq!(o.code, 1, "{} {}", o.out, o.err);
    assert!(o.err.ends_with("greffier: 2026-11-19-1530 · failed: Transcription: whisper a planté\n"), "{}", o.err);
}

#[test]
fn the_timeout_exits_with_4_while_still_processing() {
    // Time does not move: the review starts and stays at 0 %.
    let s = serve(fixtures(), adapters, 0.0);
    let o = run_in(&cx(&s), &["redo", "2026-11-20", "review", "--wait", "--timeout", "0.2"]);
    assert_eq!(o.code, 4, "{} {}", o.out, o.err);
    assert_eq!(o.err, "2026-11-20-1000 · Review\ngreffier: 2026-11-20-1000 still processing after 0.2 s: Review 0 %\n");
}

#[test]
fn status_shows_the_live_progress_of_a_running_process() {
    // Slow enough to be seen running.
    let s = serve(fixtures(), adapters, 0.05);
    let cx = cx(&s);
    ok(&cx, &["process", "2026-11-19"]);
    ok(&cx, &["redo", "2026-11-20", "summary"]);
    std::thread::sleep(Duration::from_millis(100));
    let out = ok(&cx, &["status"]);
    let lines: Vec<&str> = out.lines().collect();
    assert!(lines[0].starts_with("2026-11-19-1530 · Transcription ") && lines[0].contains(" % · "), "{out}");
    assert!(lines[1].starts_with("2026-11-20-1000 · 2nd in queue · "), "{out}");
    let v: serde_json::Value = serde_json::from_str(&ok(&cx, &["status", "2026-11-19", "--json"])).unwrap();
    assert_eq!(v["status"], "running");
    assert_eq!(v["rank"], 1);
    assert!(v["progress"].as_f64().is_some() && v["time_remaining"].as_u64().is_some(), "{v}");
}

#[test]
fn status_reads_the_folder_when_nobody_answers() {
    let f = fixtures();
    let app = Temp::new();
    let out = ok(&on(&f.0, &app.0), &["status"]);
    assert!(out.contains("2026-11-19-1530 · to process"), "{out}");
}

#[test]
fn the_launch_follows_where_the_command_line_runs() {
    let program = PathBuf::from("/tmp/test-server");
    assert_eq!(Launch::rule(Some(program.clone()), vec![], None), Launch::Program { program, env: vec![] });
    let bundled = PathBuf::from("/Applications/Greffier.app/Contents/MacOS/greffier");
    assert_eq!(Launch::rule(None, vec![], Some(bundled.clone())), Launch::Bundle { id: BUNDLE_ID.into(), env: vec![] });
    let built = PathBuf::from("/Users/a/greffier/target/debug/greffier");
    assert_eq!(Launch::rule(None, vec![], Some(built.clone())), Launch::Program { program: built.clone(), env: vec![] });
    assert_eq!(BUNDLE_ID, "io.github.fredealexandre.greffier");
    // A replaced folder outside a bundle: the binary, which inherits the variables.
    let trial = vec![("GREFFIER_FOLDER".to_string(), OsString::from("/tmp/essai"))];
    assert_eq!(Launch::rule(None, trial.clone(), Some(built.clone())), Launch::Program { program: built, env: vec![] });
}

#[test]
fn a_trial_from_the_installed_app_keeps_the_bundle_and_passes_its_folders() {
    // Spawned directly, the binary would lose the bundle's macOS permissions.
    let bundled = PathBuf::from("/Applications/Greffier.app/Contents/MacOS/greffier");
    let trial = vec![("GREFFIER_FOLDER".to_string(), OsString::from("/tmp/essai")), ("GREFFIER_APP_FOLDER".to_string(), OsString::from("/tmp/app"))];
    let launch = Launch::rule(None, trial.clone(), Some(bundled));
    assert_eq!(launch, Launch::Bundle { id: BUNDLE_ID.into(), env: trial });
    let args: Vec<String> = launch.open_args().iter().map(|a| a.to_string_lossy().into_owned()).collect();
    assert_eq!(
        args,
        ["-n", "-g", "-j", "-b", BUNDLE_ID, "--env", "GREFFIER_FOLDER=/tmp/essai", "--env", "GREFFIER_APP_FOLDER=/tmp/app", "--args", "--background"],
        "a separate instance for that app folder"
    );
    let only_one = Launch::rule(None, vec![("GREFFIER_APP_FOLDER".to_string(), OsString::from("/tmp/app"))], Some(PathBuf::from("/Applications/Greffier.app/Contents/MacOS/greffier")));
    let args: Vec<String> = only_one.open_args().iter().map(|a| a.to_string_lossy().into_owned()).collect();
    assert_eq!(args, ["-n", "-g", "-j", "-b", BUNDLE_ID, "--env", "GREFFIER_APP_FOLDER=/tmp/app", "--args", "--background"]);
    let plain: Vec<String> = Launch::Bundle { id: BUNDLE_ID.into(), env: vec![] }.open_args().iter().map(|a| a.to_string_lossy().into_owned()).collect();
    assert_eq!(plain, ["-g", "-j", "-b", BUNDLE_ID, "--args", "--background"], "the one instance");
}

#[test]
fn a_second_launch_asks_the_first_one_to_show_its_window() {
    use greffier_cli::protocol::{Request, Response};
    let s = serve(fixtures(), adapters, 0.0);
    let socket = greffier_cli::server::socket_path(&s.app.0);
    assert!(matches!(greffier_cli::client::ask_once(&socket, &Request::OpenWindow), Some(Response::Ok(_))));
    let nobody = Temp::new();
    assert_eq!(greffier_cli::client::ask_once(&greffier_cli::server::socket_path(&nobody.0), &Request::OpenWindow), None);
}
