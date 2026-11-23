//! What the command line and the Greffier process say to each other over the socket
//! (`<app folder>/greffier.sock`, ADR 0006): one JSON object per line, a request then its
//! response, as many requests as wanted on one connection.
//!
//! Meetings, speakers, utterances, passages and actions travel as the user typed them
//! (`2026-11-20-0939`, `SPEAKER_01`, `#3`, `0:12:05`…): the process resolves them against its
//! live state ([`crate::server`]), so the command line never acts on a stale reading of the
//! folder.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// The protocol's version, given by [`Request::Hello`]: the command line refuses a process
/// that speaks another one.
pub const VERSION: u32 = 3;

/// A request, one JSON line: `"hello"`, `{"status":{"meeting":null}}`,
/// `{"execute":{"rename":{"meeting":"2026-11-20","title":"Revue"}}}`, `"open_window"`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Request {
    /// Who answers: [`Response::Hello`].
    Hello,
    /// The live state: [`Response::Status`] with the meetings in the queue or recording, plus
    /// `meeting` (as typed) whatever its status, if given.
    Status { meeting: Option<String> },
    /// Does something: [`Response::Ok`] or [`Response::Error`].
    Execute(Action),
    /// Shows the window (a second launch of the app found this one).
    OpenWindow,
}

/// What the command line asks the process to do. `meeting` and the other references are what
/// the user typed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    /// Puts the meeting in the queue from its first missing stage, with this speaker count if
    /// given.
    Process { meeting: String, speakers: Option<u8> },
    /// Puts the meeting back in the queue from this stage.
    Redo { meeting: String, stage: Stage },
    /// Stops its processing, or takes it out of the queue.
    Stop { meeting: String },
    Rename { meeting: String, title: String },
    NameSpeaker { meeting: String, speaker: String, name: String },
    /// `speaker` is the same voice as `into`.
    MergeSpeaker { meeting: String, speaker: String, into: String },
    SplitSpeaker { meeting: String, speaker: String },
    /// The utterance's new text.
    Correct { meeting: String, utterance: String, text: String },
    Resolve { meeting: String, passage: String, resolution: Resolution },
    /// Checks (or unchecks) the summary's `action`-th checkbox.
    CheckAction { meeting: String, action: String, done: bool },
    /// Moves the meeting's folder to the trash.
    Delete { meeting: String },
    /// Adds a meeting from these files (absolute paths): a media, a transcription, a summary.
    /// `process`: queue it at once (with `speakers` if given).
    Import { files: Vec<PathBuf>, title: Option<String>, speakers: Option<u8>, process: bool },
}

/// A processing stage, by the command line's name for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Transcription,
    Review,
    Summary,
}

impl From<Stage> for greffier_core::Stage {
    fn from(s: Stage) -> greffier_core::Stage {
        match s {
            Stage::Transcription => greffier_core::Stage::Transcription,
            Stage::Review => greffier_core::Stage::Review,
            Stage::Summary => greffier_core::Stage::Summary,
        }
    }
}

impl From<greffier_core::Stage> for Stage {
    fn from(s: greffier_core::Stage) -> Stage {
        match s {
            greffier_core::Stage::Transcription => Stage::Transcription,
            greffier_core::Stage::Review => Stage::Review,
            greffier_core::Stage::Summary => Stage::Summary,
        }
    }
}

/// How a passage to check is settled.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Resolution {
    Accept,
    Keep,
    Reopen,
    Correct(String),
}

impl From<Resolution> for greffier_core::Resolution {
    fn from(r: Resolution) -> greffier_core::Resolution {
        match r {
            Resolution::Accept => greffier_core::Resolution::AcceptHypothesis,
            Resolution::Keep => greffier_core::Resolution::KeepOriginal,
            Resolution::Reopen => greffier_core::Resolution::Reopen,
            Resolution::Correct(t) => greffier_core::Resolution::Correct(t),
        }
    }
}

