//! The commands that write, from the command line to a Greffier process running in a thread of
//! the test (a core on fake adapters, a real socket): what each sends, what it prints, its exit
//! code; reaching the process, and refusing one on another meetings folder.

mod common;

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
    assert_eq!(ok(&cx, &["rename", "2026-11-20", "Revue", "de", "la", "1.4"]), "2026-11-20-1000 · renommée : Revue de la 1.4\n");
    assert!(s.folder.0.join("2026-11-20 10h00 - Revue de la 1.4").is_dir());
    assert!(!s.folder.0.join(RELEASE).exists());
}

#[test]
fn every_command_that_writes_says_what_it_did_in_one_line() {
    let s = serve(fixtures(), adapters, 0.0);
    let cx = cx(&s);
    for (args, said) in [
        (&["speaker", "name", "2026-11-20", "1", "Khalid", "B."][..], "2026-11-20-1000 · SPEAKER_01 : Khalid B.\n"),
        (&["speaker", "merge", "2026-11-20", "SPEAKER_01", "0"], "2026-11-20-1000 · SPEAKER_01 fusionné dans SPEAKER_00\n"),
        (&["speaker", "split", "2026-11-20", "SPEAKER_01"], "2026-11-20-1000 · SPEAKER_01 séparé\n"),
        (&["correct", "2026-11-20", "#2", "Oui,", "si", "la", "recette", "passe", "jeudi."], "2026-11-20-1000 · réplique #2 corrigée\n"),
        (&["resolve", "2026-11-20", "1", "corriger", "l'équipe", "support"], "2026-11-20-1000 · passage #1 : corrigé\n"),
        (&["resolve", "2026-11-20", "#1", "rouvrir"], "2026-11-20-1000 · passage #1 : rouvert\n"),
        (&["resolve", "2026-11-20", "1", "accepter"], "2026-11-20-1000 · passage #1 : hypothèse acceptée\n"),
        (&["resolve", "2026-11-20", "1", "garder"], "2026-11-20-1000 · passage #1 : original gardé\n"),
        (&["check-action", "2026-11-20", "1"], "2026-11-20-1000 · action #1 cochée\n"),
        (&["check-action", "2026-11-20", "1", "--undo"], "2026-11-20-1000 · action #1 décochée\n"),
        (&["redo", "2026-11-20", "bilan"], "2026-11-20-1000 · dans la file : Bilan IA\n"),
        (&["stop", "2026-11-20"], "2026-11-20-1000 · traitement arrêté\n"),
        (&["process", "2026-11-19", "--speakers", "4"], "2026-11-19-1530 · dans la file : Transcription\n"),
        (&["delete", "2026-11-18", "--yes"], "2026-11-18-0915 · mise à la corbeille\n"),
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
fn json_gives_the_identifier_and_the_folder() {
    let s = serve(fixtures(), adapters, 0.0);
    let out = ok(&cx(&s), &["rename", "2026-11-20", "Revue", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["id"], "2026-11-20-1000");
    assert_eq!(v["dossier"], s.folder.0.join("2026-11-20 10h00 - Revue").to_string_lossy().as_ref());
    assert_eq!(v["message"], "2026-11-20-1000 · renommée : Revue");
}

#[test]
fn what_the_process_refuses_exits_with_1() {
    let s = serve(fixtures(), adapters, 0.0);
    let cx = cx(&s);
    for (args, message) in [
        (&["rename", "2025", "Titre"][..], "greffier : réunion introuvable : « 2025 »\n"),
        (&["speaker", "name", "2026-11-20", "Bob", "X"], "greffier : locuteur introuvable : « Bob » (index, label SPEAKER_00 ou nom affiché)\n"),
        (&["stop", "2026-11-20"], "greffier : la réunion n'est pas en traitement\n"),
        (&["check-action", "2026-11-20", "9"], "greffier : action introuvable : « 9 » (numéro de 1 à 2)\n"),
    ] {
        assert_eq!(failed(&cx, args, 1).err, message, "{args:?}");
    }
}

#[test]
fn usage_errors_exit_with_2_and_send_nothing() {
    // No process anywhere: anything sent would exit with 3.
    let f = fixtures();
    let app = Temp::new();
    let cx = on(&f.0, &app.0);
    for (args, message) in [
        (&["delete", "2026-11-20"][..], "greffier delete met la réunion à la corbeille : confirmez avec greffier delete 2026-11-20 --yes"),
        (&["redo", "2026-11-20", "résumé"], "étape inconnue : « résumé » (transcription, relecture ou bilan)"),
        (&["redo", "2026-11-20"], "il manque l'étape (transcription, relecture ou bilan)"),
        (&["rename", "2026-11-20"], "il manque le titre"),
        (&["rename"], "il manque la réunion"),
        (&["speaker", "rename", "2026-11-20", "0"], "greffier speaker rename : name, merge ou split"),
        (&["speaker", "merge", "2026-11-20", "0"], "il manque le locuteur dans lequel fusionner"),
        (&["resolve", "2026-11-20", "1", "corriger"], "il manque le texte de la correction"),
        (&["resolve", "2026-11-20", "1", "accepter", "oui"], "argument en trop : « oui »"),
        (&["resolve", "2026-11-20", "1", "valider"], "« valider » : accepter, garder, rouvrir ou corriger <texte>"),
        (&["process", "2026-11-19", "--speakers", "0"], "--speakers attend un nombre de locuteurs : « 0 »"),
        (&["process", "2026-11-19", "--timeout", "5"], "--timeout va avec --wait"),
        (&["process", "2026-11-19", "--wait", "--timeout", "bientôt"], "--timeout attend des secondes : « bientôt »"),
        (&["stop", "2026-11-19", "--wait"], "option inconnue : --wait"),
        (&["import"], "il manque le fichier à importer"),
        (&["import", "x.mp4", "--no-process", "--wait"], "--wait ne va pas avec --no-process"),
        (&["check-action", "2026-11-20"], "il manque le numéro de l'action"),
    ] {
        assert_eq!(failed(&cx, args, 2).err, format!("greffier : {message} (voir greffier help)\n"), "{args:?}");
    }
}

#[test]
fn help_lists_the_commands_that_write() {
    let s = serve(fixtures(), adapters, 0.0);
    let out = ok(&cx(&s), &["delete", "--help"]);
    for command in ["greffier import", "greffier process", "greffier redo", "greffier stop", "greffier rename", "greffier speaker", "greffier correct", "greffier resolve", "greffier check-action", "greffier delete", "3 Greffier injoignable"] {
        assert!(out.contains(command), "{command}");
    }
}

#[test]
fn a_process_on_another_meetings_folder_is_refused() {
    let s = serve(fixtures(), adapters, 0.0);
    let other = fixtures();
    let o = failed(&on(&other.0, &s.app.0), &["rename", "2026-11-20", "Autre"], 3);
    let theirs = s.folder.0.canonicalize().unwrap();
    assert!(o.err.starts_with(&format!("greffier : Greffier tourne sur un autre dossier de réunions : {}", theirs.display())), "{}", o.err);
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
        format!("greffier : Greffier ne répond pas sur {}. Si un Greffier plus ancien est ouvert, quittez-le ou mettez-le à jour (cargo xtask install-app), puis relancez la commande.\n", socket.display())
    );
    let mut cx = on(&f.0, &app.0);
    cx.process = None;
    assert_eq!(failed(&cx, &["stop", "2026-11-20"], 3).err, "greffier : Greffier n'est pas joignable d'ici.\n");
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
            "greffier : le chemin du socket de Greffier est trop long ({} octets, {} au plus) : {}. Choisissez un dossier de l'app plus court (GREFFIER_APP_FOLDER).\n",
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
        let _ = writer.write_all(encode(&Response::error("trop tard")).as_bytes());
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
        "greffier : Greffier n'a pas répondu en 0.2 s : il travaille peut-être encore, et l'action peut aboutir. Vérifiez avec greffier list ou greffier status avant de la relancer.\n"
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
    assert!(o.err.starts_with("greffier : lancement de /nulle/part/greffier : "), "{}", o.err);
}

#[test]
fn import_prints_the_identifier_then_waits_for_the_end() {
    let s = serve(fixtures(), adapters, 50.0);
    let (_outside, file) = media();
    let o = run_in(&cx(&s), &["import", file.to_str().unwrap(), "--titre", "Appel client", "--wait", "--timeout", "30"]);
    assert_eq!(o.code, 0, "{}", o.err);
    let lines: Vec<&str> = o.out.lines().collect();
    assert_eq!(lines.len(), 2, "{}", o.out);
    let id = lines[0];
    assert!(id.len() >= 15 && id.as_bytes()[4] == b'-', "an identifier: {id}");
    assert_eq!(lines[1], format!("{id} · terminée · Point produit : maquettes bookmarks, staging"));
    for stage in ["Transcription", "Relecture IA", "Bilan IA"] {
        assert!(o.err.contains(&format!("{id} · {stage}\n")), "{stage}: {}", o.err);
    }
    assert!(o.err.lines().count() <= 4, "one line per stage: {}", o.err);
    // The summary renamed the folder; the identifier stays.
    let listed = run_in(&Context { meetings_folder: Some(s.folder.0.clone()), app_folder: None, live: None, process: None }, &["show", id, "--bilan"]);
    assert_eq!(listed.code, 0, "{}", listed.err);
}

#[test]
fn import_in_json_and_without_processing() {
    let s = serve(fixtures(), adapters, 0.0);
    let (_outside, file) = media();
    let out = ok(&cx(&s), &["import", file.to_str().unwrap(), "--no-process", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let id = v["id"].as_str().unwrap();
    assert_eq!(v["message"], format!("{id} · importée"));
    assert!(Path::new(v["dossier"].as_str().unwrap()).is_dir());
}

#[test]
fn import_takes_paths_relative_to_where_it_runs() {
    let s = serve(fixtures(), adapters, 0.0);
    let o = failed(&cx(&s), &["import", "nulle-part.mp4"], 1);
    assert_eq!(o.err, "greffier : fichier introuvable : nulle-part.mp4\n");
}

#[test]
fn a_failed_processing_exits_with_1_and_its_stage() {
    let s = serve(fixtures(), failing_adapters, 50.0);
    let o = run_in(&cx(&s), &["process", "2026-11-19", "--wait"]);
    assert_eq!(o.code, 1, "{} {}", o.out, o.err);
    assert!(o.err.ends_with("greffier : 2026-11-19-1530 · échec : Transcription : whisper a planté\n"), "{}", o.err);
}

#[test]
fn the_timeout_exits_with_4_while_still_processing() {
    // Time does not move: the review starts and stays at 0 %.
    let s = serve(fixtures(), adapters, 0.0);
    let o = run_in(&cx(&s), &["redo", "2026-11-20", "relecture", "--wait", "--timeout", "0.2"]);
    assert_eq!(o.code, 4, "{} {}", o.out, o.err);
    assert_eq!(o.err, "2026-11-20-1000 · Relecture IA\ngreffier : 2026-11-20-1000 toujours en cours après 0.2 s : Relecture IA 0 %\n");
}

#[test]
fn status_shows_the_live_progress_of_a_running_process() {
    // Slow enough to be seen running.
    let s = serve(fixtures(), adapters, 0.05);
    let cx = cx(&s);
    ok(&cx, &["process", "2026-11-19"]);
    ok(&cx, &["redo", "2026-11-20", "bilan"]);
    std::thread::sleep(Duration::from_millis(100));
    let out = ok(&cx, &["status"]);
    let lines: Vec<&str> = out.lines().collect();
    assert!(lines[0].starts_with("2026-11-19-1530 · Transcription ") && lines[0].contains(" % · "), "{out}");
    assert!(lines[1].starts_with("2026-11-20-1000 · 2ᵉ dans la file · "), "{out}");
    let v: serde_json::Value = serde_json::from_str(&ok(&cx, &["status", "2026-11-19", "--json"])).unwrap();
    assert_eq!(v["statut"], "en_cours");
    assert_eq!(v["rang"], 1);
    assert!(v["progression"].as_f64().is_some() && v["temps_restant"].as_u64().is_some(), "{v}");
}

#[test]
fn status_reads_the_folder_when_nobody_answers() {
    let f = fixtures();
    let app = Temp::new();
    let out = ok(&on(&f.0, &app.0), &["status"]);
    assert!(out.contains("2026-11-19-1530 · à traiter"), "{out}");
}

#[test]
fn the_launch_follows_where_the_command_line_runs() {
    let program = PathBuf::from("/tmp/test-server");
    assert_eq!(Launch::rule(Some(program.clone()), false, None), Launch::Program { program, env: vec![] });
    let bundled = PathBuf::from("/Applications/Greffier.app/Contents/MacOS/greffier");
    assert_eq!(Launch::rule(None, false, Some(bundled.clone())), Launch::Bundle(BUNDLE_ID.into()));
    // A replaced folder: the binary itself, which keeps the variables.
    assert_eq!(Launch::rule(None, true, Some(bundled.clone())), Launch::Program { program: bundled, env: vec![] });
    let built = PathBuf::from("/Users/a/greffier/target/debug/greffier");
    assert_eq!(Launch::rule(None, false, Some(built.clone())), Launch::Program { program: built, env: vec![] });
    assert_eq!(BUNDLE_ID, "io.github.fredealexandre.greffier");
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
