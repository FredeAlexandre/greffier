//! The process's side of the protocol: each action turned into core commands on a live core
//! (fake adapters, a copy of the core's sample meetings), what the user typed resolved against
//! that core, and the responses.

mod common;

use std::path::PathBuf;

use common::{Temp, fixtures, media};
use greffier_cli::protocol::{Action, Done, LiveStatus, Request, Resolution, Response, Stage};
use greffier_cli::server::{self, Host};
use greffier_core::{Event, Greffier, MeetingId, PassageStatus, Status};

const RELEASE: &str = "2026-11-20 10h00 - Revue de release";

/// A host as the app will be: a core, and what the server hands back to it.
struct TestHost {
    core: Greffier,
    events: Vec<Event>,
    deleted: Vec<MeetingId>,
    windows: usize,
    _folder: Temp,
}

impl Host for TestHost {
    fn core(&mut self) -> &mut Greffier {
        &mut self.core
    }
    fn background(&self) -> bool {
        true
    }
    fn executed(&mut self, events: Vec<Event>) {
        self.events.extend(events);
    }
    fn deleted(&mut self, meeting: MeetingId) {
        self.deleted.push(meeting);
    }
    fn open_window(&mut self) {
        self.windows += 1;
    }
}

/// A core with fake adapters on a copy of the sample meetings: « Revue de release » (done,
/// three speakers, SPEAKER_02 merged into SPEAKER_00, three utterances, one passage to check,
/// two actions), and two meetings to process (2026-11-18 09h15, 2026-11-19 15h30).
fn host() -> TestHost {
    let folder = fixtures();
    let core = Greffier::open(&folder.0, greffier_core::demo::adapters());
    TestHost { core, events: vec![], deleted: vec![], windows: 0, _folder: folder }
}

impl TestHost {
    fn ask(&mut self, action: Action) -> Response {
        server::answer(self, Request::Execute(action))
    }
    fn ok(&mut self, action: Action) -> Done {
        match self.ask(action.clone()) {
            Response::Ok(done) => done,
            other => panic!("{action:?}: {other:?}"),
        }
    }
    fn refused(&mut self, action: Action) -> String {
        match self.ask(action.clone()) {
            Response::Error { message } => message,
            other => panic!("{action:?} should fail: {other:?}"),
        }
    }
    fn named(&self, folder_name: &str) -> &greffier_core::Meeting {
        self.core.meetings().iter().find(|m| m.folder_name == folder_name).unwrap_or_else(|| panic!("no {folder_name}"))
    }
    fn release(&self) -> &greffier_core::Meeting {
        self.core.meetings().iter().find(|m| m.title == "Revue de release" || m.folder_name.starts_with("2026-11-20 10h00")).expect("release")
    }
    fn folder(&self) -> PathBuf {
        self.core.folder().root().to_path_buf()
    }
}

fn s(x: &str) -> String {
    x.to_string()
}

// --- Hello, status, window ---

#[test]
fn hello_says_who_answers() {
    let mut h = host();
    let Response::Hello(hello) = server::answer(&mut h, Request::Hello) else { panic!("hello") };
    assert_eq!(hello.version, greffier_cli::protocol::VERSION);
    assert_eq!(hello.pid, std::process::id());
    assert_eq!(hello.meetings_folder, Some(h.folder().canonicalize().unwrap()));
    assert!(hello.background);
}

#[test]
fn open_window_goes_to_the_host() {
    let mut h = host();
    assert!(matches!(server::answer(&mut h, Request::OpenWindow), Response::Ok(_)));
    assert_eq!(h.windows, 1);
}

