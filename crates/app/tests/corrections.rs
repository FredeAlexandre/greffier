//! Correcting a meeting from its page (cycle 1 QA, E1 to E4): naming, merging and splitting
//! speakers, settling the passages to check with the keyboard, correcting an utterance. The core's
//! rules (what a merge or a resolution writes) are tested in `crates/core/tests`; here, each
//! gesture must reach the core as the right command, on the right speaker, passage or utterance.

mod support;

use greffier::{App, Tab, Target};
use greffier_core::{Meeting, MeetingId, PassageStatus, Status};
use rune::{Key, Runtime};
use support::*;

/// The prototype's meeting with passages to check and a suggested merge.
fn meeting_to_correct(rt: &mut Runtime<App>) -> MeetingId {
    let id = state(rt, |s| {
        s.core
            .meetings()
            .iter()
            .find(|m| m.status == Status::Done && m.pending_passages() >= 3 && m.speakers.iter().any(|p| p.suggested_merge.is_some()))
            .expect("a meeting with passages to check and a suggested merge")
            .id
    });
    tap(rt, &format!("m{id}"));
    // The page comes in with a short animation: let it end, as the user does before clicking.
    wait(rt, 0.5);
    id
}

fn meeting(rt: &mut Runtime<App>) -> Meeting {
    state(rt, |s| s.meeting().expect("a meeting's page").clone())
}

/// ⌘4, then the page scrolled to `key`.
fn on_speakers_tab(rt: &mut Runtime<App>, key: &str) {
    press(rt, cmd('4'));
    scroll_page_to(rt, key);
}

/// ⌘A then `text`: replaces the focused field's text.
fn retype(rt: &mut Runtime<App>, text: &str) {
    press(rt, cmd('a'));
    type_text(rt, text);
}

// --- E1: naming speakers ----------------------------------------------------------------------

#[test]
fn confirming_the_proposed_name_gives_it_to_that_speaker() {
    let mut rt = app_on(demo_core());
    meeting_to_correct(&mut rt);
    let m = meeting(&mut rt);
    let i = (0..m.speakers.len()).rev().find(|&i| m.speakers[i].name.is_none() && m.speakers[i].suggested_merge.is_none() && m.speakers[i].proposal.is_some()).expect("an unnamed speaker with a proposal");
    on_speakers_tab(&mut rt, &format!("spk{i}-confirm"));

    tap(&mut rt, &format!("spk{i}-confirm"));

    let after = meeting(&mut rt);
    assert_eq!(after.speakers[i].name, m.speakers[i].proposal);
    assert!((0..m.speakers.len()).filter(|&j| j != i).all(|j| after.speakers[j].name == m.speakers[j].name), "the other speakers keep their names");
}

#[test]
fn correcting_a_name_gives_the_typed_name_on_return() {
    let mut rt = app_on(demo_core());
    meeting_to_correct(&mut rt);
    let i = 1;
    on_speakers_tab(&mut rt, &format!("spk{i}-rename"));

    tap(&mut rt, &format!("spk{i}-rename"));
    retype(&mut rt, "Samuel");
    press(&mut rt, named(Key::Enter));

    assert_eq!(meeting(&mut rt).speakers[i].name.as_deref(), Some("Samuel"));
    assert!(state(&mut rt, |s| s.picking.is_none()), "the field closes");
}

// --- E2: merging and splitting ----------------------------------------------------------------

#[test]
fn accepting_the_suggested_merge_merges_that_speaker_into_the_suggested_voice() {
    let mut rt = app_on(demo_core());
    meeting_to_correct(&mut rt);
    let m = meeting(&mut rt);
    let (i, into) = m.speakers.iter().enumerate().find_map(|(i, p)| Some((i, p.suggested_merge?))).expect("a suggested merge");
    on_speakers_tab(&mut rt, &format!("spk{i}-merge-suggested"));

    tap(&mut rt, &format!("spk{i}-merge-suggested"));

    assert_eq!(meeting(&mut rt).speakers[i].merged_into, Some(into));
}

