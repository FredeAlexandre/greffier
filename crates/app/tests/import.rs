//! Importing a past meeting by dropping its files on the window: the import window with what
//! the core proposes from the files, then processing now or later (cycle 1 QA, F1).

mod support;

use std::path::PathBuf;

use greffier::{App, Page};
use greffier_core::{MeetingId, Stage, Status};
use rune::Runtime;
use support::*;

/// A sample meeting's recording and transcription, copied to a fresh folder (the originals are
/// not to move).
fn dropped_files() -> Vec<PathBuf> {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../core/tests/fixtures/meetings-folder/2026-11-18 09h15 - Réunion");
    let folder = temp_folder();
    ["enregistrement.mp4", "transcription.json"]
        .map(|suffix| {
            let to = folder.join(format!("Point banc de test.{}", suffix.rsplit('.').next().expect("extension")));
            std::fs::copy(fixture.join(format!("2026-11-18 09h15 - Réunion - {suffix}")), &to).expect("copy");
            to
        })
        .to_vec()
}

/// Drops `files` in the middle of the window, then imports them as proposed. The new meeting.
fn drop_and_import(rt: &mut Runtime<App>, files: &[PathBuf]) -> MeetingId {
    let before: Vec<MeetingId> = state(rt, |s| s.core.meetings().iter().map(|m| m.id).collect());
    rt.drop_files(files.to_vec(), Some((SIZE.0 / 2.0, SIZE.1 / 2.0)));
    settle(rt);
    assert!(state(rt, |s| s.import.is_some()), "the import window opens");
    wait_for_motion(rt);
    tap(rt, "import-ok");
    state(rt, |s| s.core.meetings().iter().map(|m| m.id).find(|id| !before.contains(id)).expect("a new meeting"))
}

#[test]
fn dropped_files_are_imported_with_the_date_and_title_proposed_from_them() {
    let mut rt = app_on(demo_core());
    let files = dropped_files();
    let proposal = state(&mut rt, |s| s.core.propose_import(&files).expect("a proposal"));

    let id = drop_and_import(&mut rt, &files);

    state(&mut rt, |s| {
        let m = s.core.meeting(id).expect("the imported meeting");
        assert_eq!(m.start, proposal.start);
        assert_eq!(m.title, proposal.title);
        assert_eq!(m.completed_stage, Some(Stage::Transcription), "the dropped transcription came along");
        assert!(s.import.is_none());
        assert!(s.page == Page::Meeting(id), "the imported meeting opens");
        assert_eq!(s.process_imported, Some(id), "and we ask when to process it");
    });
}

#[test]
fn process_now_after_an_import_queues_the_stages_left() {
    let mut rt = app_on(demo_core());
    let id = drop_and_import(&mut rt, &dropped_files());
    wait_for_motion(&mut rt);

    tap(&mut rt, "import-now");

    state(&mut rt, |s| {
        assert!(s.process_imported.is_none());
        assert_eq!(queued_at(&s.core.meeting(id).expect("the meeting").status), Some(Stage::Review), "the transcription is imported: the review is next");
    });
}

#[test]
fn process_later_after_an_import_leaves_the_meeting_to_process() {
    let mut rt = app_on(demo_core());
    let id = drop_and_import(&mut rt, &dropped_files());
    wait_for_motion(&mut rt);

    tap(&mut rt, "import-later");

    state(&mut rt, |s| {
        assert!(s.process_imported.is_none());
        assert_eq!(s.core.meeting(id).expect("the meeting").status, Status::ToProcess);
        assert!(!s.core.busy(), "nothing is processed");
    });
}

#[test]
fn a_file_greffier_does_not_import_is_refused_in_french() {
    let mut rt = app_on(demo_core());
    let notes = temp_folder().join("notes.txt");
    std::fs::write(&notes, "").expect("notes");
    rt.update(move |s, _| s.propose_import(std::slice::from_ref(&notes)));
    state(&mut rt, |s| {
        assert!(s.import.is_none());
        let toast = s.toast.as_ref().expect("a toast says why");
        assert_eq!(toast.text, "« notes.txt » : Greffier importe un média (vidéo ou audio), une transcription (.vtt, .srt, .json) et un bilan (.md).", "the UI stays French");
        assert!(toast.error);
    });
}
