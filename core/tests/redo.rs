//! Redoing a stage: the meeting goes back into the queue at that stage, and the following ones
//! are redone too.

mod bench;

use std::cell::RefCell;
use std::rc::Rc;

use bench::Bench;
use greffier_core::adapters::{Adapters, ReviewResult};
use greffier_core::fake::{FakeAgent, FakeDiarizer};
use greffier_core::{Command, Stage, MeetingId, Status};

/// A meeting recorded and processed to the end.
fn done_meeting(bench: &mut Bench) -> MeetingId {
    let id = bench.meeting_to_process();
    bench.execute(Command::Process(id));
    bench.advance_until(id, |s| *s == Status::Done);
    id
}

/// Redoes `stage` and returns the stages replayed until processing ends.
fn redo(bench: &mut Bench, id: MeetingId, stage: Stage) -> Vec<Stage> {
    bench.execute(Command::Redo { meeting: id, stage });
    assert_eq!(bench.meeting(id).status, Status::Pending(stage));
    bench.advance_until(id, |s| *s == Status::Done)
}

#[test]
fn redoing_the_transcription_replays_the_three_stages() {
    let mut bench = Bench::new();
    let id = done_meeting(&mut bench);
    assert_eq!(redo(&mut bench, id, Stage::Transcription), [Stage::Transcription, Stage::Review, Stage::Summary]);
}

#[test]
fn redoing_the_review_replays_the_review_then_the_summary() {
    let mut bench = Bench::new();
    let id = done_meeting(&mut bench);
    assert_eq!(redo(&mut bench, id, Stage::Review), [Stage::Review, Stage::Summary]);
}

#[test]
fn redoing_the_summary_replays_only_the_summary() {
    let mut bench = Bench::new();
    let id = done_meeting(&mut bench);
    assert_eq!(redo(&mut bench, id, Stage::Summary), [Stage::Summary]);
}

#[test]
fn redoing_the_transcription_with_another_speaker_count_passes_it_to_the_diarizer() {
    let requests = Rc::new(RefCell::new(Vec::new()));
    let d = requests.clone();
    let diarizer = FakeDiarizer::new(bench::STAGE_DURATION, move |request| {
        d.borrow_mut().push(request.speaker_count);
        Ok(bench::turns())
    });
    let mut bench = Bench::with(Adapters { diarizer: Box::new(diarizer), ..bench::adapters() });
    let id = done_meeting(&mut bench);

    bench.execute(Command::SpeakerCount { meeting: id, number: Some(3) });
    redo(&mut bench, id, Stage::Transcription);

    assert_eq!(*requests.borrow(), [None, Some(3)]);
    // The count is kept in the sheet.
    let name = bench.meeting(id).folder_name.clone();
    bench.reopen();
    assert_eq!(bench.named_meeting(&name).speaker_count, Some(3));
}

#[test]
fn redoing_the_transcription_forgets_the_old_speaker_names() {
    let mut bench = Bench::new();
    let id = done_meeting(&mut bench);
    bench.execute(Command::NameSpeaker { meeting: id, speaker: 0, name: "Odile".into() });

    redo(&mut bench, id, Stage::Transcription);

    let r = bench.meeting(id);
    assert!(r.speakers.iter().all(|l| l.name.is_none()), "new speakers: {:?}", r.speakers);
    assert!(r.review.is_some(), "the review was redone");
}

#[test]
fn redoing_the_summary_gives_the_agent_the_names_given_since() {
    let names = Rc::new(RefCell::new(Vec::new()));
    let n = names.clone();
    let agent = FakeAgent::new(
        bench::STAGE_DURATION,
        |_| Ok(ReviewResult::default()),
        bench::STAGE_DURATION,
        move |d| {
            n.borrow_mut().push(d.speakers.iter().map(|l| l.name.clone()).collect::<Vec<_>>());
            Ok((bench::SUMMARY.to_string(), bench::summary_result()))
        },
    );
    let mut bench = Bench::with(Adapters { agent: Box::new(agent), ..bench::adapters() });
    let id = done_meeting(&mut bench);
    bench.execute(Command::NameSpeaker { meeting: id, speaker: 1, name: "Odile".into() });

    redo(&mut bench, id, Stage::Summary);

    assert_eq!(*names.borrow(), [vec![None, None], vec![None, Some("Odile".to_string())]]);
}

