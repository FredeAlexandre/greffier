//! The meetings library: the sidebar's periods, the ⌘K search, the right-click menu on a meeting,
//! deleting a meeting and changing its title (cycle 1 QA, G1 to G5).

mod support;

use greffier::{App, Page, Period, Tab, period};
use greffier_core::fake::FakeTrash;
use greffier_core::search::{Location, SearchResult};
use greffier_core::{DateTime, Greffier, MeetingId, Stage, Status, demo};
use rune::{Key, Runtime};
use support::*;

// --- G1: the sidebar's periods ------------------------------------------------------------------

#[test]
fn meetings_are_grouped_by_day_then_by_week_starting_on_monday() {
    let wednesday = DateTime::new(2026, 11, 18, 10, 0);
    let on = |day: u8| DateTime::new(2026, 11, day, 9, 0);

    assert_eq!(period(on(18), wednesday), Period::Today);
    assert_eq!(period(on(17), wednesday), Period::Yesterday);
    assert_eq!(period(on(16), wednesday), Period::ThisWeek, "Monday is this week");
    assert_eq!(period(on(15), wednesday), Period::LastWeek, "Sunday is last week");
    assert_eq!(period(on(9), wednesday), Period::LastWeek, "last week's Monday");
    assert_eq!(period(on(8), wednesday), Period::Earlier);
}

#[test]
fn on_a_monday_yesterday_is_yesterday_not_last_week() {
    let monday = DateTime::new(2026, 11, 16, 10, 0);
    let on = |day: u8| DateTime::new(2026, 11, day, 9, 0);

    assert_eq!(period(on(15), monday), Period::Yesterday);
    assert_eq!(period(on(9), monday), Period::LastWeek);
    assert_eq!(period(on(8), monday), Period::Earlier);
}

#[test]
fn each_meeting_of_the_sidebar_sits_in_the_section_of_its_period() {
    let mut rt = app_on(demo_core());
    let expected: Vec<(MeetingId, usize)> = state(&mut rt, |s| {
        let today = s.core.today();
        s.core.meetings().iter().map(|m| (m.id, Period::ALL.iter().position(|p| *p == period(m.start, today)).expect("a period"))).collect()
    });
    let sections: Vec<usize> = expected.iter().map(|(_, p)| *p).collect();
    assert!(sections.iter().any(|p| *p != sections[0]), "the demo spans several periods");

    let elements = rt.elements();
    for (id, section) in expected {
        let row = elements.iter().find(|e| e.key.as_deref() == Some(&format!("m{id}"))).unwrap_or_else(|| panic!("meeting {id} in the sidebar"));
        assert!(row.path.contains(&format!("/sec{section}/")), "meeting {id} belongs to section {section}: {}", row.path);
    }
}

// --- G2: the ⌘K search --------------------------------------------------------------------------

/// A word of the meetings whose first search result is found at `location`, at a moment after the
/// meeting's start, with that result.
fn a_word_found_first_in(core: &Greffier, location: Location) -> (String, SearchResult) {
    let texts: Vec<String> = core
        .meetings()
        .iter()
        .flat_map(|m| match location {
            Location::Transcription => m.utterances.iter().map(|u| u.text.clone()).collect::<Vec<_>>(),
            _ => vec![m.summary.clone()],
        })
        .collect();
    texts
        .iter()
        .flat_map(|t| t.split(|c: char| !c.is_alphabetic()))
        .filter(|w| w.chars().count() >= 7)
        .find_map(|w| {
            let first = core.search(w).into_iter().next()?;
            (first.location == location && first.instant.is_some_and(|t| t > 0.0)).then(|| (w.to_string(), first))
        })
        .unwrap_or_else(|| panic!("a word found first in {location:?}"))
}

