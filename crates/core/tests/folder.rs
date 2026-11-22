//! Reading the meetings folder: each `YYYY-MM-DD HHhMM - Title` subfolder gives a meeting, with
//! what it contains (sheet, summary, structured transcription, recording), even incomplete. The
//! sample meetings are in `tests/fixtures/meetings-folder`.

mod bench;

use bench::Bench;
use greffier_core::fake::FakeResources;
use greffier_core::{Command, DateTime, Stage, MeetingId, Marker, Meeting, Role, Status, PassageStatus};

#[test]
fn a_meeting_with_only_its_recording_is_to_process() {
    let mut bench = Bench::on_fixtures();
    let r = bench.named_meeting("2026-11-19 15h30 - Réunion");
    assert_eq!(r.start, DateTime::new(2026, 11, 19, 15, 30));
    assert_eq!(r.title, "Réunion");
    assert_eq!(r.duration, 12.0, "the duration is read from the recording");
    assert_eq!(r.status, Status::ToProcess);
    assert_eq!(r.completed_stage, None);
    assert_eq!(r.files, [Role::Recording]);
    assert!(r.utterances.is_empty() && r.summary.is_empty() && r.speakers.is_empty());

    let id = r.id;
    bench.execute(Command::Process(id));
    assert_eq!(bench.meeting(id).status, Status::Pending(Stage::Transcription));
}

#[test]
fn a_complete_meeting_is_read_from_its_sheet_summary_and_transcription() {
    let bench = Bench::on_fixtures();
    let r = bench.named_meeting("2026-11-20 10h00 - Revue de release");
    assert_eq!(r.title, "Revue de release");
    assert_eq!(r.start, DateTime::new(2026, 11, 20, 10, 0));
    assert_eq!(r.duration, 12.0);
    assert_eq!(r.status, Status::Done);
    assert_eq!(r.completed_stage, Some(Stage::Summary));
    assert_eq!(r.speaker_count, Some(3));
    assert_eq!(r.files, Role::ALL);
    assert!(!r.is_new);

    // Sheet: names, proposals, merges, markers, sessions, brief and key actions.
    let speakers: Vec<(&str, String)> = r.speakers.iter().map(|l| (l.label.as_str(), l.display_name())).collect();
    assert_eq!(speakers, [("SPEAKER_00", "Alexandre".into()), ("SPEAKER_01", "Khalid ?".into()), ("SPEAKER_02", "SPEAKER_02".into())]);
    assert_eq!(r.speakers[1].justification, "Alexandre l'appelle Khalid à 0:00:00.");
    assert_eq!(r.speakers[2].merged_into, Some(0));
    assert_eq!(r.markers, [Marker { instant: 5.0, label: "Le support".into() }]);
    assert_eq!(r.sessions.review.as_deref(), Some("sess-relecture"));
    assert_eq!(r.sessions.summary.as_deref(), Some("sess-bilan"));
    assert_eq!(r.brief, "On publie la release lundi si la recette passe vendredi.");
    assert_eq!(r.key_actions, ["Alexandre — publier la release — lundi", "Khalid — lancer la recette — vendredi"]);

    // Structured transcription: words, turns, utterances, review, passages to check.
    assert_eq!(r.words.len(), 19);
    assert_eq!(r.words[11].text, "recète");
    assert_eq!(r.words[11].confidence, 0.42);
    assert_eq!(r.turns.len(), 3);
    let utterances: Vec<(usize, f64, f64, &str)> = r.utterances.iter().map(|x| (r.displayed_speaker(x.speaker), x.start, x.end, x.text.as_str())).collect();
    assert_eq!(
        utterances,
        [
            (0, 0.0, 2.0, "Bonjour Khalid, on publie la release lundi ?"),
            (1, 2.0, 3.5, "Oui, si la recette passe vendredi."),
            (0, 5.0, 6.25, "Je préviens le support demain."),
        ]
    );
    assert_eq!(r.corrections, 1);
    assert_eq!(r.passages.len(), 1);
    assert_eq!(r.passages[0].hypothesis.as_deref(), Some("l'équipe support"));
    assert_eq!(r.passages[0].status, PassageStatus::Pending);
    assert_eq!(r.utterances[2].passages, [0]);

    // Summary: rendered as is, topics and decisions for the timeline.
    assert!(r.summary.starts_with("## En bref\n\nOn publie la release lundi"));
    let topics: Vec<(f64, &str)> = r.topics.iter().map(|s| (s.start, s.title.as_str())).collect();
    assert_eq!(topics, [(0.0, "Date de la release"), (5.0, "Support")]);
    assert_eq!(r.decisions.len(), 1);
    assert_eq!(r.decisions[0].instant, 2.0);
}