#[test]
fn redoing_from_review_gives_the_lexicon_as_it_is_now() {
    let lexicons = Rc::new(RefCell::new(Vec::new()));
    let l = lexicons.clone();
    let agent = FakeAgent::new(
        bench::STAGE_DURATION,
        move |d| {
            l.borrow_mut().push(d.lexicon.clone());
            Ok(ReviewResult::default())
        },
        bench::STAGE_DURATION,
        |_| Ok((bench::SUMMARY.to_string(), bench::summary_result())),
    );
    let mut bench = Bench::with(Adapters { agent: Box::new(agent), ..bench::adapters() });
    let lexicon = bench.folder.join("lexique.md");
    std::fs::write(&lexicon, "## Mots justes\n\n- Lumion\n").unwrap();
    bench.execute(Command::ChangeSettings(greffier_core::Settings { lexicon: Some(lexicon.clone()), ..bench.greffier.settings().clone() }));
    let id = done_meeting(&mut bench);

    std::fs::write(&lexicon, "## Mots justes\n\n- Lumion\n- Ormeau\n").unwrap();
    redo(&mut bench, id, Stage::Review);

    let seen = lexicons.borrow();
    assert_eq!(seen.len(), 2);
    assert!(!seen[0].contains("Ormeau") && seen[1].contains("Ormeau"), "{seen:?}");
}

#[test]
fn a_redone_stage_survives_a_restart_and_resumes_at_that_stage() {
    let mut bench = Bench::new();
    let id = done_meeting(&mut bench);
    bench.execute(Command::Redo { meeting: id, stage: Stage::Review });
    let name = bench.meeting(id).folder_name.clone();

    bench.reopen();
    let r = bench.named_meeting(&name);
    assert_eq!((r.status.clone(), r.completed_stage), (Status::Pending(Stage::Review), Some(Stage::Transcription)));
    let id = r.id;
    assert_eq!(bench.advance_until(id, |s| *s == Status::Done), [Stage::Review, Stage::Summary]);
}

#[test]
fn redo_is_rejected_in_the_queue_or_if_earlier_stages_are_missing() {
    let mut bench = Bench::new();
    let fresh = bench.meeting_to_process();
    assert!(!bench.meeting(fresh).can_redo(Stage::Review));
    assert!(bench.greffier.execute(Command::Redo { meeting: fresh, stage: Stage::Review }).is_err());
    assert!(bench.meeting(fresh).can_redo(Stage::Transcription));

    bench.execute(Command::Process(fresh));
    for stage in Stage::ALL {
        assert!(!bench.meeting(fresh).can_redo(stage));
        assert!(bench.greffier.execute(Command::Redo { meeting: fresh, stage }).is_err(), "{stage:?} during processing");
    }
    bench.advance_until(fresh, |s| *s == Status::Done);
    for stage in Stage::ALL {
        assert!(bench.meeting(fresh).can_redo(stage), "{stage:?} once done");
    }
}

#[test]
fn a_failed_stage_can_be_redone_from_earlier() {
    let agent = FakeAgent::new(bench::STAGE_DURATION, |_| Ok(ReviewResult::default()), bench::STAGE_DURATION, |_| Err("Claude ne répond pas.".into()));
    let mut bench = Bench::with(Adapters { agent: Box::new(agent), ..bench::adapters() });
    let id = bench.meeting_to_process();
    bench.execute(Command::Process(id));
    bench.advance_until(id, |s| matches!(s, Status::Failed { .. }));

    bench.execute(Command::Redo { meeting: id, stage: Stage::Review });
    bench.advance(0.1);
    assert!(matches!(bench.meeting(id).status, Status::Running { stage: Stage::Review, .. }));
}
