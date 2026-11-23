//! A meeting's page (cycle 1 QA, D1 to D6): the summary first, playback and its controls, the
//! timeline, the summary's timestamps and checkboxes, the transcript following playback, the tabs.
//! Expected moments are read from the core (a topic's start, a decision's instant), never copied
//! from the demo data.

mod support;

use greffier::{App, Page, Tab};
use greffier_core::{Meeting, MeetingId, Role, Status};
use rune::Runtime;
use support::*;

/// The prototype's meeting with the most to look at: topics, decisions, a marker, several
/// speakers, passages to check.
fn richest_meeting(rt: &mut Runtime<App>) -> MeetingId {
    state(rt, |s| {
        s.core
            .meetings()
            .iter()
            .filter(|m| m.status == Status::Done)
            .max_by_key(|m| (m.markers.len().min(1), m.decisions.len(), m.active_speakers().count()))
            .expect("a processed meeting")
            .id
    })
}

/// The open meeting, as the core has it.
fn meeting(rt: &mut Runtime<App>) -> Meeting {
    state(rt, |s| s.meeting().expect("a meeting's page").clone())
}

fn open_richest(rt: &mut Runtime<App>) -> MeetingId {
    let id = richest_meeting(rt);
    tap(rt, &format!("m{id}"));
    // The page comes in with a short animation: let it end, as the user does before clicking.
    wait(rt, 0.5);
    id
}

fn pos(rt: &mut Runtime<App>) -> f64 {
    state(rt, |s| s.pos)
}

fn assert_near(actual: f64, expected: f64, tolerance: f64, what: &str) {
    assert!((actual - expected).abs() <= tolerance, "{what}: {actual} instead of {expected} (± {tolerance})");
}

// --- D1: the page of a meeting --------------------------------------------------------------

#[test]
fn a_meeting_opened_from_the_sidebar_shows_its_summary_first_even_after_another_tab_elsewhere() {
    let mut rt = app_on(demo_core());
    let id = open_richest(&mut rt);
    let other = state(&mut rt, |s| s.core.meetings().iter().find(|m| m.id != id && m.status == Status::Done).expect("another processed meeting").id);
    press(&mut rt, cmd('2'));

    tap(&mut rt, &format!("m{other}"));

    state(&mut rt, |s| {
        assert!(s.page == Page::Meeting(other));
        assert!(s.tab == Tab::Summary, "the summary comes first");
    });
}

#[test]
fn during_playback_the_utterance_being_said_becomes_the_current_one() {
    let mut rt = app_on(demo_core());
    open_richest(&mut rt);
    let m = meeting(&mut rt);
    let (i, u) = m.utterances.iter().enumerate().find(|(_, u)| u.start > 60.0).expect("an utterance after a minute");
    rt.update(|s, _| s.seek(u.start - 1.0));
    let before = state(&mut rt, |s| s.current);

    press(&mut rt, key(' '));
    wait(&mut rt, 1.5);

    assert_ne!(before, Some(i));
    assert_eq!(state(&mut rt, |s| s.current), Some(i), "the caption shows the utterance playback has reached");
}

// --- D2: the timeline -----------------------------------------------------------------------

#[test]
fn the_timeline_has_one_track_per_speaker_still_shown_after_a_merge() {
    let mut rt = app_on(demo_core());
    let id = open_richest(&mut rt);
    // Given two speakers merged (the Speakers tab's wiring is tested in corrections.rs).
    rt.update(|s, _| s.command(greffier_core::Command::MergeSpeakers { meeting: id, speaker: 2, into: 0 }));
    settle(&mut rt);

    let m = meeting(&mut rt);
    let active: Vec<usize> = m.active_speakers().collect();
    assert!(active.len() < m.speakers.len(), "a speaker is merged");
    for i in 0..m.speakers.len() {
        assert_eq!(shown(&rt, &format!("lane{i}")), active.contains(&i), "track of speaker {i}");
    }
}