#[test]
fn status_gives_the_queue_with_ranks_progress_and_time_remaining() {
    let mut h = host();
    let Response::Status(idle) = server::answer(&mut h, Request::Status { meeting: None }) else { panic!() };
    assert!(!idle.busy);
    assert!(idle.meetings.is_empty());
    h.ok(Action::Process { meeting: s("2026-11-19"), speakers: None });
    h.ok(Action::Process { meeting: s("2026-11-18"), speakers: None });
    h.core.advance(1.0);
    let Response::Status(live) = server::answer(&mut h, Request::Status { meeting: None }) else { panic!() };
    assert!(live.busy);
    let ids: Vec<_> = live.meetings.iter().map(|m| m.id.clone().unwrap()).collect();
    assert_eq!(ids, ["2026-11-19-1530", "2026-11-18-0915"], "queue order");
    let (first, second) = (&live.meetings[0], &live.meetings[1]);
    assert!(matches!(first.status, LiveStatus::Running { stage: Stage::Transcription, progress } if progress > 0.0));
    assert_eq!((first.rank, second.rank), (Some(1), Some(2)));
    // Its transcription is done: it resumes at the review.
    assert_eq!(second.status, LiveStatus::Pending { stage: Stage::Review });
    assert!(second.time_remaining.unwrap() > first.time_remaining.unwrap());
    assert_eq!(first.folder_name, "2026-11-19 15h30 - Réunion");
}

#[test]
fn status_of_a_meeting_gives_it_even_out_of_the_queue() {
    let mut h = host();
    let Response::Status(snapshot) = server::answer(&mut h, Request::Status { meeting: Some(s("2026-11-20")) }) else { panic!() };
    assert_eq!(snapshot.meetings.len(), 1);
    assert_eq!(snapshot.meetings[0].status, LiveStatus::Done);
    assert_eq!(snapshot.meetings[0].title, "Revue de release");
    assert_eq!(snapshot.meetings[0].rank, None);
    let Response::Error { message } = server::answer(&mut h, Request::Status { meeting: Some(s("2025")) }) else { panic!() };
    assert_eq!(message, "réunion introuvable : « 2025 »");
}

// --- Meetings ---

#[test]
fn a_meeting_is_named_by_identifier_folder_name_or_date() {
    let mut h = host();
    for name in ["2026-11-20-1000", RELEASE, "2026-11-20", "2026-11-20-1"] {
        let done = h.ok(Action::Rename { meeting: s(name), title: s("Revue de release") });
        assert_eq!(done.meeting.as_deref(), Some("2026-11-20-1000"), "{name}");
    }
    assert_eq!(h.refused(Action::Stop { meeting: s("2026-12-01") }), "réunion introuvable : « 2026-12-01 »");
    let ambiguous = h.refused(Action::Stop { meeting: s("2026-11") });
    assert!(ambiguous.starts_with("plusieurs réunions pour « 2026-11 » :"), "{ambiguous}");
    assert!(ambiguous.contains("\n2026-11-20-1000 · Revue de release"), "titles of the live meetings: {ambiguous}");
}

/// Adds a meeting folder behind the core's back (the Finder, an agent), as the folder watcher has
/// not reported yet: a recording alone.
fn add_behind_the_core(h: &TestHost, folder_name: &str) {
    let dir = h.folder().join(folder_name);
    std::fs::create_dir_all(&dir).unwrap();
    let recording = h.folder().join("2026-11-19 15h30 - Réunion/2026-11-19 15h30 - Réunion - enregistrement.mp4");
    std::fs::copy(recording, dir.join(format!("{folder_name} - enregistrement.mp4"))).unwrap();
}

#[test]
fn a_meeting_is_named_against_the_folder_as_it_is_now() {
    let mut h = host();
    // « Alpha » comes before « Réunion » in the same minute: it takes 2026-11-19-1530, as `list`
    // shows it, and « Réunion » becomes 2026-11-19-1530-2.
    add_behind_the_core(&h, "2026-11-19 15h30 - Alpha");
    let done = h.ok(Action::Rename { meeting: s("2026-11-19-1530"), title: s("Renommée") });
    assert_eq!(done.folder_name.as_deref(), Some("2026-11-19 15h30 - Renommée"));
    assert!(!h.folder().join("2026-11-19 15h30 - Alpha").exists(), "Alpha was renamed");
    assert!(h.folder().join("2026-11-19 15h30 - Réunion").is_dir(), "the other meeting of that minute is left alone");
    assert!(h.events.iter().any(|e| matches!(e, Event::MeetingAppeared(_))), "the rescan's events go to the host: {:?}", h.events);
}

