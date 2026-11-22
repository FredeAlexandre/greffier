//! L'enregistrement par la fausse capture : règle de la minute, repères dans la fiche,
//! démarrage automatique du traitement, autorisations, récupération d'un enregistrement coupé.

mod bench;

use bench::Bench;
use greffier_core::adapters::{Permission, Permissions, PermissionState, Sources};
use greffier_core::fake::FakeCapture;
use greffier_core::{Command, Stage, Event, Rejection, Settings, Role, Status};

#[test]
fn a_recording_under_a_minute_is_offered_for_deletion() {
    let mut bench = Bench::new();
    bench.execute(Command::StartRecording);
    bench.advance(59.0);
    let events = bench.execute(Command::StopRecording);
    let id = bench.greffier.meetings()[0].id;
    assert_eq!(events[0], Event::MeetingRecorded(id));
    assert!(events.contains(&Event::DeletionProposed(id)), "{events:?}");
}

#[test]
fn a_recording_of_a_minute_or_more_is_not_offered_for_deletion() {
    let mut bench = Bench::new();
    bench.execute(Command::StartRecording);
    bench.advance(60.0);
    let events = bench.execute(Command::StopRecording);
    assert!(!events.iter().any(|e| matches!(e, Event::DeletionProposed(_))), "{events:?}");
}

#[test]
fn placed_markers_are_kept_in_the_sheet_and_read_back_on_next_launch() {
    let mut bench = Bench::new();
    bench.execute(Command::StartRecording);
    bench.advance(312.0);
    bench.execute(Command::AddMarker);
    bench.advance(10.5);
    bench.execute(Command::AddMarker);
    bench.advance(5.0);
    let id = bench.finish_recording();
    let name = bench.meeting(id).folder_name.clone();

    let sheet = bench.read(&bench.folder.join(format!("{name}/{name} - réunion.md")));
    assert!(sheet.contains("reperes:\n  - instant: \"0:05:12\"\n    libelle: \"Repère 1\"\n  - instant: \"0:05:22.5\"\n    libelle: \"Repère 2\"\n"), "{sheet}");

    bench.reopen();
    let r = bench.named_meeting(&name);
    let markers: Vec<(f64, &str)> = r.markers.iter().map(|x| (x.instant, x.label.as_str())).collect();
    assert_eq!(markers, [(312.0, "Repère 1"), (322.5, "Repère 2")]);
}

#[test]
fn without_auto_start_the_meeting_stays_to_process() {
    let mut bench = Bench::new();
    bench.execute(Command::ChangeSettings(Settings { process_on_stop: false, ..Settings::default() }));
    let id = bench.record(90.0);
    assert_eq!(bench.meeting(id).status, Status::ToProcess);
    bench.advance(5.0);
    assert_eq!(bench.meeting(id).status, Status::ToProcess);
}

#[test]
fn with_auto_start_the_meeting_is_queued_and_processed() {
    let mut bench = Bench::new();
    let id = bench.record(90.0);
    assert_eq!(bench.meeting(id).status, Status::Pending(Stage::Transcription));
    assert_eq!(bench.advance_until(id, |s| *s == Status::Done), [Stage::Transcription, Stage::Review, Stage::Summary]);
}

fn bench_with_permissions(permissions: Permissions) -> Bench {
    let mut a = bench::adapters();
    a.capture = Box::new(FakeCapture::new(1.0, 0.0).with_permissions(permissions));
    Bench::with(a)
}

fn rejection(bench: &mut Bench, command: Command) -> String {
    match bench.greffier.execute(command) {
        Err(Rejection::Impossible(m)) => m,
        other => panic!("refus attendu, obtenu {other:?}"),
    }
}


#[test]
fn without_screen_permission_recording_is_rejected_without_creating_anything() {
    let mut bench = bench_with_permissions(Permissions { screen: PermissionState::Denied, ..Permissions::GRANTED });
    assert_eq!(bench.greffier.permissions().screen, PermissionState::Denied);

    let message = rejection(&mut bench, Command::StartRecording);
    assert!(message.contains("Enregistrement de l'écran"), "{message}");
    assert!(bench.greffier.recording().is_none());
    assert_eq!(std::fs::read_dir(&bench.folder).unwrap().count(), 0);
}