/// A response, one JSON line.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Response {
    Hello(Hello),
    Status(Snapshot),
    Ok(Done),
    /// The request was refused or failed; nothing changed. `message` is in English.
    Error { message: String },
}

impl Response {
    pub fn error(message: impl Into<String>) -> Response {
        Response::Error { message: message.into() }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Hello {
    /// [`VERSION`].
    pub version: u32,
    pub pid: u32,
    /// The meetings folder it works on, canonical; `None` until one is chosen.
    pub meetings_folder: Option<PathBuf>,
    /// Started without a window (`--background`).
    pub background: bool,
}

/// What the process is doing.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    /// Recording, processing, a queue, an installation… ([`greffier_core::Greffier::busy`]).
    pub busy: bool,
    /// The meetings recording or in the queue (and the one asked for), queue order first.
    pub meetings: Vec<LiveMeeting>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LiveMeeting {
    /// Its identifier (`2026-11-20-0939`).
    pub identifier: Option<String>,
    pub folder_name: String,
    pub title: String,
    pub status: LiveStatus,
    /// Its place in the queue, from 1 (the meeting being processed is 1).
    pub rank: Option<usize>,
    /// Seconds until processed, the meetings ahead included.
    pub time_remaining: Option<f64>,
}

/// [`greffier_core::Status`] on the wire.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LiveStatus {
    Recording,
    RecordingInterrupted,
    ToProcess,
    Pending { stage: Stage },
    Running { stage: Stage, progress: f64 },
    Failed { stage: Stage, message: String },
    Done,
}

impl From<&greffier_core::Status> for LiveStatus {
    fn from(s: &greffier_core::Status) -> LiveStatus {
        use greffier_core::Status as S;
        match s {
            S::Recording => LiveStatus::Recording,
            S::RecordingInterrupted => LiveStatus::RecordingInterrupted,
            S::ToProcess => LiveStatus::ToProcess,
            S::Pending(stage) => LiveStatus::Pending { stage: (*stage).into() },
            S::Running { stage, progress } => LiveStatus::Running { stage: (*stage).into(), progress: *progress },
            S::Failed { stage, message } => LiveStatus::Failed { stage: (*stage).into(), message: message.clone() },
            S::Done => LiveStatus::Done,
        }
    }
}

impl From<&LiveStatus> for greffier_core::Status {
    fn from(s: &LiveStatus) -> greffier_core::Status {
        use greffier_core::Status as S;
        match s {
            LiveStatus::Recording => S::Recording,
            LiveStatus::RecordingInterrupted => S::RecordingInterrupted,
            LiveStatus::ToProcess => S::ToProcess,
            LiveStatus::Pending { stage } => S::Pending((*stage).into()),
            LiveStatus::Running { stage, progress } => S::Running { stage: (*stage).into(), progress: *progress },
            LiveStatus::Failed { stage, message } => S::Failed { stage: (*stage).into(), message: message.clone() },
            LiveStatus::Done => S::Done,
        }
    }
}

/// A request carried out.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Done {
    /// One short line in English: "2026-11-20-1000 · renamed: Review".
    pub message: String,
    /// The meeting's identifier after the action (the new meeting's, for an import).
    pub meeting: Option<String>,
    /// Its folder's name after the action.
    pub folder_name: Option<String>,
}

/// One message as one line of JSON, newline included.
pub fn encode(message: &impl Serialize) -> String {
    let mut s = serde_json::to_string(message).unwrap_or_else(|e| format!("{{\"error\":{{\"message\":\"{e}\"}}}}"));
    s.push('\n');
    s
}

impl crate::LiveSource for Snapshot {
    fn live(&self, folder_name: &str) -> Option<crate::Live> {
        let m = self.meetings.iter().find(|m| m.folder_name == folder_name)?;
        Some(crate::Live { status: (&m.status).into(), rank: m.rank, time_remaining: m.time_remaining })
    }
}
