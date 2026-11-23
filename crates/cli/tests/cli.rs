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
    run_in(&Context { meetings_folder: Some(folder.0.clone()), live: None }, args)
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
        for command in ["greffier list", "greffier show", "greffier status", "--json"] {
            assert!(o.out.contains(command), "{command}");
        }
        assert!(!o.out.contains("search") && !o.out.contains("à venir"), "only what works");
    }
}

#[test]
fn usage_errors_exit_with_2() {
    let f = meetings();
    for (args, message) in [
        (&["list", "--frobnicate"][..], "option inconnue : --frobnicate"),
        (&["list", "en-trop"], "argument en trop : « en-trop »"),
        (&["list", "--limit"], "--limit attend une valeur"),
        (&["list", "--limit", "zéro"], "--limit attend un nombre"),
        (&["list", "--date", "20/11/2026"], "date invalide : « 20/11/2026 »"),
        (&["show"], "il manque la réunion"),
        (&["show", "2026-11-20", "--bilan", "--fiche"], "une seule vue à la fois"),
        (&["show", "2026-11-20", "--from", "0:10"], "--from et --to vont avec --transcription ou --passages"),
        (&["show", "2026-11-20", "--transcription", "--from", "dix"], "instant invalide : « dix »"),
        (&["status", "a", "b"], "argument en trop : « b »"),
        (&["lsit"], "commande inconnue : lsit"),
        (&["search", "release"], "search n'est pas encore disponible"),
    ] {
        let o = greffier(&f, args);
        assert_eq!(o.code, 2, "{args:?}");
        assert!(o.out.is_empty(), "{args:?}");
        assert!(o.err.starts_with("greffier : ") && o.err.contains(message), "{args:?}: {}", o.err);
    }
}

#[test]
fn without_a_meetings_folder_it_says_how_to_choose_one() {
    let o = run_in(&Context { meetings_folder: None, live: None }, &["list"]);
    assert_eq!(o.code, 1);
    assert_eq!(o.err, "greffier : aucun dossier de réunions : ouvrez Greffier une fois pour le choisir, ou définissez GREFFIER_FOLDER.\n");
    let o = run_in(&Context { meetings_folder: Some(PathBuf::from("/nulle/part")), live: None }, &["list"]);
    assert_eq!(o.code, 1);
    assert_eq!(o.err, "greffier : dossier de réunions introuvable : /nulle/part\n");
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
        "2026-11-22-0800 · 0:10:00 · enregistrement interrompu · Démo\n\
         2026-11-21-0939-2 · 0:30:00 · en attente · Rétro\n\
         2026-11-21-0939 · 0:45:00 · échec : Relecture IA · Point d'équipe\n\
         2026-11-20-1000 · 0:00:12 · terminée, 1 à vérifier · Revue de release\n\
         2026-11-19-1530 · 0:00:12 · à traiter · Réunion\n\
         2026-11-18-0915 · 0:00:12 · à traiter · Réunion\n"
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
    assert_eq!(greffier(&f, &["list", "--date", "2025-01-01"]).out, "Aucune réunion.\n");
}

#[test]
fn list_in_json_mirrors_the_sheet() {
    let f = meetings();
    let v = greffier(&f, &["list", "--json"]).json();
    let all = v.as_array().expect("an array");
    assert_eq!(all.len(), 6);
    let failed = &all[2];
    assert_eq!(failed["id"], "2026-11-21-0939");
    assert_eq!(failed["titre"], "Point d'équipe");
    assert_eq!(failed["date"], "2026-11-21");
    assert_eq!(failed["debut"], "09:39");
    assert_eq!(failed["duree"], 2700);
    assert_eq!(failed["duree_texte"], "0:45:00");
    assert_eq!(failed["statut"], "echec");
    assert_eq!(failed["etape"], "relecture");
    assert_eq!(failed["etape_faite"], "transcription");
    assert_eq!(failed["erreur"], "La CLI claude a échoué.");
    assert_eq!(failed["a_verifier"], 0);
    assert_eq!(failed["dossier"], f.0.join("2026-11-21 09h39 - Point d'équipe").to_string_lossy().as_ref());
    let done = &all[3];
    assert_eq!((done["statut"].as_str(), done["etape"].is_null(), done["erreur"].is_null(), done["a_verifier"].as_u64()), (Some("terminee"), true, true, Some(1)));
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
                 2026-11-20 10h00 · 0:00:12 · terminée, 1 à vérifier\n\
                 Locuteurs : Alexandre, Khalid ?\n\
                 Dossier : {}\n\
                 En bref : On publie la release lundi si la recette passe vendredi.\n\
                 Sujets :\n  [0:00:00] Date de la release\n  [0:00:05] Support\n\
                 Décisions :\n  [0:00:02] Release lundi si la recette passe vendredi.\n\
                 Actions : 2\n\
                 Suite : --bilan (bilan complet), --actions, --transcription --from h:mm:ss --to h:mm:ss (un passage)\n",
                f.0.join(RELEASE).display()
            ),
            "{name}"
        );
    }
}