#[test]
fn the_timeline_shows_each_topic_decision_and_marker() {
    let mut rt = app_on(demo_core());
    open_richest(&mut rt);
    let m = meeting(&mut rt);
    assert!(!m.topics.is_empty() && !m.decisions.is_empty() && !m.markers.is_empty());

    for (prefix, n) in [("ch", m.topics.len()), ("dec", m.decisions.len()), ("mk", m.markers.len())] {
        for i in 0..n {
            assert!(shown(&rt, &format!("{prefix}{i}")), "{prefix}{i} on the timeline");
        }
        assert!(!shown(&rt, &format!("{prefix}{n}")), "no extra {prefix}");
    }
}

#[test]
fn a_click_then_a_drag_on_the_tracks_move_playback_under_the_pointer() {
    let mut rt = app_on(demo_core());
    open_richest(&mut rt);
    let m = meeting(&mut rt);
    // The ◆ of a decision sits at its moment on the same time axis as the tracks: pressing a
    // track straight above it aims at that moment.
    let last = m.decisions.len() - 1;
    let (dx, _) = on_screen(&rt, "dec0");
    let (mx, _) = on_screen(&rt, &format!("dec{last}"));
    let (_, y) = on_screen(&rt, "lane0");
    let tolerance = m.duration / 400.0;

    rt.pointer_move(dx, y);
    rt.pointer_down(dx, y);
    settle(&mut rt);
    assert_near(pos(&mut rt), m.decisions[0].instant, tolerance, "pressed above the decision");

    rt.pointer_move(mx, y);
    settle(&mut rt);
    assert_near(pos(&mut rt), m.decisions[last].instant, tolerance, "dragged above the last decision");
    rt.pointer_up(mx, y);
}

#[test]
fn a_click_on_a_topic_plays_from_its_start() {
    let mut rt = app_on(demo_core());
    open_richest(&mut rt);
    let m = meeting(&mut rt);
    let i = m.topics.len() / 2;

    tap(&mut rt, &format!("ch{i}"));

    state(&mut rt, |s| {
        assert_eq!(s.pos, m.topics[i].start);
        assert!(s.playing);
    });
}

#[test]
fn a_click_on_a_decision_plays_from_its_moment() {
    let mut rt = app_on(demo_core());
    open_richest(&mut rt);
    let m = meeting(&mut rt);
    let i = m.decisions.len() - 1;

    tap(&mut rt, &format!("dec{i}"));

    state(&mut rt, |s| {
        assert_eq!(s.pos, m.decisions[i].instant);
        assert!(s.playing);
    });
}

// --- D3: playback controls ------------------------------------------------------------------

#[test]
fn the_ten_second_buttons_move_playback_back_and_forth() {
    let mut rt = app_on(demo_core());
    open_richest(&mut rt);
    rt.update(|s, _| s.seek(120.0));

    tap(&mut rt, "fwd10");
    assert_near(pos(&mut rt), 130.0, 0.01, "after +10 s");
    tap(&mut rt, "back10");
    tap(&mut rt, "back10");
    assert_near(pos(&mut rt), 110.0, 0.01, "after twice −10 s");
}

#[test]
fn at_one_and_a_half_then_twice_the_speed_playback_covers_that_much_more_time() {
    let mut rt = app_on(demo_core());
    open_richest(&mut rt);
    press(&mut rt, key(' '));
    tap(&mut rt, "speed/segment1");

    let start = pos(&mut rt);
    wait(&mut rt, 2.0);
    assert_near(pos(&mut rt) - start, 3.0, 0.15, "2 s at 1.5×");

    tap(&mut rt, "speed/segment2");
    let start = pos(&mut rt);
    wait(&mut rt, 2.0);
    assert_near(pos(&mut rt) - start, 4.0, 0.15, "2 s at 2×");
}

// --- D4: the summary ------------------------------------------------------------------------

#[test]
fn a_timestamp_of_the_summary_plays_from_that_moment_and_brings_the_video_back_into_view() {
    let mut rt = app_on(demo_core());
    open_richest(&mut rt);
    let m = meeting(&mut rt);
    let decision = m.decisions.last().expect("a decision").instant;
    let link = format!("t:{}", decision as u64);
    scroll_page_to(&mut rt, &link);
    assert!(!shown_inside(&rt, "video-frame"), "the video has scrolled away");

    tap(&mut rt, &link);
    wait(&mut rt, 1.0);

    state(&mut rt, |s| {
        assert_near(s.pos, decision, 1.5, "playback position");
        assert!(s.playing);
    });
    assert!(shown_inside(&rt, "video-frame"), "the video is back in view");
}