#[test]
fn the_status_reads_the_folder_as_it_is_now() {
    let mut h = host();
    add_behind_the_core(&h, "2026-11-19 15h30 - Alpha");
    let Response::Status(snapshot) = server::answer(&mut h, Request::Status { meeting: Some(s("2026-11-19-1530")) }) else { panic!() };
    assert_eq!(snapshot.meetings[0].folder_name, "2026-11-19 15h30 - Alpha");
}

#[test]
fn rename_renames_the_folder_and_gives_the_identifier() {
    let mut h = host();
    let done = h.ok(Action::Rename { meeting: s("2026-11-20"), title: s("Revue de la release 1.4") });
    assert_eq!(done.message, "2026-11-20-1000 · renommée : Revue de la release 1.4");
    assert_eq!(done.folder_name.as_deref(), Some("2026-11-20 10h00 - Revue de la release 1.4"));
    assert!(h.folder().join("2026-11-20 10h00 - Revue de la release 1.4").is_dir());
    assert!(!h.folder().join(RELEASE).exists());
    assert_eq!(h.refused(Action::Rename { meeting: s("2026-11-20"), title: s("  ") }), "le titre est vide");
}

// --- Processing ---

#[test]
fn process_queues_the_meeting_with_its_speaker_count() {
    let mut h = host();
    let done = h.ok(Action::Process { meeting: s("2026-11-19"), speakers: Some(3) });
    assert_eq!(done.message, "2026-11-19-1530 · dans la file : Transcription");
    let m = h.named("2026-11-19 15h30 - Réunion");
    assert_eq!(m.status, Status::Pending(greffier_core::Stage::Transcription));
    assert_eq!(m.speaker_count, Some(3));
    assert_eq!(h.refused(Action::Process { meeting: s("2026-11-20"), speakers: Some(2) }), "la réunion est déjà traitée (greffier redo pour refaire une étape)");
    assert_eq!(h.refused(Action::Process { meeting: s("2026-11-19"), speakers: None }), "la réunion est déjà dans la file");
    assert_eq!(h.release().speaker_count, Some(3), "a refused process changes no speaker count");
}

#[test]
fn redo_queues_from_the_stage() {
    let mut h = host();
    let done = h.ok(Action::Redo { meeting: s("2026-11-20"), stage: Stage::Review });
    assert_eq!(done.message, "2026-11-20-1000 · dans la file : Relecture IA");
    assert_eq!(h.release().status, Status::Pending(greffier_core::Stage::Review));
    assert_eq!(h.refused(Action::Redo { meeting: s("2026-11-20"), stage: Stage::Summary }), "la réunion est déjà dans la file");
    assert_eq!(h.refused(Action::Redo { meeting: s("2026-11-19"), stage: Stage::Summary }), "les étapes précédentes ne sont pas faites");
}

#[test]
fn stop_takes_the_meeting_out_of_the_queue() {
    let mut h = host();
    h.ok(Action::Process { meeting: s("2026-11-19"), speakers: None });
    h.core.advance(0.5);
    let done = h.ok(Action::Stop { meeting: s("2026-11-19") });
    assert_eq!(done.message, "2026-11-19-1530 · traitement arrêté");
    assert_eq!(h.named("2026-11-19 15h30 - Réunion").status, Status::ToProcess);
    assert!(!h.core.busy());
    assert_eq!(h.refused(Action::Stop { meeting: s("2026-11-19") }), "la réunion n'est pas en traitement");
}

