//! The interfaces of everything that leaves the process. The core only knows them: the real
//! engines (ScreenCaptureKit, whisper.cpp, pyannote, the `claude` CLI, the downloads) and the
//! fake adapters of the tests and the prototype implement them.
//!
//! A long job returns a [`Task`] that the core polls on each [`crate::Greffier::advance`]: a
//! real adapter runs its job on its own thread and reports its progress; a fake one advances
//! with the elapsed time it is given, which makes the tests deterministic.

use std::path::PathBuf;

pub use crate::lexicon::LexiconAddition;
use crate::meeting::{Stage, Word, Marker, SpeakerTurn};
use crate::time::DateTime;

/// Where a task stands.
#[derive(Debug, PartialEq)]
pub enum TaskProgress<T> {
    /// In progress, from 0 to 1.
    Running(f64),
    Done(T),
    Failed(String),
}

/// A job running outside the core.
pub trait Task<T> {
    /// Called on each advance of the core; `elapsed` is the time since the previous call, in
    /// seconds. After `Done` or `Failed`, the task is no longer polled.
    fn poll(&mut self, elapsed: f64) -> TaskProgress<T>;
    /// Abandons the job (processing stopped, meeting deleted).
    fn stop(&mut self) {}
}

pub type BoxedTask<T> = Box<dyn Task<T>>;

// --- Capture -------------------------------------------------------------------------------

/// The recorded sources.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sources {
    pub screen: bool,
    pub mac_audio: bool,
    pub micro: bool,
}

impl Default for Sources {
    fn default() -> Sources {
        Sources { screen: true, mac_audio: true, micro: true }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CaptureRequest {
    /// The recording file, to be written as it goes.
    pub file: PathBuf,
    pub sources: Sources,
}

/// Live levels, from 0 to 1.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Levels {
    pub micro: f32,
    pub mac_audio: f32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CaptureState {
    /// Recorded duration, in seconds.
    pub duration: f64,
    pub levels: Levels,
    /// The capture stopped on its own (macOS cut the stream, the display disappeared…). The core
    /// then calls [`Capture::stop`] to finish the file and keeps the meeting as
    /// "recording interrupted".
    pub error: Option<String>,
}

/// Where a macOS permission stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PermissionState {
    Granted,
    /// Denied: macOS no longer asks again, System Settings must be used.
    Denied,
    /// Never requested: requesting it shows the macOS prompt.
    ToRequest,
}

/// The permissions each source needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Permission {
    Screen,
    Micro,
    MacAudio,
}

impl Permission {
    pub const ALL: [Permission; 3] = [Permission::Screen, Permission::MacAudio, Permission::Micro];

    /// The source's name, as in the UI.
    pub fn name(self) -> &'static str {
        match self {
            Permission::Screen => "Écran",
            Permission::Micro => "Micro",
            Permission::MacAudio => "Son du Mac",
        }
    }

    /// The System Settings › Privacy & Security section that grants it.
    pub fn section(self) -> &'static str {
        match self {
            Permission::Screen | Permission::MacAudio => "Enregistrement de l'écran et de l'audio du système",
            Permission::Micro => "Micro",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Permissions {
    pub screen: PermissionState,
    pub micro: PermissionState,
    pub mac_audio: PermissionState,
}

impl Permissions {
    pub const GRANTED: Permissions = Permissions { screen: PermissionState::Granted, micro: PermissionState::Granted, mac_audio: PermissionState::Granted };

    pub fn state(&self, a: Permission) -> PermissionState {
        match a {
            Permission::Screen => self.screen,
            Permission::Micro => self.micro,
            Permission::MacAudio => self.mac_audio,
        }
    }

    pub fn state_mut(&mut self, a: Permission) -> &mut PermissionState {
        match a {
            Permission::Screen => &mut self.screen,
            Permission::Micro => &mut self.micro,
            Permission::MacAudio => &mut self.mac_audio,
        }
    }
}

impl Sources {
    /// The permissions needed by the enabled sources.
    pub fn permissions(&self) -> impl Iterator<Item = Permission> {
        [(self.screen, Permission::Screen), (self.mac_audio, Permission::MacAudio), (self.micro, Permission::Micro)].into_iter().filter(|(on, _)| *on).map(|(_, a)| a)
    }

    pub fn none_enabled(&self) -> bool {
        !(self.screen || self.mac_audio || self.micro)
    }
}

/// Records the screen, the Mac's audio and the microphone into a file.
pub trait Capture {
    /// The state of the macOS permissions (fast read: called by the UI).
    fn permissions(&self) -> Permissions {
        Permissions::GRANTED
    }
    /// Shows the macOS prompt for this permission, if it has never been requested.
    fn request_permission(&mut self, _permission: Permission) {}
    fn start(&mut self, request: CaptureRequest) -> Result<(), String>;
    fn poll(&mut self, elapsed: f64) -> CaptureState;
    /// Stops and finishes writing the file; returns the recorded duration.
    fn stop(&mut self) -> Result<f64, String>;
}

// --- Transcriber and diarizer --------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct TranscriptionRequest {
    /// The meeting's recording (converting its mix track is the adapter's business).
    pub audio: PathBuf,
    /// The meeting's duration, to estimate progress.
    pub duration: f64,
    /// The lexicon's Vocabulaire section, given as the prompt.
    pub vocabulary: String,
}