#[test]
fn the_outline_says_when_there_is_no_decision() {
    let f = meetings();
    let file = f.0.join(RELEASE).join(format!("{RELEASE} - bilan.md"));
    let summary = std::fs::read_to_string(&file).unwrap().replace("- (0:00:02) Release lundi si la recette passe vendredi.", "- Aucune décision.");
    std::fs::write(&file, summary).unwrap();
    let o = greffier(&f, &["show", "2026-11-20-1000"]);
    assert!(o.out.contains("\nDécisions : aucune\nActions : 2\n"), "{}", o.out);
}

#[test]
fn show_without_a_summary_says_where_the_meeting_stands() {
    let f = meetings();
    let o = greffier(&f, &["show", "2026-11-21-0939"]);
    assert_eq!(o.code, 0, "{}", o.err);
    assert_eq!(
        o.out,
        format!(
            "2026-11-21-0939 · Point d'équipe\n2026-11-21 09h39 · 0:45:00 · échec : Relecture IA\nErreur : La CLI claude a échoué.\nDossier : {}\nPas encore de bilan (étape faite : Transcription).\n",
            f.0.join("2026-11-21 09h39 - Point d'équipe").display()
        )
    );
    let o = greffier(&f, &["show", "2026-11-19"]);
    assert!(o.out.ends_with("Pas encore de bilan (aucune étape faite).\n"), "{}", o.out);
    let o = greffier(&f, &["show", "2026-11-18"]);
    assert!(o.out.ends_with("Pas encore de bilan (étape faite : Transcription).\nSuite : --transcription [--from h:mm:ss --to h:mm:ss]\n"), "{}", o.out);
}

#[test]
fn an_unknown_or_ambiguous_meeting_fails_with_the_candidates() {
    let f = meetings();
    let o = greffier(&f, &["show", "2026-12-01"]);
    assert_eq!((o.code, o.err.as_str()), (1, "greffier : réunion introuvable : « 2026-12-01 »\n"));
    let o = greffier(&f, &["show", "2026-11-21"]);
    assert_eq!(o.code, 1);
    assert_eq!(o.err, "greffier : plusieurs réunions pour « 2026-11-21 » :\n2026-11-21-0939-2 · Rétro\n2026-11-21-0939 · Point d'équipe\n");
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
    assert_eq!(o.err, "greffier : plusieurs réunions pour « 2026-11-23 » :\n2026-11-23-1400 · Démo : bookmarks\n2026-11-23-0932 · Revue : permissions\n", "the sheet's titles, as in list");
    let o = greffier(&f, &["show", "2026-11-23-0932"]);
    assert!(o.out.contains("\nLocuteurs : Jérôme ? (SPEAKER_00), Jérôme ? (SPEAKER_01), Alexandre\n"), "{}", o.out);
}

#[test]
fn show_bilan_and_fiche_give_the_files() {
    let f = meetings();
    let o = greffier(&f, &["show", "2026-11-20-1000", "--bilan"]);
    assert_eq!(o.out, std::fs::read_to_string(f.0.join(RELEASE).join(format!("{RELEASE} - bilan.md"))).unwrap());
    let o = greffier(&f, &["show", "--fiche", "2026-11-20-1000"]);
    assert_eq!(o.out, std::fs::read_to_string(f.0.join(RELEASE).join(format!("{RELEASE} - réunion.md"))).unwrap());
    let o = greffier(&f, &["show", "2026-11-21-0939", "--bilan"]);
    assert_eq!((o.code, o.err.as_str()), (1, "greffier : pas encore de bilan pour 2026-11-21-0939 (étape faite : Transcription)\n"));
    let o = greffier(&f, &["show", "2026-11-19", "--fiche"]);
    assert_eq!((o.code, o.err.as_str()), (1, "greffier : pas de fiche pour 2026-11-19-1530\n"));
}

#[test]
fn the_summary_is_shown_without_its_front_matter() {
    let f = meetings();
    let file = f.0.join(RELEASE).join(format!("{RELEASE} - bilan.md"));
    let summary = std::fs::read_to_string(&file).unwrap();
    std::fs::write(&file, format!("---\ntype: bilan\n---\n\n{summary}")).unwrap();
    assert_eq!(greffier(&f, &["show", "2026-11-20-1000", "--bilan"]).out, summary);
    assert!(greffier(&f, &["show", "2026-11-20-1000"]).out.contains("Dossier : ") && !greffier(&f, &["show", "2026-11-20-1000"]).out.contains("type: bilan"));
}