#[test]
fn a_disabled_source_does_not_need_its_permission() {
    let mut bench = bench_with_permissions(Permissions { micro: PermissionState::Denied, ..Permissions::GRANTED });
    assert!(rejection(&mut bench, Command::StartRecording).contains("Micro"));

    let mut settings = Settings::default();
    settings.sources.micro = false;
    bench.execute(Command::ChangeSettings(settings));
    bench.execute(Command::StartRecording);
    assert_eq!(bench.greffier.recording().expect("en cours").sources, Sources { screen: true, mac_audio: true, micro: false });
}

#[test]
fn a_never_requested_permission_is_requested_at_startup() {
    let mut bench = bench_with_permissions(Permissions { micro: PermissionState::ToRequest, ..Permissions::GRANTED });
    let message = rejection(&mut bench, Command::StartRecording);
    assert!(message.contains("micro"), "{message}");
    // La fausse capture accepte toute demande, comme l'utilisateur qui clique « Autoriser ».
    assert_eq!(bench.greffier.permissions(), Permissions::GRANTED);
    bench.execute(Command::StartRecording);
    assert!(bench.greffier.recording().is_some());
}

#[test]
fn requesting_a_permission_from_settings() {
    let mut bench = bench_with_permissions(Permissions { screen: PermissionState::ToRequest, ..Permissions::GRANTED });
    bench.execute(Command::RequestPermission(Permission::Screen));
    assert_eq!(bench.greffier.permissions().screen, PermissionState::Granted);
}

#[test]
fn without_any_source_recording_is_rejected() {
    let mut bench = Bench::new();
    bench.execute(Command::ChangeSettings(Settings { sources: Sources { screen: false, mac_audio: false, micro: false }, ..Settings::default() }));
    assert!(rejection(&mut bench, Command::StartRecording).contains("source"));
}

#[test]
fn a_cut_recording_keeps_the_duration_already_written_in_its_fragments() {
    let mut bench = Bench::new();
    bench.execute(Command::StartRecording);
    bench.advance(90.0);
    bench.execute(Command::AddMarker);
    bench.advance(4.0);
    // L'app s'arrête net : pas d'ArreterEnregistrement.
    bench.reopen();

    let r = bench.named_meeting("2026-11-21 14h02 - Réunion");
    assert_eq!(r.status, Status::RecordingInterrupted);
    assert!((r.duration - 94.0).abs() < 0.05, "durée {}", r.duration);
}

#[test]
fn markers_and_speaker_count_survive_the_app_quitting() {
    let mut bench = Bench::new();
    bench.execute(Command::StartRecording);
    bench.advance(90.0);
    bench.execute(Command::AddMarker);
    bench.execute(Command::RecordingSpeakerCount(Some(3)));
    bench.advance(4.0);
    bench.reopen();

    let r = bench.named_meeting("2026-11-21 14h02 - Réunion");
    assert_eq!(r.status, Status::RecordingInterrupted);
    let markers: Vec<(f64, &str)> = r.markers.iter().map(|x| (x.instant, x.label.as_str())).collect();
    assert_eq!(markers, [(90.0, "Repère 1")]);
    assert_eq!(r.speaker_count, Some(3));
}

#[test]
fn an_interrupted_recording_is_read_and_processed_up_to_the_summary() {
    let mut bench = Bench::new();
    bench.execute(Command::StartRecording);
    bench.advance(75.0);
    bench.reopen();

    let r = bench.named_meeting("2026-11-21 14h02 - Réunion");
    assert!(r.files.contains(&Role::Recording), "{:?}", r.files);
    assert!((r.duration - 75.0).abs() < 0.05, "durée {}", r.duration);
    let id = r.id;
    bench.execute(Command::Process(id));
    assert_eq!(bench.advance_until(id, |s| *s == Status::Done), [Stage::Transcription, Stage::Review, Stage::Summary]);
    assert_eq!(bench.meeting(id).utterances.len(), 2);

    // Et elle n'est plus interrompue au lancement suivant.
    // Le titre du bilan a renommé le dossier.
    bench.reopen();
    assert_eq!(bench.named_meeting("2026-11-21 14h02 - Point d'équipe - démo").status, Status::Done);
}