/// An audio file and a vocabulary in; timestamped words with their confidence out.
pub trait Transcriber {
    fn transcribe(&mut self, request: TranscriptionRequest) -> BoxedTask<Vec<Word>>;
}

#[derive(Clone, Debug, PartialEq)]
pub struct DiarizationRequest {
    pub audio: PathBuf,
    pub duration: f64,
    /// The given speaker count, otherwise detected.
    pub speaker_count: Option<u8>,
}

/// An audio file and an optional speaker count in; exclusive speaker turns out.
pub trait Diarizer {
    fn diarize(&mut self, request: DiarizationRequest) -> BoxedTask<Vec<SpeakerTurn>>;
}

/// No diarization: no speaker turns, every word goes to a single speaker (for the tests).
pub struct SingleSpeaker;

impl Diarizer for SingleSpeaker {
    fn diarize(&mut self, _: DiarizationRequest) -> BoxedTask<Vec<SpeakerTurn>> {
        struct NoTurns;
        impl Task<Vec<SpeakerTurn>> for NoTurns {
            fn poll(&mut self, _: f64) -> TaskProgress<Vec<SpeakerTurn>> {
                TaskProgress::Done(Vec::new())
            }
        }
        Box::new(NoTurns)
    }
}

// --- Agent ---------------------------------------------------------------------------------

