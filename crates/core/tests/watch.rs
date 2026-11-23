//! The files are the source of truth: what changes in the meetings folder outside the app (a
//! summary touched up in an editor, a transcription corrected by an agent, a folder deleted in the
//! Finder) shows in the app as soon as the core rescans the folder, without relaunching.

mod bench;

use std::path::PathBuf;

use bench::Bench;
use greffier_core::{Command, Event, Role, Status};

const RELEASE: &str = "2026-11-20 10h00 - Revue de release";

fn file(bench: &Bench, name: &str, role: Role) -> PathBuf {
    bench.greffier.folder().file(name, role)
}

#[test]
fn a_summary_edited_outside_the_app_is_reloaded() {
    let mut bench = Bench::on_fixtures();
    let id = bench.named_meeting(RELEASE).id;
    let path = file(&bench, RELEASE, Role::Summary);
    let md = bench.read(&path).replace("### Support (0:00:05)", "### Support client (0:00:07)");
    std::fs::write(&path, md).unwrap();

    let events = bench.greffier.rescan_folder();

    assert_eq!(events, [Event::MeetingReviewed(id)]);
    let r = bench.meeting(id);
    assert!(r.summary.contains("### Support client (0:00:07)"));
    let topics: Vec<(f64, &str)> = r.topics.iter().map(|s| (s.start, s.title.as_str())).collect();
    assert_eq!(topics, [(0.0, "Date de la release"), (7.0, "Support client")]);
}

#[test]
fn a_sheet_modified_outside_the_app_is_reloaded() {
    let mut bench = Bench::on_fixtures();
    let id = bench.named_meeting(RELEASE).id;
    let path = file(&bench, RELEASE, Role::Sheet);
    let md = bench.read(&path).replace("SPEAKER_00: \"Alexandre\"", "SPEAKER_00: \"Alex\"\n  SPEAKER_01: \"Khalid\"");
    std::fs::write(&path, md).unwrap();

    assert_eq!(bench.greffier.rescan_folder(), [Event::MeetingReviewed(id)]);
    let r = bench.meeting(id);
    let names: Vec<String> = r.speakers.iter().map(|l| l.display_name()).collect();
    assert_eq!(names, ["Alex", "Khalid", "SPEAKER_02"]);
}

#[test]
fn a_transcription_corrected_outside_the_app_is_reloaded() {
    let mut bench = Bench::on_fixtures();
    let id = bench.named_meeting(RELEASE).id;
    let path = file(&bench, RELEASE, Role::StructuredTranscription);
    let json = bench.read(&path).replace("Je préviens le support demain.", "Je préviens le support ce soir.");
    std::fs::write(&path, json).unwrap();

    assert_eq!(bench.greffier.rescan_folder(), [Event::MeetingReviewed(id)]);
    assert_eq!(bench.meeting(id).utterances[2].text, "Je préviens le support ce soir.");
}

fn names(bench: &Bench) -> Vec<&str> {
    bench.greffier.meetings().iter().map(|r| r.folder_name.as_str()).collect()
}

#[test]
fn a_folder_deleted_in_finder_makes_the_meeting_disappear() {
    let mut bench = Bench::on_fixtures();
    let id = bench.named_meeting(RELEASE).id;
    std::fs::remove_dir_all(bench.folder.join(RELEASE)).unwrap();

    assert_eq!(bench.greffier.rescan_folder(), [Event::MeetingDisappeared(id)]);
    assert!(bench.greffier.meeting(id).is_none());
    assert_eq!(names(&bench), ["2026-11-19 15h30 - Réunion", "2026-11-18 09h15 - Réunion"]);
}

#[test]
fn a_waiting_meeting_whose_folder_vanishes_leaves_the_queue() {
    let mut bench = Bench::on_fixtures();
    let id = bench.named_meeting("2026-11-19 15h30 - Réunion").id;
    let other = bench.named_meeting("2026-11-18 09h15 - Réunion").id;
    bench.execute(Command::Process(other));
    bench.execute(Command::Process(id));
    assert_eq!(bench.greffier.rank(id), Some(1));
    std::fs::remove_dir_all(bench.folder.join("2026-11-19 15h30 - Réunion")).unwrap();

    assert_eq!(bench.greffier.rescan_folder(), [Event::MeetingDisappeared(id)]);
    assert_eq!(bench.greffier.rank(id), None);
    bench.advance_until(other, |s| *s == Status::Done);
}

#[test]
fn a_folder_added_in_finder_makes_the_meeting_appear_in_its_place() {
    let mut bench = Bench::on_fixtures();
    let name = "2026-11-19 18h00 - Rétro";
    std::fs::create_dir_all(bench.folder.join(name)).unwrap();
    std::fs::copy(file(&bench, RELEASE, Role::Summary), file(&bench, name, Role::Summary)).unwrap();

    let events = bench.greffier.rescan_folder();

    let r = bench.named_meeting(name);
    assert_eq!(events, [Event::MeetingAppeared(r.id)]);
    assert_eq!(r.title, "Rétro");
    assert_eq!(r.status, Status::Done);
    assert!(bench.greffier.meetings().iter().filter(|x| x.id == r.id).count() == 1, "a fresh identifier");
    assert_eq!(names(&bench), [RELEASE, name, "2026-11-19 15h30 - Réunion", "2026-11-18 09h15 - Réunion"]);
    assert_eq!(bench.greffier.rescan_folder(), [], "only once");
}

