//! Regressions of the cycle 1 QA: each test replays a UI bug fixed during the QA as the user met
//! it, and fails with the fix taken out. The commit (and issue) of the fix is named on each test.

mod support;

use greffier::{App, Page, Tab};
use greffier_core::adapters::{Permission, PermissionState, Permissions};
use greffier_core::fake::FakeCapture;
use greffier_core::{Greffier, MeetingId, Status, demo};
use rune::{Key, Runtime, Shortcut};
use rune_kit::ShortcutRecorderState;
use support::*;

// --- The macOS permissions card ---------------------------------------------------------------

/// The prototype's meetings, macOS's permissions standing in `permissions` (the sources) and
/// `notifications`.
fn app_with_permissions(permissions: &MacPermissions, notifications: &FakeNotifications) -> Runtime<App> {
    let core = Greffier::with_meetings(temp_folder(), adapters_with(|a| a.capture = permissions.capture()), demo::meetings());
    let mut rt = app_on(core);
    // The notification permission is macOS's in the live app only: the test stands for macOS.
    let notifications = notifications.clone();
    rt.update(move |s, _| s.notifications = Some(Box::new(notifications)));
    settle(&mut rt);
    rt
}

/// `3b5243d` (#26): after « Autoriser » in the macOS prompt, the card kept showing the permission
/// as missing until something else redrew the page.
#[test]
fn the_recording_page_shows_the_screen_permission_granted_in_the_macos_prompt_without_a_click() {
    let permissions = MacPermissions::new(Permissions { screen: PermissionState::ToRequest, ..Permissions::GRANTED });
    let mut rt = app_with_permissions(&permissions, &FakeNotifications::new(PermissionState::Granted));
    // The recording cannot start without the screen: its page shows the card.
    tap(&mut rt, "record");
    assert!(state(&mut rt, |s| s.page == Page::Recording && !s.recorded()));

    tap(&mut rt, "perm-screen-allow");
    assert_eq!(permissions.asked(), [Permission::Screen], "the button shows the macOS prompt");
    wait(&mut rt, 1.0);
    assert!(shown(&rt, "perm-screen-allow"), "unanswered, the permission is still to grant");

    // « Autoriser » in the macOS prompt.
    permissions.set(Permission::Screen, PermissionState::Granted);
    wait(&mut rt, 1.0);

    assert!(shown(&rt, "perm-screen"));
    assert!(!shown(&rt, "perm-screen-allow"), "the card shows the screen permission granted");
}

/// `3b5243d` (#26) and `45eee6b`: the settings card follows System Settings, for the sources and
/// for the notifications « Bilan prêt » needs.
#[test]
fn the_settings_card_follows_permissions_granted_in_system_settings_notifications_included() {
    let permissions = MacPermissions::new(Permissions { micro: PermissionState::Denied, ..Permissions::GRANTED });
    let notifications = FakeNotifications::new(PermissionState::Denied);
    let mut rt = app_with_permissions(&permissions, &notifications);
    tap(&mut rt, "nav-settings");
    wait_for_motion(&mut rt);
    assert!(shown(&rt, "perm-micro-settings"));
    assert!(shown(&rt, "perm-notifications-settings"));

    permissions.set(Permission::Micro, PermissionState::Granted);
    wait(&mut rt, 1.0);
    assert!(!shown(&rt, "perm-micro-settings"), "the micro shows granted");
    assert!(shown(&rt, "perm-notifications-settings"));

    notifications.set(PermissionState::Granted);
    wait(&mut rt, 1.0);
    assert!(shown(&rt, "perm-notifications"));
    assert!(!shown(&rt, "perm-notifications-settings"), "the notifications show granted");
}

/// `45eee6b`: « Bilan prêt » was dropped silently when macOS refused notifications; the card now
/// has their line, and its button asks macOS.
#[test]
fn allow_on_the_notifications_line_asks_macos_and_the_line_shows_the_answer() {
    let notifications = FakeNotifications::new(PermissionState::ToRequest);
    let mut rt = app_with_permissions(&MacPermissions::new(Permissions::GRANTED), &notifications);
    tap(&mut rt, "nav-settings");
    wait_for_motion(&mut rt);

    tap(&mut rt, "perm-notifications-allow");
    assert_eq!(notifications.asked(), 1, "the button shows the macOS prompt");
    notifications.set(PermissionState::Granted);
    wait(&mut rt, 1.0);

    assert!(shown(&rt, "perm-notifications"));
    assert!(!shown(&rt, "perm-notifications-allow"));
}

// --- The recording shortcut -------------------------------------------------------------------