#[test]
fn return_in_the_search_opens_the_meeting_at_the_utterance_found() {
    let mut rt = app_on(demo_core());
    let (word, found) = state(&mut rt, |s| a_word_found_first_in(&s.core, Location::Transcription));

    press(&mut rt, cmd('k'));
    assert!(state(&mut rt, |s| s.search.is_some()), "⌘K opens the search");
    type_text(&mut rt, &word);
    press(&mut rt, named(Key::Enter));

    state(&mut rt, |s| {
        assert!(s.search.is_none(), "the search closes");
        assert!(s.page == Page::Meeting(found.meeting));
        assert!(s.tab == Tab::Transcript);
        assert_eq!(Some(s.pos), found.instant);
    });
}

#[test]
fn a_click_on_a_summary_result_shows_the_summary_at_its_topic() {
    let mut rt = app_on(demo_core());
    let (word, found) = state(&mut rt, |s| a_word_found_first_in(&s.core, Location::Summary));
    // Given its meeting open on another tab.
    tap(&mut rt, &format!("m{}", found.meeting));
    press(&mut rt, cmd('2'));
    assert!(state(&mut rt, |s| s.tab == Tab::Transcript));

    press(&mut rt, cmd('k'));
    type_text(&mut rt, &word);
    wait_for_motion(&mut rt);
    tap(&mut rt, "hit0");

    state(&mut rt, |s| {
        assert!(s.search.is_none());
        assert!(s.page == Page::Meeting(found.meeting));
        assert!(s.tab == Tab::Summary);
        assert_eq!(Some(s.pos), found.instant);
    });
}

// --- G3: the right-click menu -------------------------------------------------------------------

/// The most recent processed meeting with a transcription and a summary.
fn a_done_meeting(rt: &mut Runtime<App>) -> MeetingId {
    state(rt, |s| s.core.meetings().iter().find(|m| m.status == Status::Done && !m.utterances.is_empty() && !m.summary.is_empty()).expect("a processed meeting").id)
}

#[test]
fn open_in_the_right_click_menu_opens_the_meeting() {
    let mut rt = app_on(demo_core());
    let id = a_done_meeting(&mut rt);

    secondary_tap(&mut rt, &format!("m{id}"));
    assert!(state(&mut rt, |s| s.menu.is_some_and(|(m, _)| m == id)), "the menu is about that meeting");
    choose(&mut rt, &format!("menu{id}"), "Ouvrir");

    state(&mut rt, |s| {
        assert!(s.page == Page::Meeting(id));
        assert!(s.tab == Tab::Summary);
        assert!(s.menu.is_none());
    });
}

#[test]
fn read_the_transcript_in_the_right_click_menu_opens_the_transcript_tab() {
    let mut rt = app_on(demo_core());
    let id = a_done_meeting(&mut rt);

    secondary_tap(&mut rt, &format!("m{id}"));
    choose(&mut rt, &format!("menu{id}"), "Lire la transcription");

    state(&mut rt, |s| {
        assert!(s.page == Page::Meeting(id));
        assert!(s.tab == Tab::Transcript);
    });
}

#[test]
fn redo_the_summary_in_the_right_click_menu_queues_the_summary_only() {
    let mut rt = app_on(demo_core());
    let id = a_done_meeting(&mut rt);

    secondary_tap(&mut rt, &format!("m{id}"));
    choose(&mut rt, &format!("menu{id}"), "Refaire le bilan");

    state(&mut rt, |s| {
        let m = s.core.meeting(id).expect("the meeting");
        assert_eq!(queued_at(&m.status), Some(Stage::Summary));
        assert!(s.menu.is_none());
    });
}

#[test]
fn process_in_the_right_click_menu_queues_a_meeting_to_process() {
    let mut rt = app_on(demo_core());
    let id = state(&mut rt, |s| s.core.meetings().iter().find(|m| m.status == Status::ToProcess).expect("a meeting to process").id);

    secondary_tap(&mut rt, &format!("m{id}"));
    choose(&mut rt, &format!("menu{id}"), "Transcrire et faire le bilan");

    assert_eq!(state(&mut rt, |s| queued_at(&s.core.meeting(id).expect("the meeting").status)), Some(Stage::Transcription));
}

