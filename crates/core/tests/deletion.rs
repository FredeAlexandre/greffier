//! Deleting a meeting: its whole folder goes to the Mac's trash, so it can be recovered. The
//! core goes through the trash adapter (here a fake trash).

mod bench;

use bench::Bench;
use greffier_core::adapters::Adapters;
use greffier_core::fake::FakeTrash;
use greffier_core::{Command, Rejection, Status};

const RELEASE: &str = "2026-11-20 10h00 - Revue de release";

fn bench_with(trash: &FakeTrash) -> Bench {
    let mut bench = Bench::on_fixtures();
    bench.greffier = greffier_core::Greffier::open(&bench.folder, Adapters { trash: Box::new(trash.clone()), ..bench::adapters() });
    bench
}

#[test]
fn deleting_sends_the_folder_to_the_trash() {
    let trash = FakeTrash::new();
    let mut bench = bench_with(&trash);
    let id = bench.named_meeting(RELEASE).id;

    bench.execute(Command::Delete(id));

    assert!(bench.greffier.meeting(id).is_none());
    assert!(!bench.folder.join(RELEASE).exists());
    assert_eq!(trash.trashed(), [bench.folder.join(RELEASE)]);
    // The whole folder is in the trash, recording and summary included.
    let trashed = trash.folder().join(RELEASE);
    assert!(trashed.join(format!("{RELEASE} - bilan.md")).is_file());
    assert!(trashed.join(format!("{RELEASE} - enregistrement.mp4")).is_file());
    assert!(bench.greffier.search("prévient").is_empty());

    // Nothing comes back: neither on folder rescan nor on the next launch.
    assert!(bench.greffier.rescan_folder().is_empty());
    bench.reopen();
    assert!(bench.greffier.meetings().iter().all(|r| r.folder_name != RELEASE));
}

#[test]
fn a_refusing_trash_keeps_the_meeting() {
    let trash = FakeTrash::new().broken("Le volume est en lecture seule.");
    let mut bench = bench_with(&trash);
    let id = bench.named_meeting(RELEASE).id;

    let rejection = bench.greffier.execute(Command::Delete(id)).unwrap_err();

    let Rejection::Impossible(message) = rejection else { panic!("{rejection:?}") };
    assert!(message.contains("Le volume est en lecture seule."), "{message}");
    assert!(bench.greffier.meeting(id).is_some());
    assert!(bench.folder.join(RELEASE).join(format!("{RELEASE} - bilan.md")).is_file());
    assert_eq!(bench.greffier.search("prévient").len(), 1);
}

#[test]
fn deleting_a_meeting_being_processed_stops_the_processing() {
    let trash = FakeTrash::new();
    let mut bench = Bench::with(Adapters { trash: Box::new(trash.clone()), ..bench::adapters() });
    let id = bench.meeting_to_process();
    let name = bench.meeting(id).folder_name.clone();
    bench.execute(Command::Process(id));
    bench.advance(0.5);
    assert!(matches!(bench.meeting(id).status, Status::Running { .. }));

    bench.execute(Command::Delete(id));
    bench.advance(5.0);

    assert!(bench.greffier.meeting(id).is_none());
    assert!(!bench.greffier.busy());
    assert!(!bench.folder.join(&name).exists(), "the folder must not be rewritten");
    assert_eq!(trash.trashed(), [bench.folder.join(&name)]);
}

#[test]
fn a_meeting_whose_folder_already_vanished_is_deleted_without_trash() {
    let trash = FakeTrash::new();
    let mut bench = bench_with(&trash);
    let id = bench.named_meeting(RELEASE).id;
    std::fs::remove_dir_all(bench.folder.join(RELEASE)).unwrap();

    bench.execute(Command::Delete(id));

    assert!(bench.greffier.meeting(id).is_none());
    assert!(trash.trashed().is_empty());
}
