//! Processing a meeting: the speaker count before processing, the run up to the summary, the
//! queue and stopping, redoing a stage (cycle 1 QA, C1, C2, C3, C11).

mod support;

use std::cell::RefCell;
use std::rc::Rc;

use greffier::{App, Page, Tab};
use greffier_core::fake::FakeDiarizer;
use greffier_core::{Greffier, MeetingId, Stage, Status, demo};
use rune::Runtime;
use support::*;

/// The prototype's meeting waiting to be processed.
fn a_meeting_to_process(rt: &mut Runtime<App>) -> MeetingId {
    state(rt, |s| s.core.meetings().iter().find(|m| m.status == Status::ToProcess && m.completed_stage.is_none()).expect("a meeting to process").id)
}

/// The prototype's most recent processed meeting with a transcription.
fn a_done_meeting(rt: &mut Runtime<App>) -> MeetingId {
    state(rt, |s| s.core.meetings().iter().find(|m| m.status == Status::Done && !m.utterances.is_empty()).expect("a processed meeting").id)
}

fn status(rt: &mut Runtime<App>, id: MeetingId) -> Status {
    state(rt, |s| s.core.meeting(id).expect("the meeting").status.clone())
}

fn open(rt: &mut Runtime<App>, id: MeetingId) {
    tap(rt, &format!("m{id}"));
    wait_for_motion(rt);
}

// --- C1: the speaker count before processing ----------------------------------------------------

#[test]
fn the_speaker_count_chosen_before_processing_reaches_the_diarization() {
    let asked = Rc::new(RefCell::new(Vec::new()));
    let seen = asked.clone();
    let diarizer = FakeDiarizer::new(demo::simulated_duration(Stage::Transcription), move |r| {
        seen.borrow_mut().push(r.speaker_count);
        Ok(vec![])
    });
    let mut rt = app_on(Greffier::with_meetings(temp_folder(), adapters_with(|a| a.diarizer = Box::new(diarizer)), demo::meetings()));
    let id = a_meeting_to_process(&mut rt);
    open(&mut rt, id);
    assert_eq!(state(&mut rt, |s| s.core.meeting(id).expect("the meeting").speaker_count), None, "detected by default");

    tap(&mut rt, "ppl-plus");
    tap(&mut rt, "ppl-plus");
    assert_eq!(state(&mut rt, |s| s.core.meeting(id).expect("the meeting").speaker_count), Some(3));
    tap(&mut rt, "process");

    assert_eq!(*asked.borrow(), [Some(3)]);
}

#[test]
fn minus_from_two_speakers_goes_back_to_detected() {
    let mut rt = app_on(demo_core());
    let id = a_meeting_to_process(&mut rt);
    open(&mut rt, id);

    tap(&mut rt, "ppl-plus");
    tap(&mut rt, "ppl-minus");

    assert_eq!(state(&mut rt, |s| s.core.meeting(id).expect("the meeting").speaker_count), None);
}

// --- C2: the run ----------------------------------------------------------------------------------

#[test]
fn processing_moves_on_with_time_until_the_summary_is_ready() {
    let mut rt = app_on(demo_core());
    let id = a_meeting_to_process(&mut rt);
    open(&mut rt, id);
    tap(&mut rt, "process");

    wait(&mut rt, 1.0);
    let Status::Running { stage: Stage::Transcription, progress } = status(&mut rt, id) else { panic!("transcribing: {:?}", status(&mut rt, id)) };
    assert!(progress > 0.0);
    let all_stages: f64 = Stage::ALL.into_iter().map(demo::simulated_duration).sum();
    wait(&mut rt, all_stages);

    state(&mut rt, |s| {
        let m = s.core.meeting(id).expect("the meeting");
        assert_eq!(m.status, Status::Done);
        assert!(!m.summary.is_empty());
        assert!(!m.is_new, "read: its page was open when the summary arrived");
        assert!(s.toast.is_some(), "a toast announces the summary");
        assert!(s.page == Page::Meeting(id));
    });
}

// --- C3: the queue --------------------------------------------------------------------------------