#[test]
fn show_transcription_uses_the_displayed_names() {
    let f = meetings();
    let o = greffier(&f, &["show", "2026-11-20-1000", "--transcription"]);
    assert_eq!(
        o.out,
        "[0:00:00] Alexandre : Bonjour Khalid, on publie la release lundi ?\n\
         [0:00:02] Khalid ? : Oui, si la recette passe vendredi.\n\
         [0:00:05] Alexandre : Je préviens le support demain.\n"
    );
    let o = greffier(&f, &["show", "2026-11-20-1000", "--transcription", "--numeros", "--from", "0:03", "--to", "6"]);
    assert_eq!(o.out, "#2 [0:00:02] Khalid ? : Oui, si la recette passe vendredi.\n#3 [0:00:05] Alexandre : Je préviens le support demain.\n");
    let o = greffier(&f, &["show", "2026-11-20-1000", "--transcription", "--from", "0:00:10"]);
    assert_eq!(o.out, "Aucune réplique dans cet intervalle.\n");
    let o = greffier(&f, &["show", "2026-11-19", "--transcription"]);
    assert_eq!((o.code, o.err.as_str()), (1, "greffier : pas encore de transcription pour 2026-11-19-1530\n"));
}

#[test]
fn show_transcription_in_json() {
    let f = meetings();
    let v = greffier(&f, &["show", "2026-11-20-1000", "--transcription", "--json", "--from", "2.5"]).json();
    assert_eq!(v["id"], "2026-11-20-1000");
    let r = v["repliques"].as_array().unwrap();
    assert_eq!(r.len(), 2);
    assert_eq!(r[0]["numero"], 2);
    assert_eq!(r[0]["debut"], 2.0);
    assert_eq!(r[0]["fin"], 3.5);
    assert_eq!(r[0]["horodatage"], "0:00:02");
    assert_eq!(r[0]["locuteur"], "Khalid ?");
    assert_eq!(r[0]["label"], "SPEAKER_01");
    assert_eq!(r[0]["texte"], "Oui, si la recette passe vendredi.");
}

#[test]
fn show_speakers_actions_and_passages() {
    let f = meetings();
    let o = greffier(&f, &["show", "2026-11-20-1000", "--locuteurs"]);
    assert_eq!(
        o.out,
        "0 · SPEAKER_00 · Alexandre · 0:00:03 · 68 %\n\
         1 · SPEAKER_01 · Khalid ? · 0:00:01 · 32 %\n\
         2 · SPEAKER_02 · Alexandre · fusionné dans SPEAKER_00\n"
    );
    let o = greffier(&f, &["show", "2026-11-20-1000", "--actions"]);
    assert_eq!(o.out, "#1 [ ] Alexandre — publier la release — lundi\n#2 [x] Khalid — lancer la recette — vendredi\n");
    let o = greffier(&f, &["show", "2026-11-20-1000", "--passages"]);
    assert_eq!(o.out, "#1 [0:00:05] à vérifier : « le support » (hypothèse : « l'équipe support ») · réplique #3\n");
    let o = greffier(&f, &["show", "2026-11-20-1000", "--passages", "--to", "0:00:04"]);
    assert_eq!(o.out, "Aucun passage à vérifier dans cet intervalle.\n");
}