#[test]
fn merging_by_hand_merges_the_speaker_into_the_voice_picked() {
    let mut rt = app_on(demo_core());
    meeting_to_correct(&mut rt);
    let m = meeting(&mut rt);
    let (i, into) = (m.speakers.len() - 1, 1);
    on_speakers_tab(&mut rt, &format!("spk{i}-merge"));

    tap(&mut rt, &format!("spk{i}-merge"));
    scroll_page_to(&mut rt, &format!("spk{i}-into{into}"));
    tap(&mut rt, &format!("spk{i}-into{into}"));

    let after = meeting(&mut rt);
    assert_eq!(after.speakers[i].merged_into, Some(into));
    assert_eq!(after.speakers[into].merged_into, None);
}

#[test]
fn splitting_gives_the_merged_speaker_back_its_own_voice() {
    let mut rt = app_on(demo_core());
    let id = meeting_to_correct(&mut rt);
    // Given two merges into the same voice (the merge gestures are tested above).
    rt.update(|s, _| {
        s.command(greffier_core::Command::MergeSpeakers { meeting: id, speaker: 2, into: 0 });
        s.command(greffier_core::Command::MergeSpeakers { meeting: id, speaker: 4, into: 0 });
    });
    on_speakers_tab(&mut rt, "spk0-split4");

    tap(&mut rt, "spk0-split4");

    let after = meeting(&mut rt);
    assert_eq!(after.speakers[4].merged_into, None, "split");
    assert_eq!(after.speakers[2].merged_into, Some(0), "the other merge stays");
}

// --- E3: the passages to check, with the keyboard ---------------------------------------------

fn pending(m: &Meeting) -> Vec<usize> {
    (0..m.passages.len()).filter(|&i| m.passages[i].status == PassageStatus::Pending).collect()
}

/// The meeting's page on « À vérifier » (⌘3).
fn on_passages(rt: &mut Runtime<App>) -> Meeting {
    meeting_to_correct(rt);
    press(rt, cmd('3'));
    meeting(rt)
}

#[test]
fn down_and_up_move_from_one_pending_passage_to_the_next() {
    let mut rt = app_on(demo_core());
    let m = on_passages(&mut rt);
    let p = pending(&m);

    press(&mut rt, named(Key::Down));
    press(&mut rt, named(Key::Down));
    assert_eq!(state(&mut rt, |s| s.doubt), p[2]);
    press(&mut rt, named(Key::Up));
    assert_eq!(state(&mut rt, |s| s.doubt), p[1]);
}

#[test]
fn e_plays_the_selected_passage_excerpt_and_stops_at_its_end() {
    let mut rt = app_on(demo_core());
    let m = on_passages(&mut rt);
    let i = pending(&m)[1];
    let (from, to) = m.excerpt_to_listen(i).expect("an excerpt");
    press(&mut rt, named(Key::Down));

    press(&mut rt, key('e'));
    state(&mut rt, |s| {
        assert_eq!(s.pos, from);
        assert!(s.playing);
    });
    wait(&mut rt, to - from + 1.0);

    state(&mut rt, |s| {
        assert!(!s.playing, "stopped at the end of the excerpt");
        assert!((s.pos - to).abs() < 0.2, "at {} instead of {to}", s.pos);
    });
}

#[test]
fn a_accepts_the_hypothesis_of_the_selected_passage_and_moves_to_the_next() {
    let mut rt = app_on(demo_core());
    let m = on_passages(&mut rt);
    let p: Vec<usize> = pending(&m).into_iter().filter(|&i| m.passages[i].hypothesis.is_some()).collect();
    while state(&mut rt, |s| s.doubt) != p[1] {
        press(&mut rt, named(Key::Down));
    }

    press(&mut rt, key('a'));

    let after = meeting(&mut rt);
    assert_eq!(after.passages[p[1]].status, PassageStatus::HypothesisAccepted);
    assert_eq!(pending(&after).len(), pending(&m).len() - 1, "only that passage is settled");
    assert_eq!(state(&mut rt, |s| s.doubt), p[1] + 1);
}