/// The element's centre is inside the window.
fn shown_inside(rt: &Runtime<App>, key: &str) -> bool {
    rt.find(key).is_some_and(|(x, y)| (0.0..SIZE.0).contains(&x) && (0.0..SIZE.1).contains(&y))
}

#[test]
fn ticking_an_action_writes_its_checkbox_in_the_summary_file_and_nothing_else() {
    let mut rt = app_on(demo_core());
    let id = open_richest(&mut rt);
    // Given the summary on disk, as the agent wrote it.
    let (file, written) = state(&mut rt, |s| {
        let m = s.core.meeting(id).expect("the meeting");
        let folder = s.core.meeting_folder(id).expect("its folder");
        (folder.join(format!("{} - {}", m.folder_name, Role::Summary.suffix())), m.summary.clone())
    });
    std::fs::create_dir_all(file.parent().expect("folder")).expect("meeting folder");
    std::fs::write(&file, &written).expect("bilan.md");
    let n = 1;
    let checkbox = |rt: &Runtime<App>| {
        rt.elements().into_iter().filter(|e| !e.exiting && e.role == "checkbox" && e.path.contains("/summary-md/")).nth(n).map(|e| e.rect)
    };
    scroll_page_until(&mut rt, "second checkbox of the summary", |rt| checkbox(rt).map(|r| r[1] + r[3] / 2.0));

    let r = checkbox(&rt).expect("the checkbox");
    click_at(&mut rt, r[0] + 10.0, r[1] + r[3] / 2.0);

    let mut boxes = 0;
    let expected: String = written
        .split_inclusive('\n')
        .map(|line| {
            if !line.trim_start().starts_with("- [ ]") {
                return line.to_string();
            }
            boxes += 1;
            if boxes == n + 1 { line.replacen("- [ ]", "- [x]", 1) } else { line.to_string() }
        })
        .collect();
    assert_ne!(expected, written);
    assert_eq!(std::fs::read_to_string(&file).expect("bilan.md"), expected);
}

// --- D5: the transcript follows playback ----------------------------------------------------

#[test]
fn the_transcript_scrolls_to_the_utterance_being_played() {
    let mut rt = app_on(demo_core());
    open_richest(&mut rt);
    press(&mut rt, cmd('2'));
    scroll_page_to(&mut rt, "transcript-scroll");
    let m = meeting(&mut rt);
    let last = m.utterances.len() - 1;
    assert!(!shown_within(&rt, &format!("u{last}"), "transcript-scroll"), "the last utterance starts out of view");

    rt.update(|s, _| s.seek(m.utterances[last].start));
    press(&mut rt, key(' '));
    wait(&mut rt, 1.0);

    assert!(shown_within(&rt, &format!("u{last}"), "transcript-scroll"), "the transcript has followed playback");
}

/// The element's centre is inside the visible part of `area`.
fn shown_within(rt: &Runtime<App>, key: &str, area: &str) -> bool {
    let a = rect(rt, area);
    rt.find(key).is_some_and(|(x, y)| (a[0]..a[0] + a[2]).contains(&x) && (a[1]..a[1] + a[3]).contains(&y))
}

#[test]
fn a_click_on_an_utterance_moves_playback_to_its_start() {
    let mut rt = app_on(demo_core());
    open_richest(&mut rt);
    press(&mut rt, cmd('2'));
    scroll_page_to(&mut rt, "u3");
    let m = meeting(&mut rt);

    tap(&mut rt, "u3");

    state(&mut rt, |s| {
        assert_eq!(s.pos, m.utterances[3].start);
        assert_eq!(s.current, Some(3));
    });
}

// --- D6: the tabs ---------------------------------------------------------------------------

#[test]
fn command_one_to_five_open_summary_transcript_to_check_speakers_and_info() {
    let mut rt = app_on(demo_core());
    open_richest(&mut rt);

    for (c, tab) in [('2', Tab::Transcript), ('3', Tab::Doubts), ('4', Tab::Speakers), ('5', Tab::Info), ('1', Tab::Summary)] {
        press(&mut rt, cmd(c));
        assert!(state(&mut rt, |s| s.tab == tab), "⌘{c}");
    }
}