// --- Speakers ---

#[test]
fn a_speaker_is_named_by_index_label_or_displayed_name() {
    let mut h = host();
    let done = h.ok(Action::NameSpeaker { meeting: s("2026-11-20"), speaker: s("1"), name: s("Khalid") });
    assert_eq!(done.message, "2026-11-20-1000 · SPEAKER_01 : Khalid");
    assert_eq!(h.release().speakers[1].name.as_deref(), Some("Khalid"));
    h.ok(Action::NameSpeaker { meeting: s("2026-11-20"), speaker: s("SPEAKER_01"), name: s("Jérôme") });
    assert_eq!(h.release().speakers[1].name.as_deref(), Some("Jérôme"));
    // Case and accents do not count.
    h.ok(Action::NameSpeaker { meeting: s("2026-11-20"), speaker: s("jerome"), name: s("Khalid") });
    assert_eq!(h.release().speakers[1].name.as_deref(), Some("Khalid"));
    // SPEAKER_02 is merged into SPEAKER_00: « Alexandre » is a single voice.
    h.ok(Action::NameSpeaker { meeting: s("2026-11-20"), speaker: s("ALEXANDRE"), name: s("Alex") });
    assert_eq!(h.release().speakers[0].name.as_deref(), Some("Alex"));
}

#[test]
fn a_proposed_name_names_its_speaker_without_its_question_mark() {
    let mut h = host();
    // SPEAKER_01 is displayed « Khalid ? ».
    h.ok(Action::NameSpeaker { meeting: s("2026-11-20"), speaker: s("Khalid"), name: s("Khalid") });
    assert_eq!(h.release().speakers[1].name.as_deref(), Some("Khalid"));
    h.ok(Action::NameSpeaker { meeting: s("2026-11-20"), speaker: s("Khalid ?"), name: s("Pablo") });
    assert_eq!(h.release().speakers[1].name.as_deref(), Some("Pablo"));
}

#[test]
fn an_unknown_or_ambiguous_speaker_is_refused() {
    let mut h = host();
    let unknown = h.refused(Action::NameSpeaker { meeting: s("2026-11-20"), speaker: s("Bob"), name: s("X") });
    assert_eq!(unknown, "locuteur introuvable : « Bob » (index, label SPEAKER_00 ou nom affiché)");
    assert!(h.refused(Action::NameSpeaker { meeting: s("2026-11-20"), speaker: s("7"), name: s("X") }).starts_with("locuteur introuvable : « 7 »"));
    h.ok(Action::SplitSpeaker { meeting: s("2026-11-20"), speaker: s("2") });
    h.ok(Action::NameSpeaker { meeting: s("2026-11-20"), speaker: s("2"), name: s("Alexandre") });
    let ambiguous = h.refused(Action::NameSpeaker { meeting: s("2026-11-20"), speaker: s("Alexandre"), name: s("X") });
    assert_eq!(ambiguous, "plusieurs locuteurs pour « Alexandre » : SPEAKER_00, SPEAKER_02");
    assert_eq!(h.refused(Action::NameSpeaker { meeting: s("2026-11-20"), speaker: s("0"), name: s(" ") }), "le nom est vide");
}

#[test]
fn speakers_merge_and_split() {
    let mut h = host();
    let done = h.ok(Action::MergeSpeaker { meeting: s("2026-11-20"), speaker: s("SPEAKER_01"), into: s("Alexandre") });
    assert_eq!(done.message, "2026-11-20-1000 · SPEAKER_01 fusionné dans SPEAKER_00");
    assert_eq!(h.release().speakers[1].merged_into, Some(0));
    assert_eq!(h.refused(Action::MergeSpeaker { meeting: s("2026-11-20"), speaker: s("1"), into: s("0") }), "ces deux locuteurs sont déjà la même voix");
    let done = h.ok(Action::SplitSpeaker { meeting: s("2026-11-20"), speaker: s("1") });
    assert_eq!(done.message, "2026-11-20-1000 · SPEAKER_01 séparé");
    assert_eq!(h.release().speakers[1].merged_into, None);
    assert_eq!(h.refused(Action::SplitSpeaker { meeting: s("2026-11-20"), speaker: s("1") }), "ce locuteur n'est pas fusionné");
}