#[test]
fn a_folder_not_following_the_naming_rule_is_ignored() {
    let mut bench = Bench::on_fixtures();
    std::fs::create_dir_all(bench.folder.join("Brouillons")).unwrap();
    assert_eq!(bench.greffier.rescan_folder(), []);
}

#[test]
fn a_meeting_deleted_in_the_app_does_not_come_back() {
    let mut bench = Bench::on_fixtures();
    let id = bench.named_meeting(RELEASE).id;
    bench.execute(Command::Delete(id));
    assert_eq!(bench.greffier.rescan_folder(), []);
    assert!(bench.greffier.meeting(id).is_none());
}

#[test]
fn running_recording_folder_does_not_appear_as_a_meeting() {
    let mut bench = Bench::new();
    bench.execute(Command::StartRecording);
    bench.advance(2.0);
    assert_eq!(bench.greffier.rescan_folder(), []);
    assert!(bench.greffier.meetings().is_empty());
    bench.finish_recording();
    assert_eq!(bench.greffier.rescan_folder(), [], "the stopped recording is already known");
    assert_eq!(bench.greffier.meetings().len(), 1);
}

#[test]
fn greffier_writes_reload_nothing() {
    let mut bench = Bench::new();
    let id = bench.record(3.0);
    bench.execute(Command::KeepShortRecording(id));
    assert_eq!(bench.greffier.rescan_folder(), []);
    // During the whole processing (the agent writes the summary), then after.
    for _ in 0..60 {
        bench.advance(0.1);
        assert_eq!(bench.greffier.rescan_folder(), [], "during {:?}", bench.meeting(id).status);
    }
    assert_eq!(bench.meeting(id).status, Status::Done);
    bench.execute(Command::NameSpeaker { meeting: id, speaker: 1, name: "Khalid".into() });
    bench.execute(Command::CheckAction { meeting: id, action: 0, done: true });
    bench.execute(Command::MarkRead(id));
    assert_eq!(bench.greffier.rescan_folder(), []);
    bench.reopen();
    assert_eq!(bench.greffier.rescan_folder(), []);
}

#[test]
fn a_change_during_processing_is_reread_afterwards() {
    let mut bench = Bench::on_fixtures();
    let id = bench.named_meeting(RELEASE).id;
    bench.execute(Command::Redo { meeting: id, stage: greffier_core::Stage::Summary });
    bench.advance(0.2);
    let path = file(&bench, RELEASE, Role::ReadableTranscription);
    std::fs::write(&path, "retouchée").unwrap();
    assert_eq!(bench.greffier.rescan_folder(), [], "the meeting being processed waits");
    bench.advance_until(id, |s| *s == Status::Done);
    // Greffier rewrote the readable transcription at the end of the stage: nothing left to rescan.
    assert_eq!(bench.greffier.rescan_folder(), []);
}

#[test]
fn ticking_an_action_changes_only_its_checkbox_in_the_summary() {
    let mut bench = Bench::on_fixtures();
    let id = bench.named_meeting(RELEASE).id;
    let path = file(&bench, RELEASE, Role::Summary);
    // Windows line endings, no final newline, a checkbox outside the actions, an [X].
    let before = bench.read(&path).trim_end().replace("- Aucune.", "- [X] Déjà tranchée").replace('\n', "\r\n");
    std::fs::write(&path, &before).unwrap();
    bench.greffier.rescan_folder();

    bench.execute(Command::CheckAction { meeting: id, action: 0, done: true });
    let after = bench.read(&path);
    let differences: Vec<usize> = (0..before.len()).filter(|&i| before.as_bytes()[i] != after.as_bytes()[i]).collect();
    assert_eq!(after.len(), before.len());
    assert_eq!(differences.len(), 1);
    assert!(after.contains("- [x] **Alexandre** — publier la release — lundi\r\n"));
    assert_eq!(bench.meeting(id).summary, after, "the app's summary is the file's");

    bench.execute(Command::CheckAction { meeting: id, action: 1, done: false });
    bench.execute(Command::CheckAction { meeting: id, action: 2, done: true });
    let after = bench.read(&path);
    assert!(after.contains("- [ ] **Khalid** — lancer la recette — vendredi\r\n"));
    assert!(after.ends_with("- [X] Déjà tranchée"), "an already ticked checkbox stays as is");
    assert_eq!(bench.greffier.rescan_folder(), []);
}

#[test]
fn ticking_an_action_starts_from_the_summary_as_it_is_on_disk() {
    let mut bench = Bench::on_fixtures();
    let id = bench.named_meeting(RELEASE).id;
    let path = file(&bench, RELEASE, Role::Summary);
    let md = bench.read(&path).replace("## Questions ouvertes", "- [ ] Ajoutée dans l'éditeur\n\n## Questions ouvertes");
    std::fs::write(&path, md).unwrap();

    bench.execute(Command::CheckAction { meeting: id, action: 0, done: true });
    let r = bench.meeting(id);
    assert!(r.summary.contains("- [ ] Ajoutée dans l'éditeur"));
    assert!(bench.read(&path).contains("- [x] **Alexandre**"));
}
