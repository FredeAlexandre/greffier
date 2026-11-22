//! The processing queue: one meeting at a time, Transcription → Review → Summary, stopping,
//! pausing during a recording.

mod bench;

use bench::Bench;
use greffier_core::fake::FakeCapture;
use greffier_core::{Command, Stage, Status};

#[test]
fn a_meeting_to_process_goes_through_the_three_stages_until_done() {
    let mut bench = Bench::new();
    let id = bench.meeting_to_process();
    assert_eq!(bench.meeting(id).status, Status::ToProcess);

    bench.execute(Command::Process(id));
    let stages = bench.advance_until(id, |s| *s == Status::Done);

    assert_eq!(stages, [Stage::Transcription, Stage::Review, Stage::Summary]);
    let r = bench.meeting(id);
    assert_eq!(r.title, "Point d'équipe : démo");
    let utterances: Vec<(&str, &str)> = r.utterances.iter().map(|x| (r.speakers[x.speaker].label.as_str(), x.text.as_str())).collect();
    assert_eq!(utterances, [("SPEAKER_00", "Bonjour à tous."), ("SPEAKER_01", "Salut Alexandre.")]);
    assert_eq!(r.topics.len(), 1);
    assert_eq!(r.topics[0].title, "Salutations");
}

#[test]
fn stopping_processing_puts_it_back_to_process() {
    let mut bench = Bench::new();
    let id = bench.meeting_to_process();
    bench.execute(Command::Process(id));
    bench.advance(0.5);
    assert!(matches!(bench.meeting(id).status, Status::Running { stage: Stage::Transcription, .. }));

    bench.execute(Command::StopProcessing(id));
    assert_eq!(bench.meeting(id).status, Status::ToProcess);
    bench.advance(5.0);
    assert_eq!(bench.meeting(id).status, Status::ToProcess);
    assert_eq!(bench.greffier.rank(id), None);
    assert!(bench.meeting(id).utterances.is_empty());
}

#[test]
fn queue_processes_one_meeting_at_a_time_in_order() {
    let mut bench = Bench::new();
    let first = bench.meeting_to_process();
    let second = bench.meeting_to_process();
    bench.execute(Command::Process(first));
    bench.execute(Command::Process(second));
    bench.advance(0.5);

    assert!(bench.meeting(first).status.is_processing());
    assert_eq!(bench.meeting(second).status, Status::Pending(Stage::Transcription));
    assert_eq!((bench.greffier.rank(first), bench.greffier.rank(second)), (Some(0), Some(1)));

    bench.advance_until(first, |s| *s == Status::Done);
    assert!(!matches!(bench.meeting(second).status, Status::Done));
    bench.advance_until(second, |s| *s == Status::Done);
}

#[test]
fn meeting_enters_the_queue_when_the_recording_stops() {
    let mut bench = Bench::new();
    assert!(bench.greffier.settings().process_on_stop);
    let id = bench.record(3.0);
    assert_eq!(bench.meeting(id).status, Status::Pending(Stage::Transcription));
    bench.advance_until(id, |s| *s == Status::Done);
}

#[test]
fn queue_waits_for_the_recording_to_end() {
    let mut bench = Bench::new();
    let id = bench.meeting_to_process();
    bench.execute(Command::Process(id));
    bench.execute(Command::StartRecording);
    bench.advance(5.0);
    assert_eq!(bench.meeting(id).status, Status::Pending(Stage::Transcription));

    bench.execute(Command::StopRecording);
    bench.advance(0.1);
    assert!(bench.meeting(id).status.is_processing());
}

#[test]
fn started_stage_runs_to_completion_during_a_recording() {
    let mut bench = Bench::new();
    let id = bench.meeting_to_process();
    bench.execute(Command::Process(id));
    bench.advance(0.5);
    bench.execute(Command::StartRecording);
    bench.advance(3.0);

    let r = bench.meeting(id);
    assert_eq!(r.status, Status::Pending(Stage::Review));
    assert_eq!(r.utterances.len(), 2);

    bench.execute(Command::StopRecording);
    let stages = bench.advance_until(id, |s| *s == Status::Done);
    assert_eq!(stages, [Stage::Review, Stage::Summary]);
}

