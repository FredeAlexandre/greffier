//! Why the core refused a command, in two languages: French for the app's UI (`Display`, as the
//! UI shows it), English for the command line ([`Rejection::english`], ADR 0006). Each refusal
//! is one [`Reason`]; what the OS or an adapter said comes along as is, in a `detail`.

use std::fmt;
use std::path::PathBuf;

use crate::adapters::Permission;
use crate::folder::ImportError;
use crate::meeting::{MeetingId, Status};

/// Why a command was not executed. The state has not changed.
#[derive(Clone, Debug, PartialEq)]
pub enum Rejection {
    UnknownMeeting(MeetingId),
    Impossible(Reason),
}

impl Rejection {
    /// The refusal in English, for the command line.
    pub fn english(&self) -> String {
        match self {
            Rejection::UnknownMeeting(id) => format!("unknown meeting ({id})"),
            Rejection::Impossible(r) => r.english(),
        }
    }
}

impl fmt::Display for Rejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Rejection::UnknownMeeting(id) => write!(f, "réunion inconnue ({id})"),
            Rejection::Impossible(r) => r.fmt(f),
        }
    }
}

impl std::error::Error for Rejection {}

impl From<Reason> for Rejection {
    fn from(r: Reason) -> Rejection {
        Rejection::Impossible(r)
    }
}

/// What made a command impossible. `Display` gives the UI's French, [`Reason::english`] the
/// command line's English.
#[derive(Clone, Debug, PartialEq)]
pub enum Reason {
    NoRecording,
    AlreadyRecording,
    /// No source is enabled in the settings.
    NoSource,
    /// macOS is asking the user for this permission.
    PermissionRequested(Permission),
    PermissionDenied(Permission),
    AlreadyProcessed,
    /// The meeting's status does not allow processing.
    CannotProcess(Status),
    AlreadyInQueue,
    EarlierStagesNotDone,
    NotProcessing,
    EmptyName,
    /// Merging two speakers that are already one voice.
    SameVoice,
    /// Splitting a speaker that is not merged.
    NotMerged,
    UnknownSpeaker,
    UnknownAction,
    EmptyTitle,
    /// The title changes once the running stage ends.
    TitleWhileRunning,
    /// A stage is about to redo the transcription.
    TranscriptionBeingProcessed,
    EmptyUtterance,
    UnknownUtterance,
    /// The change touches a passage to check, which is settled as such.
    InPassageToCheck,
    UnknownPassage,
    NoHypothesis,
    EmptyCorrection,
    /// What the passage shows is no longer in its utterance.
    CorrectedByHand(String),
    /// The meeting's folder (by name) did not go to the trash; `detail`: the trash's message.
    Trash { folder_name: String, detail: String },
    /// The import's files were refused or could not be copied.
    Import(ImportError),
    /// The file system or an adapter failed; `detail`: its message, as it gave it.
    Failed { what: Operation, detail: String },
}

/// What failed, for [`Reason::Failed`].
#[derive(Clone, Debug, PartialEq)]
pub enum Operation {
    /// Creating the folder of the meeting about to be recorded.
    CreateMeetingFolder,
    /// Starting the capture.
    StartCapture,
    /// Checking an action in this summary file.
    CheckAction(PathBuf),
}

impl Reason {
    /// The reason in English, for the command line.
    pub fn english(&self) -> String {
        match self {
            Reason::NoRecording => "no recording in progress".into(),
            Reason::AlreadyRecording => "a recording is already in progress".into(),
            Reason::NoSource => "Nothing to record: turn on the screen, the Mac's sound or the microphone in the settings.".into(),
            Reason::PermissionRequested(p) => format!("Greffier is asking for the \"{}\" permission: accept macOS's request, then start the recording again.", english_name(*p).to_lowercase()),
            Reason::PermissionDenied(p) => {
                format!("Greffier does not have the \"{}\" permission: System Settings › Privacy & Security › {}, turn on Greffier.", english_name(*p), english_section(*p))
            }
            Reason::AlreadyProcessed => "the meeting is already processed".into(),
            Reason::CannotProcess(s) => format!("the meeting cannot be processed ({s:?})"),
            Reason::AlreadyInQueue => "the meeting is already in the queue".into(),
            Reason::EarlierStagesNotDone => "the earlier stages are not done".into(),
            Reason::NotProcessing => "the meeting is not being processed".into(),
            Reason::EmptyName => "the name is empty".into(),
            Reason::SameVoice => "these two speakers are already the same voice".into(),
            Reason::NotMerged => "this speaker is not merged".into(),
            Reason::UnknownSpeaker => "unknown speaker".into(),
            Reason::UnknownAction => "unknown action".into(),
            Reason::EmptyTitle => "the title is empty".into(),
            Reason::TitleWhileRunning => "the title can change once the running stage ends".into(),
            Reason::TranscriptionBeingProcessed => "the transcription is being processed: wait for the review to end".into(),
            Reason::EmptyUtterance => "an utterance cannot be empty".into(),
            Reason::UnknownUtterance => "unknown utterance".into(),
            Reason::InPassageToCheck => "this text is in a passage to check: resolve the passage instead".into(),
            Reason::UnknownPassage => "unknown passage".into(),
            Reason::NoHypothesis => "this passage has no hypothesis".into(),
            Reason::EmptyCorrection => "the correction is empty".into(),
            Reason::CorrectedByHand(current) => format!("\"{current}\" is no longer in the utterance: it was corrected by hand"),
            Reason::Trash { folder_name, detail } => format!("\"{folder_name}\" could not be moved to the trash: {detail}"),
            Reason::Import(e) => e.english(),
            Reason::Failed { what, detail } => match what {
                Operation::CreateMeetingFolder => format!("the meeting's folder could not be created: {detail}"),
                Operation::StartCapture => format!("the recording could not start: {detail}"),
                Operation::CheckAction(path) => format!("the action could not be checked in {}: {detail}", path.display()),
            },
        }
    }
}