// --- Transcription ---

#[test]
fn an_utterance_is_named_by_number_or_instant() {
    let mut h = host();
    let done = h.ok(Action::Correct { meeting: s("2026-11-20"), utterance: s("#2"), text: s("Oui, si la recette passe jeudi.") });
    assert_eq!(done.message, "2026-11-20-1000 · réplique #2 corrigée");
    assert_eq!(h.release().utterances[1].text, "Oui, si la recette passe jeudi.");
    h.ok(Action::Correct { meeting: s("2026-11-20"), utterance: s("1"), text: s("Bonjour Khalid, on publie la release mardi ?") });
    assert_eq!(h.release().utterances[0].text, "Bonjour Khalid, on publie la release mardi ?");
    // 0:00:03 is in the second utterance (2.0 to 3.5).
    h.ok(Action::Correct { meeting: s("2026-11-20"), utterance: s("0:00:03"), text: s("Oui, si la recette passe lundi.") });
    assert_eq!(h.release().utterances[1].text, "Oui, si la recette passe lundi.");
}

#[test]
fn an_unknown_utterance_is_refused() {
    let mut h = host();
    let correct = |u: &str| Action::Correct { meeting: s("2026-11-20"), utterance: s(u), text: s("x") };
    assert_eq!(h.refused(correct("#9")), "réplique introuvable : « #9 » (numéro de 1 à 3, ou instant h:mm:ss)");
    assert_eq!(h.refused(correct("0")), "réplique introuvable : « 0 » (numéro de 1 à 3, ou instant h:mm:ss)");
    assert_eq!(h.refused(correct("0:00:04")), "aucune réplique à 0:00:04");
    assert_eq!(h.refused(correct("bof")), "réplique introuvable : « bof » (numéro de 1 à 3, ou instant h:mm:ss)");
    let overlap = h.refused(Action::Correct { meeting: s("2026-11-20"), utterance: s("3"), text: s("Je préviens la hotline demain.") });
    assert_eq!(overlap, "ce passage est à vérifier : il se tranche dans l'onglet À vérifier");
}

#[test]
fn passages_are_resolved() {
    let mut h = host();
    let resolve = |r: Resolution| Action::Resolve { meeting: s("2026-11-20"), passage: s("#1"), resolution: r };
    let done = h.ok(resolve(Resolution::Accept));
    assert_eq!(done.message, "2026-11-20-1000 · passage #1 : hypothèse acceptée");
    assert_eq!(h.release().passages[0].status, PassageStatus::HypothesisAccepted);
    assert_eq!(h.release().utterances[2].text, "Je préviens l'équipe support demain.");
    assert_eq!(h.ok(resolve(Resolution::Reopen)).message, "2026-11-20-1000 · passage #1 : rouvert");
    assert_eq!(h.ok(resolve(Resolution::Keep)).message, "2026-11-20-1000 · passage #1 : original gardé");
    h.ok(resolve(Resolution::Reopen));
    assert_eq!(h.ok(resolve(Resolution::Correct(s("la hotline")))).message, "2026-11-20-1000 · passage #1 : corrigé");
    assert_eq!(h.release().utterances[2].text, "Je préviens la hotline demain.");
    let unknown = h.refused(Action::Resolve { meeting: s("2026-11-20"), passage: s("2"), resolution: Resolution::Keep });
    assert_eq!(unknown, "passage introuvable : « 2 » (numéro de 1 à 1)");
    let none = h.refused(Action::Resolve { meeting: s("2026-11-19"), passage: s("1"), resolution: Resolution::Keep });
    assert_eq!(none, "aucun passage à vérifier dans 2026-11-19-1530");
}