#[test]
fn copy_the_summary_puts_the_summary_markdown_on_the_clipboard() {
    let mut rt = app_on(demo_core());
    let clipboard = rune::clipboard::memory();
    rt.use_clipboard(&clipboard);
    let id = a_done_meeting(&mut rt);
    let summary = state(&mut rt, |s| s.core.meeting(id).expect("the meeting").summary.clone());

    secondary_tap(&mut rt, &format!("m{id}"));
    choose(&mut rt, &format!("menu{id}"), "Copier le bilan en Markdown");

    assert_eq!(clipboard.text().as_deref().map(String::as_str), Some(summary.as_str()));
    state(&mut rt, |s| {
        assert!(s.toast.is_some(), "a toast says it is copied");
        assert!(s.menu.is_none());
    });
}

// --- G4: deleting a meeting ---------------------------------------------------------------------

/// The core tests' sample meetings, on disk, with this trash.
fn library_on_disk(trash: &FakeTrash) -> Runtime<App> {
    let trash = trash.clone();
    app_on(Greffier::open(fixtures_folder(), adapters_with(|a| a.trash = Box::new(trash))))
}

#[test]
fn deleting_from_the_right_click_menu_sends_the_folder_to_the_trash_once_confirmed() {
    let trash = FakeTrash::new();
    let mut rt = library_on_disk(&trash);
    let (id, folder) = state(&mut rt, |s| {
        let m = &s.core.meetings()[0];
        (m.id, s.core.meeting_folder(m.id).expect("its folder"))
    });
    tap(&mut rt, &format!("m{id}"));

    secondary_tap(&mut rt, &format!("m{id}"));
    choose(&mut rt, &format!("menu{id}"), "Supprimer…");
    assert_eq!(state(&mut rt, |s| s.confirm_delete), Some(id), "the deletion is asked first");
    assert!(trash.trashed().is_empty(), "nothing is trashed before the answer");
    wait_for_motion(&mut rt);
    tap(&mut rt, "del-ok");

    assert_eq!(trash.trashed(), [folder]);
    state(&mut rt, |s| {
        assert!(s.core.meeting(id).is_none());
        assert!(s.confirm_delete.is_none());
        assert!(s.page == Page::Overview, "the deleted meeting's page is left");
    });
    assert!(!shown(&rt, &format!("m{id}")), "gone from the sidebar");
}

#[test]
fn command_backspace_asks_before_deleting_the_open_meeting_and_cancel_keeps_it() {
    let trash = FakeTrash::new();
    let mut rt = library_on_disk(&trash);
    let id = state(&mut rt, |s| s.core.meetings()[0].id);
    tap(&mut rt, &format!("m{id}"));

    press(&mut rt, rune::KeyEvent { key: Key::Backspace, mods: rune::Mods { cmd: true, ..rune::Mods::default() } });
    assert_eq!(state(&mut rt, |s| s.confirm_delete), Some(id));
    wait_for_motion(&mut rt);
    tap(&mut rt, "del-cancel");

    assert!(trash.trashed().is_empty());
    state(&mut rt, |s| {
        assert!(s.confirm_delete.is_none());
        assert!(s.core.meeting(id).is_some());
        assert!(s.page == Page::Meeting(id));
    });
}

// --- G5: changing the title ---------------------------------------------------------------------

#[test]
fn a_click_on_the_title_renames_the_meeting_with_what_is_typed() {
    let mut rt = app_on(Greffier::open(fixtures_folder(), demo::adapters()));
    let id = state(&mut rt, |s| s.core.meetings()[0].id);
    tap(&mut rt, &format!("m{id}"));

    wait_for_motion(&mut rt);

    tap(&mut rt, "meeting-title");
    assert!(state(&mut rt, |s| s.rename.as_ref().is_some_and(|r| r.meeting == id)), "the title window opens on this meeting");
    wait_for_motion(&mut rt);
    press(&mut rt, cmd('a'));
    type_text(&mut rt, "Point budget");
    tap(&mut rt, "rename-ok");

    state(&mut rt, |s| {
        assert!(s.rename.is_none());
        assert_eq!(s.core.meeting(id).expect("the meeting").title, "Point budget");
    });
}