fn bench_with_capture(capture: FakeCapture) -> Bench {
    let mut a = bench::adapters();
    a.capture = Box::new(capture);
    Bench::with(a)
}

#[test]
fn a_capture_stopped_by_macos_keeps_the_meeting_interrupted_with_what_was_captured() {
    let mut bench = bench_with_capture(FakeCapture::new(1.0, 0.0).broken_at(80.0, "L'écran a été débranché."));
    bench.execute(Command::StartRecording);
    bench.advance(30.0);
    bench.execute(Command::AddMarker);
    let events = bench.advance(60.0);

    assert!(bench.greffier.recording().is_none());
    let r = &bench.greffier.meetings()[0];
    let id = r.id;
    assert_eq!(events, [Event::RecordingInterrupted { meeting: id, message: "L'écran a été débranché.".into() }]);
    assert_eq!(r.status, Status::RecordingInterrupted);
    assert!((r.duration - 80.0).abs() < 0.15, "durée {}", r.duration);
    assert_eq!(r.markers.len(), 1);
    assert!(r.files.contains(&Role::Recording));
    // Pas de traitement d'office : on le lance quand on veut.
    bench.advance(2.0);
    assert_eq!(bench.meeting(id).status, Status::RecordingInterrupted);
    bench.execute(Command::Process(id));
    bench.advance_until(id, |s| *s == Status::Done);

    // Le titre du bilan a renommé le dossier.
    bench.reopen();
    assert_eq!(bench.named_meeting("2026-11-21 14h02 - Point d'équipe - démo").status, Status::Done);
}

#[test]
fn a_failed_stop_keeps_the_meeting_interrupted_with_its_fragments_duration() {
    let mut bench = bench_with_capture(FakeCapture::new(1.0, 0.0).failing_on_stop("Le fichier n'a pas pu être fini."));
    bench.execute(Command::StartRecording);
    bench.advance(70.0);
    let events = bench.execute(Command::StopRecording);

    assert!(bench.greffier.recording().is_none());
    let r = &bench.greffier.meetings()[0];
    assert_eq!(events, [Event::RecordingInterrupted { meeting: r.id, message: "Le fichier n'a pas pu être fini.".into() }]);
    assert_eq!(r.status, Status::RecordingInterrupted);
    assert!((r.duration - 70.0).abs() < 0.15, "durée {}", r.duration);
    bench.reopen();
    assert_eq!(bench.named_meeting("2026-11-21 14h02 - Réunion").status, Status::RecordingInterrupted);
}

#[test]
fn fake_capture_writes_an_mp4_whose_duration_is_read() {
    let mut bench = Bench::new();
    let id = bench.record(75.0);
    let name = bench.meeting(id).folder_name.clone();
    let file = bench.greffier.folder().file(&name, Role::Recording);
    let duration = greffier_core::folder::mp4_duration(&file).expect("durée du mp4");
    assert!((duration - 75.0).abs() < 0.05, "{duration}");
}

#[test]
fn an_avassetwriter_mp4_cut_mid_recording_keeps_its_duration() {
    // Écrit par la vraie capture (essai_ecrivain --couper) : 2 s dans le moov, puis des fragments
    // sans tfdt ; ffprobe y lit 6,04 s.
    let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mp4/avassetwriter-cut.mp4");
    let duration = greffier_core::folder::mp4_duration(&file).expect("durée");
    assert!((duration - 6.04).abs() < 0.01, "{duration}");
}

#[test]
fn neither_a_running_recording_nor_its_stop_count_as_outside_changes() {
    let mut bench = Bench::new();
    bench.execute(Command::StartRecording);
    bench.advance(30.0);
    assert_eq!(bench.greffier.rescan_folder(), []);
    bench.advance(45.0);
    let id = bench.finish_recording();
    assert_eq!(bench.greffier.rescan_folder(), []);
    assert_eq!(bench.greffier.meetings().iter().filter(|r| r.id == id).count(), 1);
}
