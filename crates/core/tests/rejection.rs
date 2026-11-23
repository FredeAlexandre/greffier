//! A refusal speaks two languages: the UI's French (`Display`, unchanged) and the command line's
//! English (`english()`), with what the OS or an adapter said given as is.

mod bench;

use std::path::{Path, PathBuf};

use bench::Bench;
use greffier_core::adapters::{Adapters, Permission};
use greffier_core::fake::FakeTrash;
use greffier_core::folder::{FileKind, ImportError};
use greffier_core::{Command, Operation, Reason, Rejection, Resolution, Status};

const RELEASE: &str = "2026-11-20 10h00 - Revue de release";

fn refusal(bench: &mut Bench, command: Command) -> Rejection {
    let name = format!("{command:?}");
    bench.greffier.execute(command).err().unwrap_or_else(|| panic!("{name} accepted"))
}

/// (French, English) of a refusal.
fn both(r: &Rejection) -> (String, String) {
    (r.to_string(), r.english())
}

#[test]
fn a_refusal_keeps_the_ui_french_and_gives_the_command_line_english() {
    let mut bench = Bench::on_fixtures();
    let id = bench.named_meeting(RELEASE).id;
    let r = refusal(&mut bench, Command::Process(id));
    assert_eq!(both(&r), ("la réunion ne peut pas être traitée (Done)".into(), "the meeting is already processed".into()));
    let r = refusal(&mut bench, Command::ChangeTitle { meeting: id, title: "  ".into() });
    assert_eq!(both(&r), ("le titre est vide".into(), "the title is empty".into()));
    let r = refusal(&mut bench, Command::StopProcessing(id));
    assert_eq!(both(&r), ("la réunion n'est pas en traitement".into(), "the meeting is not being processed".into()));
    let r = refusal(&mut bench, Command::NameSpeaker { meeting: id, speaker: 0, name: " ".into() });
    assert_eq!(both(&r), ("le nom est vide".into(), "the name is empty".into()));
    let r = refusal(&mut bench, Command::SplitSpeaker { meeting: id, speaker: 0 });
    assert_eq!(both(&r), ("ce locuteur n'est pas fusionné".into(), "this speaker is not merged".into()));
    let r = refusal(&mut bench, Command::MergeSpeakers { meeting: id, speaker: 0, into: 0 });
    assert_eq!(both(&r), ("ces deux locuteurs sont déjà la même voix".into(), "these two speakers are already the same voice".into()));
    let r = refusal(&mut bench, Command::CheckAction { meeting: id, action: 99, done: true });
    assert_eq!(both(&r), ("action inconnue".into(), "unknown action".into()));
    let r = refusal(&mut bench, Command::CorrectUtterance { meeting: id, utterance: 0, text: "".into() });
    assert_eq!(both(&r), ("une réplique ne peut pas être vide".into(), "an utterance cannot be empty".into()));
    let r = refusal(&mut bench, Command::Resolve { meeting: id, passage: 99, resolution: Resolution::KeepOriginal });
    assert_eq!(both(&r), ("passage inconnu".into(), "unknown passage".into()));
    let r = refusal(&mut bench, Command::Process(999));
    assert_eq!(both(&r), ("réunion inconnue (999)".into(), "unknown meeting (999)".into()));
}

#[test]
fn a_refusing_trash_is_said_in_both_languages_with_its_own_message() {
    let trash = FakeTrash::new().broken("Le volume est en lecture seule.");
    let mut bench = Bench::on_fixtures();
    bench.greffier = greffier_core::Greffier::open(&bench.folder, Adapters { trash: Box::new(trash.clone()), ..bench::adapters() });
    let id = bench.named_meeting(RELEASE).id;
    let r = refusal(&mut bench, Command::Delete(id));
    assert_eq!(
        both(&r),
        (
            format!("« {RELEASE} » n'a pas pu aller à la corbeille : Le volume est en lecture seule."),
            format!("\"{RELEASE}\" could not be moved to the trash: Le volume est en lecture seule.")
        )
    );
}

#[test]
fn an_import_proposal_is_refused_in_both_languages() {
    let bench = Bench::on_fixtures();
    let notes = bench.folder.join("notes.txt");
    let r = bench.greffier.propose_import(std::slice::from_ref(&notes)).unwrap_err();
    assert_eq!(
        both(&r),
        (
            "« notes.txt » : Greffier importe un média (vidéo ou audio), une transcription (.vtt, .srt, .json) et un bilan (.md).".into(),
            "\"notes.txt\": Greffier imports a medium (video or audio), a transcription (.vtt, .srt, .json) and a summary (.md).".into()
        )
    );
    let r = bench.greffier.propose_import(&[]).unwrap_err();
    assert_eq!(both(&r), ("Il faut un média (vidéo ou audio) pour importer une réunion.".into(), "A medium (video or audio) is needed to import a meeting.".into()));
}

/// Every reason, with a path, a detail and a text from the meeting that carry no French.
fn every_reason() -> Vec<Reason> {
    let p = || PathBuf::from("/m/a.vtt");
    let detail = || "No such file or directory (os error 2)".to_string();
    let import = [
        ImportError::NotMedia(p()),
        ImportError::NotFound(p()),
        ImportError::Io { path: p(), detail: detail() },
        ImportError::UnreadableTranscription { path: p(), detail: detail() },
        ImportError::TranscriptionFormat(p()),
        ImportError::NoUtterance(p()),
        ImportError::UnknownKind("a.pdf".into()),
        ImportError::Several(FileKind::Media),
        ImportError::Several(FileKind::Transcription),
        ImportError::Several(FileKind::Summary),
        ImportError::NoMedia,
    ];
    let mut reasons = vec![
        Reason::NoRecording,
        Reason::AlreadyRecording,
        Reason::NoSource,
        Reason::PermissionRequested(Permission::Screen),
        Reason::PermissionDenied(Permission::MacAudio),
        Reason::AlreadyProcessed,
        Reason::CannotProcess(Status::Recording),
        Reason::AlreadyInQueue,
        Reason::EarlierStagesNotDone,
        Reason::NotProcessing,
        Reason::EmptyName,
        Reason::SameVoice,
        Reason::NotMerged,
        Reason::UnknownSpeaker,
        Reason::UnknownAction,
        Reason::EmptyTitle,
        Reason::TitleWhileRunning,
        Reason::TranscriptionBeingProcessed,
        Reason::EmptyUtterance,
        Reason::UnknownUtterance,
        Reason::InPassageToCheck,
        Reason::UnknownPassage,
        Reason::NoHypothesis,
        Reason::EmptyCorrection,
        Reason::CorrectedByHand("ship it".into()),
        Reason::Trash { folder_name: "2026-11-20 10h00 - Review".into(), detail: detail() },
        Reason::Failed { what: Operation::CreateMeetingFolder, detail: detail() },
        Reason::Failed { what: Operation::StartCapture, detail: detail() },
        Reason::Failed { what: Operation::CheckAction(Path::new("/m/b.md").to_path_buf()), detail: detail() },
    ];
    reasons.extend(import.into_iter().map(Reason::Import));
    reasons
}

#[test]
fn every_reason_has_an_english_rendering_without_french() {
    let french = |s: &str| s.chars().any(|c| "àâçéèêëîïôûùüÿœæÀÂÇÉÈÊËÎÏÔÛÙÜŸŒÆ«»".contains(c));
    for r in every_reason() {
        let english = r.english();
        assert!(!english.is_empty() && !french(&english), "{r:?}: {english}");
        assert_ne!(english, r.to_string(), "{r:?}");
    }
}