impl fmt::Display for Reason {
    /// The UI's French.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Reason::NoRecording => f.write_str("aucun enregistrement en cours"),
            Reason::AlreadyRecording => f.write_str("un enregistrement est déjà en cours"),
            Reason::NoSource => f.write_str("Aucune source à enregistrer : active l'écran, le son du Mac ou le micro dans les réglages."),
            Reason::PermissionRequested(p) => write!(f, "Greffier demande l'autorisation pour « {} » : accepte la demande de macOS, puis relance l'enregistrement.", p.name().to_lowercase()),
            Reason::PermissionDenied(p) => write!(f, "Greffier n'a pas l'autorisation pour « {} » : Réglages Système › Confidentialité et sécurité › {}, active Greffier.", p.name(), p.section()),
            Reason::AlreadyProcessed => f.write_str("la réunion est déjà traitée"),
            Reason::CannotProcess(s) => write!(f, "la réunion ne peut pas être traitée ({s:?})"),
            Reason::AlreadyInQueue => f.write_str("la réunion est déjà dans la file"),
            Reason::EarlierStagesNotDone => f.write_str("les étapes précédentes ne sont pas faites"),
            Reason::NotProcessing => f.write_str("la réunion n'est pas en traitement"),
            Reason::EmptyName => f.write_str("le nom est vide"),
            Reason::SameVoice => f.write_str("ces deux locuteurs sont déjà la même voix"),
            Reason::NotMerged => f.write_str("ce locuteur n'est pas fusionné"),
            Reason::UnknownSpeaker => f.write_str("locuteur inconnu"),
            Reason::UnknownAction => f.write_str("action inconnue"),
            Reason::EmptyTitle => f.write_str("le titre est vide"),
            Reason::TitleWhileRunning => f.write_str("le titre pourra changer à la fin de l'étape en cours"),
            Reason::TranscriptionBeingProcessed => f.write_str("la transcription est en cours de traitement : attends la fin de la relecture"),
            Reason::EmptyUtterance => f.write_str("une réplique ne peut pas être vide"),
            Reason::UnknownUtterance => f.write_str("réplique inconnue"),
            Reason::InPassageToCheck => f.write_str("ce passage est à vérifier : il se tranche dans l'onglet À vérifier"),
            Reason::UnknownPassage => f.write_str("passage inconnu"),
            Reason::NoHypothesis => f.write_str("ce passage n'a pas d'hypothèse"),
            Reason::EmptyCorrection => f.write_str("la correction est vide"),
            Reason::CorrectedByHand(current) => write!(f, "« {current} » n'est plus dans la réplique : elle a été corrigée à la main"),
            Reason::Trash { folder_name, detail } => write!(f, "« {folder_name} » n'a pas pu aller à la corbeille : {detail}"),
            Reason::Import(e) => e.fmt(f),
            Reason::Failed { what, detail } => match what {
                Operation::CreateMeetingFolder => write!(f, "dossier de la réunion : {detail}"),
                Operation::StartCapture => f.write_str(detail),
                Operation::CheckAction(path) => write!(f, "{} : {detail}", path.display()),
            },
        }
    }
}

/// A permission's name in English, as macOS's English settings say it.
fn english_name(p: Permission) -> &'static str {
    match p {
        Permission::Screen => "Screen",
        Permission::Micro => "Microphone",
        Permission::MacAudio => "Mac audio",
    }
}

/// The System Settings › Privacy & Security section that grants it, in English.
fn english_section(p: Permission) -> &'static str {
    match p {
        Permission::Screen | Permission::MacAudio => "Screen & System Audio Recording",
        Permission::Micro => "Microphone",
    }
}