/// `9e2faa5`: the global shortcut is released while the settings field captures a new one. A
/// recording started from the menu bar opened its page during the capture: the field never lost
/// its focus, kept capturing, and the shortcut stayed released.
#[test]
fn a_recording_started_from_the_menu_bar_during_a_shortcut_capture_ends_it_and_gives_the_shortcut_back() {
    let system = FakeShortcuts::default();
    let chord: Shortcut = "cmd+shift+r".parse().expect("shortcut");
    let mut rt = app_on(demo_core());
    // Launched with ⇧⌘R remembered: the app registers it (in the window, with its handle).
    let (registrar, remembered) = (system.clone(), chord.clone());
    rt.update(move |s, _| {
        s.shortcuts = Some(Box::new(registrar));
        s.shortcut = ShortcutRecorderState::new(Some(remembered));
        s.register_record_shortcut();
    });
    assert_eq!(system.active(), [chord.clone()]);

    tap(&mut rt, "nav-settings");
    wait_for_motion(&mut rt);
    tap(&mut rt, "record-shortcut");
    assert!(state(&mut rt, |s| s.shortcut.is_recording()), "the field captures");
    // In the window, the app handle runs this right after the field's press; the tests have none.
    rt.update(|s, _| s.follow_shortcut_capture());
    assert!(system.active().is_empty(), "⇧⌘R is released while the field captures");

    // « Démarrer » in the menu bar: its choice runs this on the UI thread.
    rt.update(|s, _| {
        if !s.recorded() {
            s.toggle_recording()
        }
    });
    settle(&mut rt);

    state(&mut rt, |s| {
        assert!(s.page == Page::Recording && s.recorded());
        assert!(!s.shortcut.is_recording(), "the capture ended with its field");
    });
    assert_eq!(system.active(), [chord], "⇧⌘R starts and stops recording again");
}

// --- Typed letters and text fields ------------------------------------------------------------

/// The first processed meeting with speakers, open from the sidebar.
fn open_a_done_meeting(rt: &mut Runtime<App>) -> MeetingId {
    let id = state(rt, |s| s.core.meetings().iter().find(|m| m.status == Status::Done && m.active_speakers().count() > 1).expect("a processed meeting").id);
    tap(rt, &format!("m{id}"));
    wait(rt, 0.5);
    id
}

/// `be2594a`: once a click elsewhere took the focus of the speaker's
/// name field, the next letters reached the page: an f opened the video in full screen.
#[test]
fn after_a_click_away_from_the_speaker_name_field_f_and_space_stay_out_of_the_page() {
    let mut rt = app_on(demo_core());
    open_a_done_meeting(&mut rt);
    press(&mut rt, cmd('4'));
    scroll_page_to(&mut rt, "spk1-rename");
    tap(&mut rt, "spk1-rename");
    assert!(rt.has_focus(), "the name field has the keyboard");

    // A click in another speaker's card, off its buttons (in its padding).
    let [x, y, _, _] = rect(&rt, "spk0");
    click_at(&mut rt, x + 6.0, y + 6.0);
    assert!(!rt.has_focus(), "the click took the field's focus");
    assert!(shown(&rt, "spk1-name"), "the field is still open");

    press(&mut rt, key('f'));
    press(&mut rt, key(' '));

    state(&mut rt, |s| {
        assert!(!s.fullscreen, "f is typing, not full screen");
        assert!(!s.playing, "space is typing, not playback");
    });
}

/// `be2594a`: the title window did not hold the page's ⌘ shortcuts back: ⌘1 to ⌘5 changed the
/// tab behind it, ⌘⌫ asked to delete the meeting.
#[test]
fn the_page_behind_the_title_window_takes_no_command_shortcut() {
    let mut rt = app_on(demo_core());
    open_a_done_meeting(&mut rt);
    tap(&mut rt, "meeting-title");
    wait_for_motion(&mut rt);
    assert!(state(&mut rt, |s| s.rename.is_some()));

    press(&mut rt, cmd('2'));
    press(&mut rt, rune::KeyEvent { key: Key::Backspace, mods: rune::Mods { cmd: true, ..rune::Mods::default() } });

    state(&mut rt, |s| {
        assert!(s.rename.is_some(), "the title window stays open");
        assert!(s.tab == Tab::Summary, "⌘2 waits for the window to close");
        assert!(s.confirm_delete.is_none(), "⌘⌫ waits for the window to close");
    });
}

// --- Short recordings -------------------------------------------------------------------------