#[test]
fn g_keeps_the_original_text_of_the_selected_passage() {
    let mut rt = app_on(demo_core());
    let m = on_passages(&mut rt);
    let i = pending(&m)[0];

    press(&mut rt, key('g'));

    assert_eq!(meeting(&mut rt).passages[i].status, PassageStatus::OriginalKept);
}

#[test]
fn c_then_typing_and_return_saves_the_typed_correction_of_the_selected_passage() {
    let mut rt = app_on(demo_core());
    let m = on_passages(&mut rt);
    let i = pending(&m)[1];
    press(&mut rt, named(Key::Down));

    press(&mut rt, key('c'));
    type_text(&mut rt, "push en prod");
    press(&mut rt, named(Key::Enter));

    assert_eq!(meeting(&mut rt).passages[i].status, PassageStatus::CorrectedManually("push en prod".into()));
    assert!(state(&mut rt, |s| s.correction.is_none()), "the field closes");
}

#[test]
fn annuler_on_a_settled_passage_puts_it_back_to_check() {
    let mut rt = app_on(demo_core());
    let m = on_passages(&mut rt);
    let i = pending(&m)[0];
    press(&mut rt, key('g'));
    scroll_page_to(&mut rt, &format!("dz{i}-reopen"));

    tap(&mut rt, &format!("dz{i}-reopen"));

    assert_eq!(meeting(&mut rt).passages[i].status, PassageStatus::Pending);
}

// --- E4: correcting an utterance in the Transcript tab ----------------------------------------

/// The meeting's page on the Transcript tab (⌘2), the transcript scrolled into the window, and
/// one of its first utterances that has no passage to check (those are settled in « À vérifier »).
fn on_transcript(rt: &mut Runtime<App>) -> (Meeting, usize) {
    meeting_to_correct(rt);
    press(rt, cmd('2'));
    let m = meeting(rt);
    let i = (1..m.utterances.len()).find(|&i| m.utterances[i].passages.is_empty()).expect("an utterance without passage to check");
    scroll_page_to(rt, &format!("u{i}"));
    (m, i)
}

#[test]
fn a_right_click_on_an_utterance_then_return_saves_its_corrected_text() {
    let mut rt = app_on(demo_core());
    let (m, i) = on_transcript(&mut rt);

    secondary_tap(&mut rt, &format!("u{i}"));
    type_text(&mut rt, " Merci.");
    press(&mut rt, named(Key::Enter));

    let after = meeting(&mut rt);
    assert_eq!(after.utterances[i].text, format!("{} Merci.", m.utterances[i].text));
    assert!(state(&mut rt, |s| s.correction.is_none()), "the field closes");
}

#[test]
fn c_opens_the_correction_of_the_utterance_being_played() {
    let mut rt = app_on(demo_core());
    let (m, i) = on_transcript(&mut rt);
    rt.update(|s, _| s.seek(m.utterances[i].start));
    press(&mut rt, key(' '));
    wait(&mut rt, 0.5);
    let current = state(&mut rt, |s| s.current).expect("an utterance being played");

    press(&mut rt, key('c'));

    state(&mut rt, |s| {
        assert!(s.tab == Tab::Transcript);
        assert_eq!(s.correction.as_ref().map(|c| c.target), Some(Target::Utterance(current)));
    });
}

#[test]
fn escape_abandons_the_correction_and_leaves_the_utterance_as_it_was() {
    let mut rt = app_on(demo_core());
    let (m, i) = on_transcript(&mut rt);
    secondary_tap(&mut rt, &format!("u{i}"));
    type_text(&mut rt, " brouillon");
    assert!(state(&mut rt, |s| s.correction.is_some()), "a correction is open");

    press(&mut rt, named(Key::Escape));

    assert!(state(&mut rt, |s| s.correction.is_none()));
    assert_eq!(meeting(&mut rt).utterances[i].text, m.utterances[i].text);
}