#[test]
fn stopping_the_meeting_being_processed_hands_over_to_the_next_in_the_queue() {
    let mut rt = app_on(demo_core());
    let (first, second) = (a_meeting_to_process(&mut rt), a_done_meeting(&mut rt));
    open(&mut rt, first);
    tap(&mut rt, "process");
    secondary_tap(&mut rt, &format!("m{second}"));
    choose(&mut rt, &format!("menu{second}"), "Refaire le bilan");
    state(&mut rt, |s| {
        assert_eq!(s.core.rank(first), Some(0));
        assert_eq!(s.core.rank(second), Some(1), "one meeting at a time: the second waits");
    });

    wait_for_motion(&mut rt);
    tap(&mut rt, "stop-proc");

    assert_eq!(status(&mut rt, first), Status::ToProcess);
    state(&mut rt, |s| {
        assert_eq!(s.core.rank(first), None);
        assert_eq!(s.core.rank(second), Some(0));
    });
    assert!(matches!(status(&mut rt, second), Status::Running { stage: Stage::Summary, .. }));
}

// --- C11: redoing a stage -------------------------------------------------------------------------

#[test]
fn redo_the_summary_on_the_summary_tab_queues_the_summary() {
    let mut rt = app_on(demo_core());
    let id = a_done_meeting(&mut rt);
    open(&mut rt, id);
    scroll_page_to(&mut rt, "redo");

    tap(&mut rt, "redo");

    assert_eq!(queued_at(&status(&mut rt, id)), Some(Stage::Summary));
}

#[test]
fn redo_the_review_from_the_transcript_tab_queues_the_review() {
    let mut rt = app_on(demo_core());
    let id = a_done_meeting(&mut rt);
    open(&mut rt, id);
    press(&mut rt, cmd('2'));
    assert!(state(&mut rt, |s| s.tab == Tab::Transcript));
    scroll_page_to(&mut rt, "redo-menu-button");

    tap(&mut rt, "redo-menu-button");
    choose(&mut rt, "redo-menu", "Refaire la relecture");

    assert_eq!(queued_at(&status(&mut rt, id)), Some(Stage::Review));
    assert!(!state(&mut rt, |s| s.redo_menu), "the menu closes");
}

#[test]
fn redo_the_transcription_with_another_speaker_count_from_the_right_click_menu() {
    let mut rt = app_on(demo_core());
    let id = a_done_meeting(&mut rt);
    let Some(count) = state(&mut rt, |s| s.core.meeting(id).expect("the meeting").speaker_count) else { panic!("a meeting with a given speaker count") };

    secondary_tap(&mut rt, &format!("m{id}"));
    choose(&mut rt, &format!("menu{id}"), "Refaire la transcription…");
    assert!(state(&mut rt, |s| s.redo_transcription.as_ref().is_some_and(|r| r.meeting == id)), "the window asks the speaker count first");
    assert_eq!(status(&mut rt, id), Status::Done, "nothing is redone before confirming");
    wait_for_motion(&mut rt);
    tap(&mut rt, "redo-ppl-plus");
    tap(&mut rt, "redo-ppl-plus");
    tap(&mut rt, "redo-ok");

    state(&mut rt, |s| {
        let m = s.core.meeting(id).expect("the meeting");
        assert!(s.redo_transcription.is_none());
        assert_eq!(queued_at(&m.status), Some(Stage::Transcription));
        assert_eq!(m.speaker_count, Some(count + 2));
    });
}

#[test]
fn a_meeting_in_the_queue_offers_no_redo() {
    let mut rt = app_on(demo_core());
    let id = a_done_meeting(&mut rt);
    open(&mut rt, id);
    press(&mut rt, cmd('2'));
    assert!(shown(&rt, "redo-menu-button"), "done, its stages can be redone");
    press(&mut rt, cmd('1'));
    scroll_page_to(&mut rt, "redo");

    tap(&mut rt, "redo");
    assert!(queued_at(&status(&mut rt, id)).is_some());

    press(&mut rt, cmd('2'));
    assert!(!shown(&rt, "redo-menu-button"));
}
