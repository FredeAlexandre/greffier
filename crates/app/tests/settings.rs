//! The settings page: the recorded sources, processing on stop, the agent's context folders and
//! the meetings folder (cycle 1 QA, I1, I2). Not live, nothing is remembered on disk.

mod support;

use std::path::PathBuf;

use greffier::{App, Page};
use rune::Runtime;
use support::*;

fn settings_page() -> Runtime<App> {
    let mut rt = app_on(demo_core());
    tap(&mut rt, "nav-settings");
    wait_for_motion(&mut rt);
    assert!(state(&mut rt, |s| s.page == Page::Settings));
    rt
}

#[test]
fn a_source_turned_off_in_the_settings_is_not_recorded() {
    let mut rt = settings_page();

    tap(&mut rt, "source-micro");
    let sources = state(&mut rt, |s| s.core.settings().sources);
    assert!(!sources.micro && sources.screen && sources.mac_audio, "only the microphone is off: {sources:?}");
    tap(&mut rt, "record");

    assert_eq!(state(&mut rt, |s| s.core.recording().expect("recording").sources), sources);
}

#[test]
fn the_process_on_stop_switch_changes_the_setting_both_ways() {
    let mut rt = settings_page();
    let on = state(&mut rt, |s| s.core.settings().process_on_stop);

    tap(&mut rt, "process-on-stop");
    assert_eq!(state(&mut rt, |s| s.core.settings().process_on_stop), !on);
    tap(&mut rt, "process-on-stop");
    assert_eq!(state(&mut rt, |s| s.core.settings().process_on_stop), on);
}

#[test]
fn a_context_folder_picked_is_opened_to_the_agent_once_and_removed_with_its_button() {
    let mut rt = settings_page();
    let notes: PathBuf = temp_folder();
    scroll_to(&mut rt, "settings", "add-context");

    for _ in 0..2 {
        tap(&mut rt, "add-context");
        assert!(rt.file_dialog().is_some(), "the folder picker opens");
        rt.resolve_file_dialog(Some(notes.clone())).expect("answer the picker");
        settle(&mut rt);
    }
    assert_eq!(state(&mut rt, |s| s.core.settings().context_folders.clone()), [notes.clone()], "picked twice, listed once");

    wait_for_motion(&mut rt);
    scroll_to(&mut rt, "settings", "remove-context0");
    tap(&mut rt, "remove-context0");
    assert!(state(&mut rt, |s| s.core.settings().context_folders.is_empty()));
}

#[test]
fn the_meetings_folder_cannot_be_changed_during_a_recording() {
    let mut rt = settings_page();
    scroll_to(&mut rt, "settings", "change-folder");
    tap(&mut rt, "change-folder");
    assert!(rt.file_dialog().is_some(), "idle, the folder picker opens");
    rt.resolve_file_dialog(None).expect("cancel the picker");
    settle(&mut rt);

    tap(&mut rt, "record");
    tap(&mut rt, "nav-settings");
    wait_for_motion(&mut rt);
    scroll_to(&mut rt, "settings", "change-folder");
    tap(&mut rt, "change-folder");

    assert!(rt.file_dialog().is_none(), "recording, the button does nothing");
    assert!(state(&mut rt, |s| s.folder.is_none() && s.core.recording().is_some()));
}