/// The prototype's meetings, with a capture in real time ×4 (a few seconds make a short recording).
fn app_recording_fast() -> Runtime<App> {
    app_on(Greffier::with_meetings(temp_folder(), adapters_with(|a| a.capture = Box::new(FakeCapture::new(4.0, 0.0))), demo::meetings()))
}

fn meeting_ids(rt: &mut Runtime<App>) -> Vec<MeetingId> {
    state(rt, |s| s.core.meetings().iter().map(|m| m.id).collect())
}

/// The meeting that appeared since `before`.
fn new_meeting(rt: &mut Runtime<App>, before: &[MeetingId]) -> MeetingId {
    let after = meeting_ids(rt);
    let new: Vec<MeetingId> = after.into_iter().filter(|id| !before.contains(id)).collect();
    assert_eq!(new.len(), 1, "one new meeting");
    new[0]
}

/// Records about 8 s from the sidebar's button and the recording page's stop button.
fn record_short_from_the_window(rt: &mut Runtime<App>) -> MeetingId {
    let before = meeting_ids(rt);
    tap(rt, "record");
    wait(rt, 2.0);
    tap(rt, "stop");
    new_meeting(rt, &before)
}

fn status(rt: &mut Runtime<App>, id: MeetingId) -> Option<Status> {
    state(rt, |s| s.core.meeting(id).map(|m| m.status.clone()))
}

/// `1703d2c` (#23): a second short recording stopped while the question about the first was open
/// replaced it: the first was never asked about nor processed.
#[test]
fn two_short_recordings_in_a_row_are_asked_about_one_after_the_other() {
    let mut rt = app_recording_fast();
    let first = record_short_from_the_window(&mut rt);
    assert!(shown(&rt, "short-keep"), "the app asks about the first");
    // The second, started and stopped from the menu bar while the question is open.
    let before = meeting_ids(&mut rt);
    rt.update(|s, _| s.toggle_recording());
    wait(&mut rt, 1.5);
    rt.update(|s, _| s.toggle_recording());
    settle(&mut rt);
    let second = new_meeting(&mut rt, &before);

    tap(&mut rt, "short-keep");
    assert!(status(&mut rt, first).as_ref().and_then(queued_at).is_some(), "« Garder » answered the first: it is processed");
    assert_eq!(status(&mut rt, second), Some(Status::ToProcess), "the second waits for its own answer");
    assert!(shown(&rt, "short-delete"), "the app now asks about the second");

    tap(&mut rt, "short-delete");
    assert_eq!(status(&mut rt, second), None, "« Supprimer » answered the second");
    assert!(status(&mut rt, first).is_some());
    assert!(!shown(&rt, "short-keep"), "nothing left to ask");
}

/// `32929a9`: a click outside the short recording question (the one bringing the window to the
/// front) closed it unanswered; Escape now keeps the recording, like « Garder ».
#[test]
fn a_click_outside_the_short_recording_question_leaves_it_open_and_escape_keeps_the_recording() {
    let mut rt = app_recording_fast();
    let id = record_short_from_the_window(&mut rt);

    let (x, y) = on_screen(&rt, "nav-home");
    click_at(&mut rt, x, y);
    assert!(shown(&rt, "short-keep"), "the question is still asked");
    assert_eq!(status(&mut rt, id), Some(Status::ToProcess), "nothing answered it");

    press(&mut rt, named(Key::Escape));

    assert!(!shown(&rt, "short-keep"));
    assert!(status(&mut rt, id).as_ref().and_then(queued_at).is_some(), "Escape kept the recording: it is processed");
}

// --- Interrupted recordings -------------------------------------------------------------------

/// `4f28b17`: an interrupted recording's page showed a processing panel that could be neither
/// started nor stopped; it now offers processing, like a meeting to process.
#[test]
fn an_interrupted_recording_can_be_processed_from_its_page() {
    let capture = FakeCapture::new(30.0, 0.0).broken_at(90.0, "the display disappeared");
    let mut rt = app_on(Greffier::with_meetings(temp_folder(), adapters_with(|a| a.capture = Box::new(capture)), demo::meetings()));
    let before = meeting_ids(&mut rt);
    tap(&mut rt, "record");
    wait(&mut rt, 4.0);
    let id = new_meeting(&mut rt, &before);
    assert!(state(&mut rt, |s| s.page == Page::Meeting(id) && !s.recorded()), "the app shows what was captured");
    assert_eq!(status(&mut rt, id), Some(Status::RecordingInterrupted));

    tap(&mut rt, "process");

    assert!(status(&mut rt, id).as_ref().and_then(queued_at).is_some(), "the interrupted recording is processed");
}