#[test]
fn show_views_in_json() {
    let f = meetings();
    let v = greffier(&f, &["show", "2026-11-20-1000", "--json"]).json();
    assert_eq!(v["id"], "2026-11-20-1000");
    assert_eq!(v["statut"], "terminee");
    assert!(v.get("bilan").is_none(), "the whole summary is --bilan's");
    assert_eq!(v["en_bref"], "On publie la release lundi si la recette passe vendredi.");
    assert_eq!(v["sujets"][1], serde_json::json!({ "debut": 5.0, "horodatage": "0:00:05", "titre": "Support" }));
    assert_eq!(v["decisions"][0], serde_json::json!({ "debut": 2.0, "horodatage": "0:00:02", "texte": "Release lundi si la recette passe vendredi." }));
    assert_eq!(v["actions"], 2);
    let speakers = v["locuteurs"].as_array().unwrap();
    assert_eq!(speakers.len(), 3);
    assert_eq!(speakers[1]["label"], "SPEAKER_01");
    assert_eq!(speakers[1]["nom"], Value::Null);
    assert_eq!(speakers[1]["proposition"], "Khalid");
    assert_eq!(speakers[1]["affiche"], "Khalid ?");
    assert_eq!(speakers[2]["fusionne_dans"], "SPEAKER_00");
    assert_eq!(speakers[0]["temps_de_parole"], 3);
    let v = greffier(&f, &["show", "2026-11-21-0939", "--json"]).json();
    assert_eq!((v["en_bref"].is_null(), v["sujets"].is_null(), v["decisions"].is_null(), v["actions"].is_null()), (true, true, true, true), "no summary yet");

    let v = greffier(&f, &["show", "2026-11-20-1000", "--actions", "--json"]).json();
    assert_eq!(v["actions"][1]["numero"], 2);
    assert_eq!(v["actions"][1]["fait"], true);
    assert_eq!(v["actions"][1]["texte"], "Khalid — lancer la recette — vendredi");

    let v = greffier(&f, &["show", "2026-11-20-1000", "--passages", "--json"]).json();
    let p = &v["passages"][0];
    assert_eq!((p["numero"].as_u64(), p["replique"].as_u64(), p["debut"].as_f64()), (Some(1), Some(3), Some(5.0)));
    assert_eq!((p["statut"].as_str(), p["extrait"].as_str(), p["hypothese"].as_str()), (Some("a_verifier"), Some("le support"), Some("l'équipe support")));
    assert!(p["correction"].is_null());

    let v = greffier(&f, &["show", "2026-11-20-1000", "--bilan", "--json"]).json();
    assert!(v["bilan"].as_str().unwrap().starts_with("## En bref"));
    let v = greffier(&f, &["show", "2026-11-20-1000", "--fiche", "--json"]).json();
    assert!(v["fiche"].as_str().unwrap().starts_with("---\ntitre: \"Revue de release\""));
    let v = greffier(&f, &["show", "2026-11-20-1000", "--locuteurs", "--json"]).json();
    assert_eq!(v["locuteurs"][0]["nom"], "Alexandre");
}

// --- status ---

#[test]
fn status_lists_the_meetings_that_are_not_done() {
    let f = meetings();
    let o = greffier(&f, &["status"]);
    assert_eq!(o.code, 0, "{}", o.err);
    assert_eq!(
        o.out,
        "2026-11-21-0939-2 · en attente · Rétro\n\
         2026-11-21-0939 · échec : Relecture IA · Point d'équipe · La CLI claude a échoué.\n\
         2026-11-22-0800 · enregistrement interrompu · Démo\n\
         2026-11-19-1530 · à traiter · Réunion\n\
         2026-11-18-0915 · à traiter · Réunion\n"
    );
    let empty = temp_folder();
    assert_eq!(greffier(&empty, &["status"]).out, "Rien en cours.\n");
    assert_eq!(greffier(&empty, &["status", "--json"]).out, "[]\n");
    assert_eq!(greffier(&empty, &["list"]).out, "Aucune réunion.\n");
}

#[test]
fn status_of_a_meeting() {
    let f = meetings();
    let o = greffier(&f, &["status", "2026-11-21-0939"]);
    assert_eq!(o.out, "2026-11-21-0939 · Point d'équipe\nStatut : échec : Relecture IA\nÉtape faite : Transcription\nErreur : La CLI claude a échoué.\n");
    let o = greffier(&f, &["status", "2026-11-20-1000"]);
    assert_eq!(o.out, "2026-11-20-1000 · Revue de release\nStatut : terminée, 1 à vérifier\nÉtape faite : Bilan IA\n");
    let v = greffier(&f, &["status", "2026-11-21-0939", "--json"]).json();
    assert_eq!((v["id"].as_str(), v["statut"].as_str(), v["etape"].as_str(), v["etape_faite"].as_str()), (Some("2026-11-21-0939"), Some("echec"), Some("relecture"), Some("transcription")));
    assert_eq!(v["erreur"], "La CLI claude a échoué.");
    for live in ["rang", "progression", "temps_restant"] {
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
    let cx = Context { meetings_folder: Some(f.0.clone()), live: Some(Box::new(Queue)) };
    let o = run_in(&cx, &["status"]);
    assert!(o.out.starts_with("2026-11-19-1530 · Transcription 40 % · 7 min · Réunion\n2026-11-21-0939-2 · 2ᵉ dans la file · 15 min · Rétro\n"), "{}", o.out);
    let v = run_in(&cx, &["status", "2026-11-19", "--json"]).json();
    assert_eq!((v["statut"].as_str(), v["etape"].as_str(), v["rang"].as_u64(), v["progression"].as_f64(), v["temps_restant"].as_u64()), (Some("en_cours"), Some("transcription"), Some(1), Some(0.4), Some(420)));
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
        &["show", "2026-11-20-1000", "--locuteurs"],
        &["show", "2026-11-20-1000", "--actions"],
        &["show", "2026-11-20-1000", "--passages"],
        &["show", "2026-11-20-1000", "--fiche"],
        &["status"],
        &["status", "2026-11-18"],
    ] {
        assert_eq!(greffier(&f, args).code, 0, "{args:?}");
    }
    assert_eq!(snapshot(&f.0), before);
}