#[test]
fn a_summary_without_decisions_fails_the_stage_and_processing_retries_it() {
    let incomplete = "## En bref\n\nRien.\n\n## Sujets abordés\n\n### Salutations (0:00:00)\n\n## Actions\n\n- [ ] **Alexandre** — rien\n";
    let mut a = bench::adapters();
    let summaries = std::rc::Rc::new(std::cell::Cell::new(0));
    let n = summaries.clone();
    a.agent = Box::new(greffier_core::fake::FakeAgent::new(
        bench::STAGE_DURATION,
        |_| Ok(Default::default()),
        bench::STAGE_DURATION,
        move |_| {
            n.set(n.get() + 1);
            Ok(((if n.get() == 1 { incomplete } else { bench::SUMMARY }).to_string(), bench::summary_result()))
        },
    ));
    let mut bench = Bench::with(a);
    let id = bench.meeting_to_process();
    bench.execute(Command::Process(id));
    bench.advance_until(id, |s| matches!(s, Status::Failed { .. }));
    assert_eq!(bench.meeting(id).status, Status::Failed { stage: Stage::Summary, message: "Le bilan n'a pas de décisions horodatées.".into() });
    // The agent's session is kept anyway, to resume the conversation.
    assert_eq!(bench.meeting(id).sessions.summary.as_deref(), Some("session-bilan"));
    let name = bench.meeting(id).folder_name.clone();
    let sheet = bench.read(&bench.greffier.folder().file(&name, greffier_core::Role::Sheet));
    assert!(sheet.contains("bilan: \"session-bilan\""), "{sheet}");

    bench.execute(Command::Process(id));
    let stages = bench.advance_until(id, |s| *s == Status::Done);
    assert_eq!(stages, [Stage::Summary]);
    assert_eq!(summaries.get(), 2);
}

#[test]
fn a_failing_transcriber_fails_the_transcription() {
    let mut a = bench::adapters();
    a.transcriber = Box::new(greffier_core::fake::FakeTranscriber::new(1.0, |_| Err("whisper a planté".into())));
    let mut bench = Bench::with(a);
    let id = bench.meeting_to_process();
    bench.execute(Command::Process(id));
    bench.advance(2.0);
    assert_eq!(bench.meeting(id).status, Status::Failed { stage: Stage::Transcription, message: "whisper a planté".into() });
    assert!(!bench.greffier.busy());
}

#[test]
fn without_resources_transcription_fails_until_they_are_installed() {
    let mut a = bench::adapters();
    a.resources = Box::new(greffier_core::fake::FakeResources::missing(2.0));
    let mut bench = Bench::with(a);
    let id = bench.meeting_to_process();
    bench.execute(Command::Process(id));
    bench.advance(0.1);
    assert!(matches!(&bench.meeting(id).status, Status::Failed { stage: Stage::Transcription, message } if message.starts_with("Ressources manquantes")));

    bench.execute(Command::InstallResources);
    let events = bench.advance(2.5);
    assert!(events.contains(&greffier_core::Event::ResourcesInstalled));
    bench.execute(Command::Process(id));
    bench.advance_until(id, |s| *s == Status::Done);
}

#[test]
fn queue_also_resumes_when_the_recording_is_interrupted() {
    let mut a = bench::adapters();
    a.capture = Box::new(FakeCapture::new(1.0, 0.0).broken_at(20.0, "Flux coupé."));
    let mut bench = Bench::with(a);
    let id = bench.meeting_to_process();
    bench.execute(Command::Process(id));
    bench.execute(Command::StartRecording);
    bench.advance(15.0);
    assert_eq!(bench.meeting(id).status, Status::Pending(Stage::Transcription));

    bench.advance(5.5);
    assert!(bench.greffier.recording().is_none());
    assert!(bench.meeting(id).status.is_processing(), "{:?}", bench.meeting(id).status);
}