// --- Summary ---

#[test]
fn actions_are_checked_and_unchecked() {
    let mut h = host();
    let done = h.ok(Action::CheckAction { meeting: s("2026-11-20"), action: s("1"), done: true });
    assert_eq!(done.message, "2026-11-20-1000 · action #1 cochée");
    let boxes = greffier_core::summary::checkboxes(&h.release().summary);
    assert!(boxes[0].0 && boxes[1].0);
    let done = h.ok(Action::CheckAction { meeting: s("2026-11-20"), action: s("#2"), done: false });
    assert_eq!(done.message, "2026-11-20-1000 · action #2 décochée");
    assert!(!greffier_core::summary::checkboxes(&h.release().summary)[1].0);
    let summary = std::fs::read_to_string(h.folder().join(RELEASE).join(format!("{RELEASE} - bilan.md"))).unwrap();
    assert!(summary.contains("- [x] **Alexandre**") && summary.contains("- [ ] **Khalid**"), "{summary}");
    assert_eq!(h.refused(Action::CheckAction { meeting: s("2026-11-20"), action: s("3"), done: true }), "action introuvable : « 3 » (numéro de 1 à 2)");
}

// --- Delete ---

#[test]
fn delete_trashes_the_meeting_and_tells_the_host() {
    let mut h = host();
    let id = h.release().id;
    let done = h.ok(Action::Delete { meeting: s("2026-11-20") });
    assert_eq!(done.message, "2026-11-20-1000 · mise à la corbeille");
    assert!(!h.folder().join(RELEASE).exists());
    assert!(h.core.meeting(id).is_none());
    assert_eq!(h.deleted, [id]);
}

// --- Import ---

#[test]
fn import_adds_the_meeting_and_queues_it() {
    let mut h = host();
    let (outside, file) = media();
    let done = h.ok(Action::Import { files: vec![file.clone()], title: Some(s("Point client")), speakers: Some(2), process: true });
    let id = done.meeting.clone().expect("identifier");
    let name = done.folder_name.clone().expect("folder");
    assert!(name.ends_with(" - Point client"), "{name}");
    assert_eq!(done.message, format!("{id} · importée, dans la file : Transcription"));
    let m = h.named(&name);
    assert_eq!(m.status, Status::Pending(greffier_core::Stage::Transcription));
    assert_eq!(m.speaker_count, Some(2));
    assert!(file.exists(), "the original stays");
    assert!(h.events.iter().any(|e| matches!(e, Event::MeetingImported(_))), "events go to the host: {:?}", h.events);
    drop(outside);
}

#[test]
fn import_without_processing_leaves_it_to_process() {
    let mut h = host();
    let (_outside, file) = media();
    let done = h.ok(Action::Import { files: vec![file], title: None, speakers: None, process: false });
    assert_eq!(done.message, format!("{} · importée", done.meeting.clone().unwrap()));
    assert_eq!(h.named(&done.folder_name.unwrap()).status, Status::ToProcess);
    assert!(!h.core.busy());
}

#[test]
fn import_of_something_else_is_refused() {
    let mut h = host();
    let message = h.refused(Action::Import { files: vec![PathBuf::from("/tmp/notes.txt")], title: None, speakers: None, process: true });
    assert!(message.starts_with("« notes.txt » : Greffier importe un média"), "{message}");
}

#[test]
fn the_host_receives_the_events_of_each_action() {
    let mut h = host();
    h.ok(Action::Rename { meeting: s("2026-11-20"), title: s("Revue") });
    h.ok(Action::Process { meeting: s("2026-11-19"), speakers: None });
    assert!(h.deleted.is_empty());
    // Commands that produce no event leave the host's list as is; the server never invents any.
    assert!(h.events.iter().all(|e| !matches!(e, Event::MeetingImported(_))));
}