#[test]
fn a_meeting_with_its_recording_and_transcription_resumes_at_review() {
    let mut bench = Bench::on_fixtures();
    let r = bench.named_meeting("2026-11-18 09h15 - Réunion");
    assert_eq!(r.status, Status::ToProcess);
    assert_eq!(r.completed_stage, Some(Stage::Transcription), "a transcription without a review log is not reviewed");
    assert_eq!(r.duration, 12.0);
    assert_eq!(r.files, [Role::StructuredTranscription, Role::Recording]);
    let speakers: Vec<&str> = r.speakers.iter().map(|l| l.label.as_str()).collect();
    assert_eq!(speakers, ["SPEAKER_00"]);
    let texts: Vec<&str> = r.utterances.iter().map(|x| x.text.as_str()).collect();
    assert_eq!(texts, ["On commence par le point sur le banc de test.", "Il reste deux mesures à refaire."]);
    assert!(r.words.is_empty() && r.passages.is_empty() && r.review.is_none() && r.summary.is_empty());

    let id = r.id;
    bench.execute(Command::Process(id));
    assert_eq!(bench.meeting(id).status, Status::Pending(Stage::Review));
}

#[test]
fn only_meeting_folders_count_most_recent_first() {
    let bench = Bench::on_fixtures();
    let names: Vec<&str> = bench.greffier.meetings().iter().map(|r| r.folder_name.as_str()).collect();
    assert_eq!(names, ["2026-11-20 10h00 - Revue de release", "2026-11-19 15h30 - Réunion", "2026-11-18 09h15 - Réunion"]);
}

/// Records 5 s (2 speakers given, a marker at 2 s), processes to the end, names SPEAKER_00 and
/// ticks the 2nd action.
fn processed_and_edited_meeting(bench: &mut Bench) -> MeetingId {
    bench.execute(Command::StartRecording);
    bench.execute(Command::RecordingSpeakerCount(Some(2)));
    bench.advance(2.0);
    bench.execute(Command::AddMarker);
    bench.advance(3.0);
    let id = bench.finish_recording();
    bench.advance_until(id, |s| *s == Status::Done);
    bench.execute(Command::NameSpeaker { meeting: id, speaker: 0, name: "Alexandre".into() });
    bench.execute(Command::CheckAction { meeting: id, action: 1, done: true });
    id
}

#[test]
fn a_processed_meeting_comes_back_identical_at_next_launch() {
    let mut bench = Bench::new();
    let id = processed_and_edited_meeting(&mut bench);
    let before = bench.meeting(id).clone();
    assert_eq!(before.files, Role::ALL);

    bench.reopen();
    let after = bench.named_meeting(&before.folder_name).clone();
    assert_eq!(Meeting { id: before.id, ..after }, before);
}

