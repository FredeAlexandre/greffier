//! Les commandes sur une réunion : enregistrement, repères, locuteurs, passages à vérifier,
//! actions du bilan, suppression.

mod bench;

use bench::Bench;
use greffier_core::adapters::{FlaggedPassage, ReviewResult};
use greffier_core::fake::FakeAgent;
use greffier_core::{Command, MeetingId, Role, Status, PassageStatus, Resolution};

fn done_meeting(bench: &mut Bench) -> MeetingId {
    let id = bench.meeting_to_process();
    bench.execute(Command::Process(id));
    bench.advance_until(id, |s| *s == Status::Done);
    id
}

#[test]
fn a_recording_creates_the_meeting_folder_with_its_markers() {
    let mut bench = Bench::new();
    bench.execute(Command::StartRecording);
    bench.execute(Command::RecordingSpeakerCount(Some(3)));
    bench.advance(2.0);
    bench.execute(Command::AddMarker);
    bench.advance(1.0);
    assert!((bench.greffier.recording().expect("en cours").duration - 3.0).abs() < 1e-9);
    let id = bench.finish_recording();

    let r = bench.meeting(id);
    assert_eq!(r.folder_name, "2026-11-21 14h02 - Réunion");
    assert_eq!(r.title, "Réunion");
    assert_eq!(r.speaker_count, Some(3));
    assert_eq!(r.markers.len(), 1);
    assert_eq!(r.markers[0].label, "Repère 1");
    assert!((r.markers[0].instant - 2.0).abs() < 1e-9);
    let file = bench.folder.join("2026-11-21 14h02 - Réunion/2026-11-21 14h02 - Réunion - enregistrement.mp4");
    assert!(file.exists(), "{}", file.display());
    assert_eq!(bench.greffier.folder().file(&r.folder_name, Role::Recording), file);
}

#[test]
fn two_recordings_in_the_same_minute_each_get_their_own_folder() {
    let mut bench = Bench::new();
    let a = bench.meeting_to_process();
    let b = bench.meeting_to_process();
    assert_eq!(bench.meeting(a).folder_name, "2026-11-21 14h02 - Réunion");
    assert_eq!(bench.meeting(b).folder_name, "2026-11-21 14h02 - Réunion (2)");
}

#[test]
fn summary_writes_its_file_and_proposes_a_name_per_speaker() {
    let mut bench = Bench::new();
    let id = done_meeting(&mut bench);
    let r = bench.meeting(id);

    let file = bench.folder.join("2026-11-21 14h02 - Point d'équipe - démo/2026-11-21 14h02 - Point d'équipe - démo - bilan.md");
    assert_eq!(bench.read(&file), bench::SUMMARY);
    assert_eq!(r.brief, "Alexandre salue l'équipe.");
    assert_eq!(r.key_actions, ["Alexandre — préparer la démo — lundi", "Khalid — relire la PR"]);
    assert_eq!(r.sessions.summary.as_deref(), Some("session-bilan"));
    assert_eq!(r.decisions[0].instant, 3.0);
    let names: Vec<String> = r.speakers.iter().map(|l| l.display_name()).collect();
    assert_eq!(names, ["Alexandre ?", "Khalid ?"]);
    assert!(r.is_new);
}

#[test]
fn name_a_speaker_then_merge_another_into_it() {
    let mut bench = Bench::new();
    let id = done_meeting(&mut bench);

    bench.execute(Command::NameSpeaker { meeting: id, speaker: 0, name: "Alexandre".into() });
    assert_eq!(bench.meeting(id).speakers[0].display_name(), "Alexandre");

    bench.execute(Command::MergeSpeakers { meeting: id, speaker: 1, into: 0 });
    let r = bench.meeting(id);
    assert_eq!(r.active_speakers().collect::<Vec<_>>(), [0]);
    assert_eq!(r.displayed_speaker(r.utterances[1].speaker), 0);
    assert_eq!(r.speaking_time(0), 1.0);

    let rejection = bench.greffier.execute(Command::MergeSpeakers { meeting: id, speaker: 0, into: 1 });
    assert!(rejection.is_err(), "fusionner un locuteur dans celui qu'il a absorbé");
}

#[test]
fn review_corrects_and_flags_passages_to_decide() {
    let mut a = bench::adapters();
    a.agent = Box::new(FakeAgent::new(
        bench::STAGE_DURATION,
        |_| {
            Ok(ReviewResult {
                session: Some("session-relecture".into()),
                corrections: vec![greffier_core::adapters::ConfidentCorrection { utterance: 0, before: "tous".into(), after: "toutes et tous".into() }],
                passages_to_check: vec![FlaggedPassage { utterance: 1, excerpt: "Alexandre".into(), hypothesis: Some("Alexandra".into()), relisten: "« Salut Alexandre. »".into() }],
                ..Default::default()
            })
        },
        bench::STAGE_DURATION,
        |_| Ok((bench::SUMMARY.to_string(), bench::summary_result())),
    ));
    let mut bench = Bench::with(a);
    let id = done_meeting(&mut bench);
    let r = bench.meeting(id);
    assert_eq!(r.utterances[0].text, "Bonjour à toutes et tous.");
    assert_eq!(r.corrections, 1);
    assert_eq!(r.sessions.review.as_deref(), Some("session-relecture"));
    assert_eq!(r.pending_passages(), 1);
    assert_eq!(r.utterances[1].passages, [0]);

    bench.execute(Command::Resolve { meeting: id, passage: 0, resolution: Resolution::AcceptHypothesis });
    assert_eq!(bench.meeting(id).passages[0].status, PassageStatus::HypothesisAccepted);
    assert_eq!(bench.meeting(id).pending_passages(), 0);

    bench.execute(Command::Resolve { meeting: id, passage: 0, resolution: Resolution::Correct("Alexandre !".into()) });
    assert_eq!(bench.meeting(id).passages[0].status, PassageStatus::CorrectedManually("Alexandre !".into()));

    bench.execute(Command::Resolve { meeting: id, passage: 0, resolution: Resolution::Reopen });
    assert_eq!(bench.meeting(id).pending_passages(), 1);
}

#[test]
fn ticking_an_action_changes_its_checkbox_in_the_summary_file() {
    let mut bench = Bench::new();
    let id = done_meeting(&mut bench);
    bench.execute(Command::CheckAction { meeting: id, action: 1, done: true });

    let file = bench.folder.join("2026-11-21 14h02 - Point d'équipe - démo/2026-11-21 14h02 - Point d'équipe - démo - bilan.md");
    let md = bench.read(&file);
    assert!(md.contains("- [ ] **Alexandre** — préparer la démo — lundi\n- [x] **Khalid** — relire la PR\n"), "{md}");
    assert_eq!(bench.meeting(id).summary, md);
}

#[test]
fn deleting_a_meeting_being_processed_frees_the_queue() {
    let mut bench = Bench::new();
    let first = bench.meeting_to_process();
    let second = bench.meeting_to_process();
    bench.execute(Command::Process(first));
    bench.execute(Command::Process(second));
    bench.advance(0.5);

    bench.execute(Command::Delete(first));
    assert!(bench.greffier.meeting(first).is_none());
    bench.advance(0.1);
    assert!(bench.meeting(second).status.is_processing());
}

#[test]
fn opening_a_done_meeting_marks_it_read() {
    let mut bench = Bench::new();
    let id = done_meeting(&mut bench);
    bench.execute(Command::MarkRead(id));
    assert!(!bench.meeting(id).is_new);
}
