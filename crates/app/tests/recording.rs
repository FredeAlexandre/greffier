//! Recording a meeting: the button, ⇧⌘R, markers, a false start offered for deletion, and
//! processing on stop (cycle 1 QA, B1, B2, B4, B5, B9). The prototype's capture runs 20 times
//! faster than the clock and lasts at least a minute; a real-time capture makes short recordings.

mod support;

use greffier::{App, Page};
use greffier_core::fake::{FakeCapture, FakeTrash};
use greffier_core::{Greffier, MeetingId, Stage, Status, demo};
use rune::Runtime;
use support::*;

/// The meeting the last recording made: the most recent one.
fn last_recorded(rt: &mut Runtime<App>) -> MeetingId {
    state(rt, |s| s.core.meetings()[0].id)
}

#[test]
fn the_record_button_records_while_time_passes_and_the_meeting_opens_at_stop() {
    let mut rt = app_on(demo_core());
    let before = state(&mut rt, |s| s.core.meetings().len());

    tap(&mut rt, "record");
    state(&mut rt, |s| {
        assert!(s.page == Page::Recording);
        let r = s.core.recording().expect("recording");
        assert_eq!(r.sources, s.core.settings().sources, "the sources of the settings are recorded");
    });
    wait(&mut rt, 0.5);
    let early = state(&mut rt, |s| s.core.recording().expect("recording").duration);
    wait(&mut rt, 0.5);
    let recorded = state(&mut rt, |s| s.core.recording().expect("recording").duration);
    assert!(early > 0.0 && recorded > early, "the duration runs: {early} then {recorded}");
    tap(&mut rt, "stop");

    let id = last_recorded(&mut rt);
    state(&mut rt, |s| {
        assert!(s.core.recording().is_none());
        assert_eq!(s.core.meetings().len(), before + 1);
        assert!(s.core.meeting(id).expect("the new meeting").duration >= recorded);
        assert!(s.page == Page::Meeting(id), "the new meeting opens");
    });
    assert!(shown(&rt, &format!("m{id}")), "it is in the sidebar");
}

#[test]
fn during_a_recording_the_sidebar_button_brings_its_page_back_then_stops_it() {
    let mut rt = app_on(demo_core());
    tap(&mut rt, "record");
    tap(&mut rt, "nav-home");

    tap(&mut rt, "record");
    state(&mut rt, |s| {
        assert!(s.page == Page::Recording);
        assert!(s.core.recording().is_some(), "away from its page, the button only shows the recording");
    });
    tap(&mut rt, "record");
    assert!(state(&mut rt, |s| s.core.recording().is_none()), "on its page, the button stops it");
}

#[test]
fn shift_command_r_starts_and_stops_the_recording_when_the_global_shortcut_is_not_registered() {
    let mut rt = app_on(demo_core());
    let before = state(&mut rt, |s| s.core.meetings().len());

    press(&mut rt, shift_cmd('r'));
    assert!(state(&mut rt, |s| s.core.recording().is_some()));
    wait(&mut rt, 0.5);
    press(&mut rt, shift_cmd('r'));

    state(&mut rt, |s| {
        assert!(s.core.recording().is_none());
        assert_eq!(s.core.meetings().len(), before + 1);
    });
}

#[test]
fn markers_placed_with_the_button_and_command_m_reach_the_meeting() {
    let mut rt = app_on(demo_core());
    tap(&mut rt, "record");
    wait(&mut rt, 0.5);

    tap(&mut rt, "marker");
    wait(&mut rt, 0.5);
    press(&mut rt, cmd('m'));
    let placed = state(&mut rt, |s| s.core.recording().expect("recording").markers.clone());
    assert_eq!(placed.len(), 2, "one marker per gesture: {placed:?}");
    tap(&mut rt, "stop");

    let id = last_recorded(&mut rt);
    let kept: Vec<f64> = state(&mut rt, |s| s.core.meeting(id).expect("the meeting").markers.iter().map(|m| m.instant).collect());
    assert_eq!(kept.len(), placed.len());
    for (k, p) in kept.iter().zip(&placed) {
        assert!((k - p).abs() < 0.01, "kept at {k}, placed at {p}");
    }
}

/// A capture in real time with no minimum duration, and this trash: a recording stopped within
/// seconds is a false start.
fn real_time_recording(trash: &FakeTrash) -> Runtime<App> {
    let trash = trash.clone();
    app_on(Greffier::with_meetings(
        temp_folder(),
        adapters_with(|a| {
            a.capture = Box::new(FakeCapture::new(1.0, 0.0));
            a.trash = Box::new(trash);
        }),
        demo::meetings(),
    ))
}

#[test]
fn a_recording_under_a_minute_is_offered_for_deletion_and_deleting_trashes_it() {
    let trash = FakeTrash::new();
    let mut rt = real_time_recording(&trash);
    tap(&mut rt, "record");
    wait(&mut rt, 2.0);
    tap(&mut rt, "stop");

    let id = last_recorded(&mut rt);
    let folder = state(&mut rt, |s| {
        assert_eq!(s.short_recordings, [id], "the question is asked about it");
        assert_eq!(s.core.meeting(id).expect("the meeting").status, Status::ToProcess, "not processed before the answer");
        s.core.meeting_folder(id).expect("its folder")
    });
    wait_for_motion(&mut rt);
    tap(&mut rt, "short-delete");

    assert_eq!(trash.trashed(), [folder]);
    state(&mut rt, |s| {
        assert!(s.short_recordings.is_empty());
        assert!(s.core.meeting(id).is_none());
        assert!(s.page == Page::Overview);
    });
    assert!(!shown(&rt, &format!("m{id}")));
}

#[test]
fn a_short_recording_kept_is_processed_like_any_other() {
    let trash = FakeTrash::new();
    let mut rt = real_time_recording(&trash);
    tap(&mut rt, "record");
    wait(&mut rt, 2.0);
    tap(&mut rt, "stop");
    let id = last_recorded(&mut rt);
    wait_for_motion(&mut rt);

    tap(&mut rt, "short-keep");

    assert!(trash.trashed().is_empty());
    state(&mut rt, |s| {
        assert!(s.short_recordings.is_empty());
        assert_eq!(queued_at(&s.core.meeting(id).expect("kept").status), Some(Stage::Transcription), "processing on stop is on");
    });
}

#[test]
fn with_processing_on_stop_turned_off_the_meeting_waits_for_its_process_button() {
    let mut rt = app_on(demo_core());
    tap(&mut rt, "record");
    wait_for_motion(&mut rt);
    tap(&mut rt, "process-on-stop");
    assert!(!state(&mut rt, |s| s.core.settings().process_on_stop));
    wait(&mut rt, 0.5);
    tap(&mut rt, "stop");

    let id = last_recorded(&mut rt);
    assert_eq!(state(&mut rt, |s| s.core.meeting(id).expect("the meeting").status.clone()), Status::ToProcess);
    wait_for_motion(&mut rt);
    tap(&mut rt, "process");

    assert_eq!(state(&mut rt, |s| queued_at(&s.core.meeting(id).expect("the meeting").status)), Some(Stage::Transcription));
}