#[test]
fn sheet_gives_metadata_brief_and_links() {
    let mut bench = Bench::new();
    let id = processed_and_edited_meeting(&mut bench);
    let name = bench.meeting(id).folder_name.clone();
    let sheet = bench.read(&bench.greffier.folder().file(&name, Role::Sheet));
    assert_eq!(
        sheet,
        r#"---
titre: "Point d'équipe : démo"
date: 2026-11-21
debut: "14:02"
duree: "0:00:05"
statut: terminee
etape_faite: bilan
nombre_de_locuteurs: 2
locuteurs:
  SPEAKER_00: "Alexandre"
propositions:
  SPEAKER_00:
    nom: "Alexandre"
    justification: "Khalid l'appelle Alexandre à 0:00:03."
  SPEAKER_01:
    nom: "Khalid"
    justification: "Répond à Alexandre."
reperes:
  - instant: "0:00:02"
    libelle: "Repère 1"
sessions:
  bilan: "session-bilan"
nouvelle: true
---

# Point d'équipe : démo

Alexandre salue l'équipe.

## Actions principales

- Alexandre — préparer la démo — lundi
- Khalid — relire la PR

## Fichiers

- [Bilan](<2026-11-21 14h02 - Point d'équipe - démo - bilan.md>)
- [Transcription](<2026-11-21 14h02 - Point d'équipe - démo - transcription.md>)
- [Transcription structurée](<2026-11-21 14h02 - Point d'équipe - démo - transcription.json>)
- [Enregistrement](<2026-11-21 14h02 - Point d'équipe - démo - enregistrement.mp4>)
"#
    );
}

#[test]
fn readable_transcription_gives_known_names() {
    let mut bench = Bench::new();
    let id = processed_and_edited_meeting(&mut bench);
    let name = bench.meeting(id).folder_name.clone();
    let md = bench.read(&bench.greffier.folder().file(&name, Role::ReadableTranscription));
    assert_eq!(md, "# Point d'équipe : démo — transcription\n\nRéunion du 2026-11-21 à 14h02, 0:00:05.\n\n[0:00:00] Alexandre : Bonjour à tous.\n\n[0:00:03] SPEAKER_01 : Salut Alexandre.\n");
}

#[test]
fn a_recording_cut_by_app_shutdown_is_reread_as_interrupted() {
    let mut bench = Bench::new();
    bench.execute(Command::StartRecording);
    bench.advance(3.0);
    bench.reopen();

    let r = bench.named_meeting("2026-11-21 14h02 - Réunion");
    assert_eq!(r.status, Status::RecordingInterrupted);
    assert!(r.files.contains(&Role::Recording));
    let id = r.id;
    bench.execute(Command::Process(id));
    assert_eq!(bench.meeting(id).status, Status::Pending(Stage::Transcription));
}

#[test]
fn a_processing_cut_by_app_shutdown_resumes_at_its_stage() {
    let mut bench = Bench::new();
    let id = bench.meeting_to_process();
    bench.execute(Command::Process(id));
    bench.advance_until(id, |s| matches!(s, Status::Running { stage: Stage::Review, .. }));
    let name = bench.meeting(id).folder_name.clone();
    bench.reopen();

    let r = bench.named_meeting(&name);
    assert_eq!(r.status, Status::Pending(Stage::Review));
    assert_eq!(r.completed_stage, Some(Stage::Transcription));
    assert_eq!(r.utterances.len(), 2);
    let id = r.id;
    assert_eq!(bench.advance_until(id, |s| *s == Status::Done), [Stage::Review, Stage::Summary]);
}

#[test]
fn a_failed_stage_stays_failed_at_next_launch() {
    let mut a = bench::adapters();
    a.resources = Box::new(FakeResources::missing(1.0));
    let mut bench = Bench::with(a);
    let id = bench.meeting_to_process();
    bench.execute(Command::Process(id));
    bench.advance_until(id, |s| matches!(s, Status::Failed { .. }));
    let before = bench.meeting(id).status.clone();
    let name = bench.meeting(id).folder_name.clone();
    bench.reopen();
    assert_eq!(bench.named_meeting(&name).status, before);
}