/// An utterance as given to the agent.
#[derive(Clone, Debug, PartialEq)]
pub struct AgentUtterance {
    /// The utterance's index in the meeting; the results refer to it.
    pub number: usize,
    /// « SPEAKER_00 »
    pub speaker: String,
    /// The name given to the speaker, if any.
    pub name: Option<String>,
    pub start: f64,
    pub end: f64,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ReviewRequest {
    /// For relistening to doubtful passages.
    pub recording: PathBuf,
    /// The agent's working directory: it can reread previous meetings there.
    pub meetings_folder: PathBuf,
    /// The meeting's folder in the meetings folder.
    pub folder_name: String,
    pub title: String,
    pub start: DateTime,
    /// The given speaker count, if it was given.
    pub speaker_count: Option<u8>,
    /// The utterances to review: the raw transcription, from before any review.
    pub utterances: Vec<AgentUtterance>,
    /// The lexicon (Markdown), empty if there is none.
    pub lexicon: String,
    /// The lexicon's Vocabulaire section: the relisten prompt, as for the transcription.
    pub vocabulary: String,
}

/// A confident correction: `before` becomes `after` in utterance `utterance`.
#[derive(Clone, Debug, PartialEq)]
pub struct ConfidentCorrection {
    pub utterance: usize,
    pub before: String,
    pub after: String,
}

/// The conversation proves the utterance is from another speaker.
#[derive(Clone, Debug, PartialEq)]
pub struct SpeakerChange {
    pub utterance: usize,
    pub speaker: String,
}

/// A doubtful passage that relistening made it possible to correct: `excerpt` becomes `after`.
#[derive(Clone, Debug, PartialEq)]
pub struct CorrectedPassage {
    pub utterance: usize,
    pub excerpt: String,
    pub after: String,
    pub relisten: String,
}

/// A passage the review could not resolve, even after relistening.
#[derive(Clone, Debug, PartialEq)]
pub struct FlaggedPassage {
    pub utterance: usize,
    pub excerpt: String,
    pub hypothesis: Option<String>,
    pub relisten: String,
}

/// The result of the review's two passes; Greffier applies it itself.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReviewResult {
    pub session: Option<String>,
    pub corrections: Vec<ConfidentCorrection>,
    pub speaker_changes: Vec<SpeakerChange>,
    pub corrected_passages: Vec<CorrectedPassage>,
    pub passages_to_check: Vec<FlaggedPassage>,
    /// The errors learned; Greffier only writes to the lexicon those it does not know.
    pub lexicon_additions: Vec<LexiconAddition>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AgentSpeaker {
    pub label: String,
    pub name: Option<String>,
    pub proposal: Option<String>,
    pub justification: String,
    /// The merge already suggested ("SPEAKER_00"), if any.
    pub proposed_same_voice: Option<String>,
    /// Merged by the user into this speaker ("SPEAKER_00"): its utterances are under that one
    /// in the readable transcription.
    pub merged_into: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SummaryRequest {
    /// The agent's working directory.
    pub meetings_folder: PathBuf,
    /// The settings' context folders, open to the agent besides the meetings folder.
    pub context_folders: Vec<PathBuf>,
    /// The file the agent must write.
    pub summary_file: PathBuf,
    /// The meeting's sheet and readable transcription, which the agent reads.
    pub sheet: PathBuf,
    pub transcription: PathBuf,
    pub title: String,
    pub start: DateTime,
    pub duration: f64,
    pub utterances: Vec<AgentUtterance>,
    pub speakers: Vec<AgentSpeaker>,
    pub markers: Vec<Marker>,
    /// The previous summary when the summary is redone.
    pub current_summary: Option<String>,
}

/// What the agent suggests for a speaker (an element of `participants` in its JSON answer: the
/// prompt's key does not change).
#[derive(Clone, Debug, PartialEq)]
pub struct SummarySpeaker {
    /// « SPEAKER_00 »
    pub speaker: String,
    /// Empty when the agent does not know who it is (the justification says why).
    pub proposed_name: String,
    pub justification: String,
    /// Probably the same voice as another speaker ("SPEAKER_00").
    pub same_voice_as: Option<String>,
}

/// What the agent returns after writing the summary file.
#[derive(Clone, Debug, PartialEq)]
pub struct SummaryResult {
    pub session: Option<String>,
    pub title: String,
    pub brief: String,
    /// The 3 main actions, at most.
    pub actions: Vec<String>,
    pub speakers: Vec<SummarySpeaker>,
}

/// The two-pass review (relistening included) and the summary, handed to an agent.
pub trait Agent {
    fn review(&mut self, request: ReviewRequest) -> BoxedTask<ReviewResult>;
    /// Writes `request.summary_file` and returns title, brief, actions and speakers.
    fn summarize(&mut self, request: SummaryRequest) -> BoxedTask<SummaryResult>;
}

// --- Resources -----------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub enum ResourceState {
    Missing,
    /// An interrupted download (app quit, connection lost): it will resume where it stopped. In
    /// bytes.
    Interrupted { received: u64, total: u64 },
    /// In bytes.
    Downloading { received: u64, total: u64 },
    /// Computing the sha256, or checking the `claude` CLI.
    Verifying,
    /// Installing the Python environment, with no measurable progress.
    Installing,
    Ready,
    Failed(String),
}

/// A model, the Python environment, or a check (the `claude` CLI).
#[derive(Clone, Debug, PartialEq)]
pub struct Resource {
    pub name: String,
    /// Where it comes from, or what the check found ("huggingface.co", "2.1.3 · connectée").
    pub detail: String,
    pub state: ResourceState,
    /// Space taken on disk, in bytes (partial downloads included).
    pub size: u64,
    /// Download size, in bytes (0: nothing to download, or unknown size).
    pub total: u64,
    /// What to do when it is missing and the app cannot install it itself.
    pub help: Option<String>,
    /// The processing stages that cannot start without it.
    pub stages: Vec<Stage>,
}

/// Downloads and checks the models and the Python environment; checks the `claude` CLI.
pub trait Resources {
    fn state(&mut self) -> Vec<Resource>;
    /// Downloads, installs and checks what is missing, and reruns the checks.
    fn install(&mut self) -> BoxedTask<()>;
    /// Reruns the checks (the `claude` CLI) without downloading anything.
    fn check(&mut self) -> BoxedTask<()>;
}

// --- Clock ---------------------------------------------------------------------------------

/// Local time: the start of recordings and today's date.
pub trait Clock {
    fn now(&self) -> DateTime;
    /// An instant (a media file's creation date, a file's modification date) in local time.
    /// UTC by default: the real clock applies the Mac's time zone.
    fn to_local_time(&self, instant: std::time::SystemTime) -> DateTime {
        let seconds = match instant.duration_since(std::time::UNIX_EPOCH) {
            Ok(d) => d.as_secs() as i64,
            Err(e) => -(e.duration().as_secs() as i64),
        };
        DateTime::from_unix(seconds)
    }
}

// --- Trash ----------------------------------------------------------------------------------

/// The Mac's trash: a deleted meeting sends its whole folder there, so the deletion can be
/// undone.
pub trait Trash {
    /// Moves this folder (or file) to the trash. The error is a message for the user.
    fn trash(&self, path: &std::path::Path) -> Result<(), String>;
}

// --- MP4 muxing -----------------------------------------------------------------------------

/// Moves an imported media file of the MP4 family (QuickTime `.mov`, `.m4a`, `.m4v`) into an
/// `.mp4` file, without re-encoding it: a meeting's recording is named `… - enregistrement.mp4`.
pub trait Mp4Muxer {
    /// Writes `to` (an `.mp4`) with the tracks of `from`, without touching `from`. The error is
    /// a message; the core then copies the media as is, with its extension.
    fn to_mp4(&self, from: &std::path::Path, to: &std::path::Path) -> Result<(), String>;
    /// The duration in seconds of an imported medium that is not an MP4 (`.wav`, `.mp3`…),
    /// whose boxes the core cannot read. `None` if unknown.
    fn duration(&self, _media: &std::path::Path) -> Option<f64> {
        None
    }
}

/// All the adapters plugged into the core.
pub struct Adapters {
    pub capture: Box<dyn Capture>,
    pub transcriber: Box<dyn Transcriber>,
    pub diarizer: Box<dyn Diarizer>,
    pub agent: Box<dyn Agent>,
    pub resources: Box<dyn Resources>,
    pub clock: Box<dyn Clock>,
    pub trash: Box<dyn Trash>,
    pub mp4_muxing: Box<dyn Mp4Muxer>,
}
