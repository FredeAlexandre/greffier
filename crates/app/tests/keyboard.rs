//! Who gets the keyboard on a meeting's page: after a click in the sidebar or a tab change,
//! Space and the arrows drive playback (cycle 1 QA, fixed in `fcd0680`).

mod support;

use greffier::{Page, Tab};
use greffier_core::{MeetingId, Status};
use rune::Key;
use support::*;

/// The first processed meeting of the prototype's data.
fn a_done_meeting(rt: &mut rune::Runtime<greffier::App>) -> MeetingId {
    state(rt, |s| s.core.meetings().iter().find(|m| m.status == Status::Done && !m.utterances.is_empty()).expect("a processed meeting").id)
}

#[test]
fn space_after_a_click_in_the_sidebar_plays_the_meeting() {
    let mut rt = app_on(demo_core());
    let id = a_done_meeting(&mut rt);

    tap(&mut rt, &format!("m{id}"));
    press(&mut rt, key(' '));
    wait(&mut rt, 1.0);

    state(&mut rt, |s| {
        assert!(s.page == Page::Meeting(id));
        assert!(s.playing, "Space plays the meeting just opened from the sidebar");
        assert!(s.pos > 0.5, "playback moves forward: {}", s.pos);
    });
}

#[test]
fn space_after_leaving_the_speaker_name_field_with_a_tab_change_plays_without_going_full_screen() {
    let mut rt = app_on(demo_core());
    let id = a_done_meeting(&mut rt);
    tap(&mut rt, &format!("m{id}"));
    press(&mut rt, cmd('4'));
    rt.update(|s, _| s.start_naming(1));
    settle(&mut rt);

    press(&mut rt, cmd('2'));
    press(&mut rt, key(' '));

    state(&mut rt, |s| {
        assert!(s.tab == Tab::Transcript);
        assert!(s.playing);
        assert!(!s.fullscreen);
    });
}

#[test]
fn the_arrows_move_playback_ten_seconds() {
    let mut rt = app_on(demo_core());
    let id = a_done_meeting(&mut rt);
    tap(&mut rt, &format!("m{id}"));
    rt.update(|s, _| s.seek(60.0));

    press(&mut rt, named(Key::Right));
    assert!((state(&mut rt, |s| s.pos) - 70.0).abs() < 0.01);
    press(&mut rt, named(Key::Left));
    press(&mut rt, named(Key::Left));
    assert!((state(&mut rt, |s| s.pos) - 50.0).abs() < 0.01);
}

/// ⌘2 then Space (#24), with focus first left on a control of the page: the speed
/// picker keeps it after a click, and would take Space.
#[test]
fn space_after_a_tab_change_plays_even_when_a_control_of_the_page_had_focus() {
    let mut rt = app_on(demo_core());
    let id = a_done_meeting(&mut rt);
    tap(&mut rt, &format!("m{id}"));
    wait(&mut rt, 0.5);
    tap(&mut rt, "speed/segment1");

    press(&mut rt, cmd('2'));
    press(&mut rt, key(' '));

    state(&mut rt, |s| {
        assert!(s.tab == Tab::Transcript);
        assert!(s.playing, "Space plays");
        assert_eq!(s.speed, 1, "the speed picker did not take Space");
    });
}

/// A decision starts playback, then on the Transcript tab Space pauses
/// it where it is.
#[test]
fn space_pauses_the_playback_a_decision_started() {
    let mut rt = app_on(demo_core());
    let id = state(&mut rt, |s| s.core.meetings().iter().find(|m| m.status == Status::Done && m.decisions.len() > 2).expect("a meeting with decisions").id);
    tap(&mut rt, &format!("m{id}"));
    wait(&mut rt, 0.5);
    let decision = state(&mut rt, |s| s.meeting().expect("open").decisions[2].instant);

    tap(&mut rt, "dec2");
    assert!(state(&mut rt, |s| s.playing));
    press(&mut rt, cmd('2'));
    wait(&mut rt, 1.0);
    press(&mut rt, key(' '));
    let paused_at = state(&mut rt, |s| s.pos);
    wait(&mut rt, 1.0);

    state(&mut rt, |s| {
        assert!(s.tab == Tab::Transcript);
        assert!(!s.playing, "paused");
        assert!(s.pos > decision && s.pos == paused_at, "paused where it was: {} after {decision}", s.pos);
    });
}
