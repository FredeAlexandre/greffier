//! Greffier's core, without a UI.
//!
//! [`Greffier`] owns the state of the meetings, the recording and the processing queue. It is
//! driven by [`Command`]s ([`Greffier::execute`]) and time is moved forward with
//! [`Greffier::advance`]; both return [`Event`]s. The UI only reads the state
//! (`meetings`, `recording`…) and calls these two functions.
//!
//! Everything that leaves the process goes through the [`adapters`]: capture, transcriber,
//! diarizer, agent, resources, trash, MP4 muxing (and the clock). The core only knows these
//! interfaces; the real adapters live in other crates (`greffier-tools`: the `claude` CLI,
//! curl, uv; `greffier-engines`: whisper.cpp, pyannote; `greffier-capture`: ScreenCaptureKit,
//! trash, MP4 muxing). The app's resources (models, private Python, `claude` check) are
//! described in [`resources`].

#![forbid(unsafe_code)]

pub mod adapters;
pub mod attribution;
pub mod summary;
pub mod folder;
pub mod fake;
pub mod demo;
pub mod lexicon;
pub mod search;
pub mod settings;
pub mod resources;
mod meeting;
mod deletion;
mod watch;
mod time;
mod title;

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::path::PathBuf;

use adapters::*;
pub use folder::{MeetingsFolder, Role};
pub use meeting::*;
pub use time::*;

/// The settings that change the core's behavior.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// Put the meeting in the processing queue as soon as the recording stops.
    pub process_on_stop: bool,
    pub sources: Sources,
    /// The lexicon (`lexique.md` in the app folder). Its Vocabulaire section is given to the
    /// transcriber; it is reread at the start of each transcription. Missing or unreadable: no
    /// vocabulary.
    pub lexicon: Option<PathBuf>,
    /// The folders open to the summary agent besides the meetings folder (notes, projects…).
    pub context_folders: Vec<PathBuf>,
    /// The search cache folder (`recherche/` in the app folder): the index is kept there from
    /// one launch to the next, and rebuilt if missing. Without it, the index stays in memory.
    pub search_cache: Option<PathBuf>,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings { process_on_stop: true, sources: Sources::default(), lexicon: None, context_folders: Vec::new(), search_cache: None }
    }
}

/// How the user resolves a passage to check.
#[derive(Clone, Debug, PartialEq)]
pub enum Resolution {
    AcceptHypothesis,
    KeepOriginal,
    Correct(String),
    /// Puts the passage back to pending.
    Reopen,
}

/// Everything that can be asked of the core.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    StartRecording,
    StopRecording,
    AddMarker,
    /// The speaker count given during the recording (`None`: detected).
    RecordingSpeakerCount(Option<u8>),
    /// The speaker count given before processing (`None`: detected).
    SpeakerCount { meeting: MeetingId, number: Option<u8> },
    /// Puts the meeting in the queue, from the first stage it is missing.
    Process(MeetingId),
    /// The user keeps a recording offered for deletion ([`Event::DeletionProposed`]): it is
    /// queued as a longer recording is at stop, if [`Settings::process_on_stop`] is set. Changes
    /// nothing if it is no longer to process (already queued, processed by hand…).
    KeepShortRecording(MeetingId),
    /// Puts the meeting back in the queue from this stage; the following ones are redone too.
    Redo { meeting: MeetingId, stage: Stage },
    /// Stops processing (or takes the meeting out of the queue): it goes back to to-process.
    StopProcessing(MeetingId),
    /// Moves the meeting's folder to the Mac's trash and forgets it (rejected if the trash
    /// refuses).
    Delete(MeetingId),
    /// Confirms or corrects a speaker's name (the one replacing it if it is merged).
    NameSpeaker { meeting: MeetingId, speaker: usize, name: String },
    /// `speaker` is the same voice as `into`, which replaces it everywhere (merges followed on
    /// both sides) and takes its name if it has none.
    MergeSpeakers { meeting: MeetingId, speaker: usize, into: usize },
    /// Undoes the merge of `speaker`: it gets its utterances back, without a name.
    SplitSpeaker { meeting: MeetingId, speaker: usize },
    /// Resolves a passage to check: its utterance's text takes the hypothesis or the correction
    /// (reopening puts the excerpt back), and both transcriptions are rewritten, without the marking.
    Resolve { meeting: MeetingId, passage: usize, resolution: Resolution },
    /// Corrects by hand one or more words of an utterance, whose new text is `text`. The change,
    /// widened to whole words, is kept as a manual correction (and reapplied if the review is
    /// redone). A passage to check is resolved with [`Command::Resolve`].
    CorrectUtterance { meeting: MeetingId, utterance: usize, text: String },
    /// Checks or unchecks the `action`-th checkbox of the summary.
    CheckAction { meeting: MeetingId, action: usize, done: bool },
    /// The meeting was opened: it is no longer new.
    MarkRead(MeetingId),
    /// Changes the title: the folder and its files are renamed, links included. Rejected during
    /// a stage (the adapter writes into the folder).
    ChangeTitle { meeting: MeetingId, title: String },
    ChangeSettings(Settings),
    /// Downloads, installs and checks the missing resources.
    InstallResources,
    /// Shows the macOS prompt for this permission.
    RequestPermission(Permission),
    /// Reruns the resource checks (the `claude` CLI) without downloading anything.
    CheckResources,
    /// Adds a meeting from existing files, copied into a new meeting folder (see
    /// [`Greffier::propose_import`]).
    Import(folder::Import),
}

/// What happened in the core that the UI may want to announce.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    MeetingRecorded(MeetingId),
    /// The capture stopped on its own, or stopping it failed: the meeting is kept as
    /// "recording interrupted" with what was captured, and can be processed.
    RecordingInterrupted { meeting: MeetingId, message: String },
    /// The recording lasted less than [`MIN_DURATION`]: probably a false start, which the UI
    /// offers to delete. The meeting is not queued in the meantime: it is processed (if
    /// [`Settings::process_on_stop`]) once kept with [`Command::KeepShortRecording`], and stays
    /// to process if no answer comes (the app quits).
    DeletionProposed(MeetingId),
    MeetingImported(MeetingId),
    StageDone { meeting: MeetingId, stage: Stage },
    StageFailed { meeting: MeetingId, stage: Stage, message: String },
    SummaryReady { meeting: MeetingId, title: String },
    ResourcesInstalled,
    InstallationFailed(String),
    /// A meeting file could not be written to the meetings folder.
    WriteFailed { meeting: MeetingId, message: String },
    /// A meeting file changed outside the app: the meeting was reread.
    MeetingReviewed(MeetingId),
    /// A meeting folder appeared outside the app.
    MeetingAppeared(MeetingId),
    /// The meeting's folder disappeared outside the app: it is no longer in the list.
    MeetingDisappeared(MeetingId),
}

/// Below this duration (in seconds), a recording is offered for deletion.
pub const MIN_DURATION: f64 = 60.0;

/// Why a command was not executed. The state has not changed.
#[derive(Clone, Debug, PartialEq)]
pub enum Rejection {
    UnknownMeeting(MeetingId),
    Impossible(String),
}

impl fmt::Display for Rejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Rejection::UnknownMeeting(id) => write!(f, "réunion inconnue ({id})"),
            Rejection::Impossible(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for Rejection {}

fn impossible<T>(message: impl Into<String>) -> Result<T, Rejection> {
    Err(Rejection::Impossible(message.into()))
}

/// What must stay awake, and why (see [`Greffier::keep_mac_awake`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Wake {
    /// The name of the sleep-prevention assertion ("Greffier enregistre une réunion").
    pub reason: &'static str,
    /// The display too: once the display is off, ScreenCaptureKit captures nothing. Otherwise,
    /// only the Mac stays awake and the display can turn off.
    pub screen: bool,
}

/// The recording in progress.
#[derive(Clone, Debug, PartialEq)]
pub struct Recording {
    pub start: DateTime,
    /// Recorded duration, in seconds.
    pub duration: f64,
    pub levels: Levels,
    /// Times of the markers added.
    pub markers: Vec<f64>,
    pub speaker_count: Option<u8>,
    pub folder_name: String,
    /// The recorded sources (those of the settings at start).
    pub sources: Sources,
}

enum Part<T> {
    Running(BoxedTask<T>),
    Done(T),
}

impl<T> Part<T> {
    fn poll(&mut self, elapsed: f64) -> Result<f64, String> {
        let a = match self {
            Part::Running(t) => t.poll(elapsed),
            Part::Done(_) => return Ok(1.0),
        };
        match a {
            TaskProgress::Running(p) => Ok(p),
            TaskProgress::Done(v) => {
                *self = Part::Done(v);
                Ok(1.0)
            }
            TaskProgress::Failed(e) => Err(e),
        }
    }
    fn stop(&mut self) {
        if let Part::Running(t) = self {
            t.stop();
        }
    }
    fn on_done(&self) -> bool {
        matches!(self, Part::Done(_))
    }
    fn take(self) -> Option<T> {
        match self {
            Part::Done(v) => Some(v),
            Part::Running(_) => None,
        }
    }
}

enum Job {
    /// Transcription and diarization run in parallel.
    Transcription { words: Part<Vec<Word>>, turns: Part<Vec<SpeakerTurn>> },
    Review(Part<ReviewResult>),
    Summary { result: Part<SummaryResult> },
}

impl Job {
    fn stage(&self) -> Stage {
        match self {
            Job::Transcription { .. } => Stage::Transcription,
            Job::Review(_) => Stage::Review,
            Job::Summary { .. } => Stage::Summary,
        }
    }
    fn stop(&mut self) {
        match self {
            Job::Transcription { words, turns } => {
                words.stop();
                turns.stop();
            }
            Job::Review(p) => p.stop(),
            Job::Summary { result, .. } => result.stop(),
        }
    }
}

struct Processing {
    meeting: MeetingId,
    job: Job,
}

enum StageOutput {
    Transcription(Vec<Word>, Vec<SpeakerTurn>),
    Review(ReviewResult),
    Summary(SummaryResult),
}

/// Greffier's core.
pub struct Greffier {
    folder: MeetingsFolder,
    adapters: Adapters,
    settings: Settings,
    /// Most recent first.
    meetings: Vec<Meeting>,
    next_id: MeetingId,
    recording: Option<Recording>,
    /// The pending meetings, in the order they will be processed.
    file: VecDeque<MeetingId>,
    processing: Option<Processing>,
    installation: Option<BoxedTask<()>>,
    verification: Option<BoxedTask<()>>,
    /// The state of the resources, reread on each advance.
    resources: Vec<Resource>,
    /// The fingerprint of each meeting folder at the core's last pass (see
    /// [`Greffier::rescan_folder`]).
    fingerprints: HashMap<String, folder::watch::Fingerprints>,
    /// The ⌘K search index (see [`Greffier::search`]).
    index: search::Index,
}

impl Greffier {
    /// The core on a meetings folder, with the meetings it already contains.
    pub fn open(folder: impl Into<PathBuf>, adapters: Adapters) -> Greffier {
        let folder = MeetingsFolder::new(folder);
        let meetings = folder.read_all(1);
        Greffier::with_meetings(folder.root(), adapters, meetings)
    }

    /// The core with already known meetings (the prototype's fake data).
    pub fn with_meetings(folder: impl Into<PathBuf>, adapters: Adapters, mut meetings: Vec<Meeting>) -> Greffier {
        let mut file = VecDeque::new();
        for r in &mut meetings {
            if let Status::Running { stage, .. } = r.status {
                r.status = Status::Pending(stage);
            }
            if let Status::Pending(_) = r.status {
                file.push_back(r.id);
            }
        }
        let next_id = meetings.iter().map(|r| r.id + 1).max().unwrap_or(1);
        let mut adapters = adapters;
        let resources = adapters.resources.state();
        let folder = MeetingsFolder::new(folder);
        let fingerprints = folder.all_fingerprints();
        let mut greffier = Greffier {
            folder,
            adapters,
            settings: Settings::default(),
            meetings,
            next_id,
            recording: None,
            file,
            processing: None,
            installation: None,
            verification: None,
            resources,
            fingerprints,
            index: search::Index::default(),
        };
        greffier.indexer();
        greffier
    }

    // --- Reading the state ---------------------------------------------------------------

    pub fn meetings(&self) -> &[Meeting] {
        &self.meetings
    }

    pub fn meeting(&self, id: MeetingId) -> Option<&Meeting> {
        self.meetings.iter().find(|r| r.id == id)
    }

    pub fn recording(&self) -> Option<&Recording> {
        self.recording.as_ref()
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// A meeting's folder on disk.
    pub fn meeting_folder(&self, id: MeetingId) -> Option<PathBuf> {
        self.meeting(id).map(|r| self.folder.folder(&r.folder_name))
    }

    /// A meeting's recording file, if it exists (an imported media file that could not be muxed
    /// to MP4 keeps its extension).
    pub fn recording_file(&self, id: MeetingId) -> Option<PathBuf> {
        self.meeting(id).and_then(|r| self.folder.recording(&r.folder_name))
    }

    /// A meeting's recording file, ready for the player: a video imported before Greffier gave
    /// it a picture from 0 is repaired first, once, in place ([`folder::mp4::fill_leading_video_gap`]),
    /// and Greffier notes its own write so that the folder's next rescan does not take it for a
    /// change made outside the app.
    pub fn recording_to_play(&mut self, id: MeetingId) -> Option<PathBuf> {
        let file = self.recording_file(id)?;
        if folder::mp4::fill_leading_video_gap(&file).unwrap_or(false) {
            let name = self.meeting(id)?.folder_name.clone();
            self.note_end(&name, &[Role::Recording]);
        }
        Some(file)
    }

    /// The settings' lexicon, read now; without a lexicon (or unreadable), the bundled lexicon.
    fn lexicon_text(&self) -> String {
        self.settings.lexicon.as_ref().and_then(|c| std::fs::read_to_string(c).ok()).unwrap_or_else(|| lexicon::INITIAL_LEXICON.to_string())
    }

    /// What the lexicon counts (people, right words, known errors), for the settings.
    pub fn lexicon(&self) -> lexicon::Overview {
        lexicon::preview(&self.lexicon_text())
    }

    /// The lexicon's people, suggested for naming speakers.
    pub fn people(&self) -> Vec<String> {
        lexicon::people(&self.lexicon_text())
    }

    pub fn folder(&self) -> &MeetingsFolder {
        &self.folder
    }

    /// A meeting's rank in the queue: 0 for the one being processed.
    pub fn rank(&self, id: MeetingId) -> Option<usize> {
        let running = self.processing.as_ref().map(|t| t.meeting);
        running.into_iter().chain(self.file.iter().copied()).position(|x| x == id)
    }

    /// The estimated time until a queued meeting's processing ends, in seconds: the rest of its
    /// own and that of the meetings ahead of it.
    pub fn time_remaining(&self, id: MeetingId) -> Option<f64> {
        let rank = self.rank(id)?;
        let running = self.processing.as_ref().map(|t| t.meeting);
        running.into_iter().chain(self.file.iter().copied()).take(rank + 1).map(|x| self.meeting(x).and_then(Meeting::estimated_time_remaining)).sum()
    }

    pub fn today(&self) -> DateTime {
        self.adapters.clock.now()
    }

    /// The macOS permissions the sources need.
    pub fn permissions(&self) -> Permissions {
        self.adapters.capture.permissions()
    }

    /// The models, the Python environment and the `claude` check, with their state and the
    /// space they take.
    pub fn resources(&self) -> &[Resource] {
        &self.resources
    }

    pub fn installing(&self) -> bool {
        self.installation.is_some()
    }

    pub fn verification_in_progress(&self) -> bool {
        self.verification.is_some()
    }

    /// What the import window proposes for dropped or chosen files: the media, the transcription
    /// and the summary, the date and the title, to edit before sending [`Command::Import`].
    /// Writes nothing.
    pub fn propose_import(&self, files: &[PathBuf]) -> Result<folder::Import, Rejection> {
        folder::import::propose(files, |t| self.adapters.clock.to_local_time(t)).map_err(Rejection::Impossible)
    }

    /// True while time matters: recording, processing, queue, installation or resource
    /// check.
    pub fn busy(&self) -> bool {
        self.recording.is_some() || self.processing.is_some() || !self.file.is_empty() || self.installation.is_some() || self.verification.is_some()
    }

    /// Why the Mac must not go to sleep automatically, while it must not: during a recording
    /// (the display too), and during processing (a meeting running or pending in the queue). The
    /// UI turns it into a macOS sleep-prevention assertion.
    pub fn keep_mac_awake(&self) -> Option<Wake> {
        if self.recording.is_some() {
            Some(Wake { reason: "Greffier enregistre une réunion", screen: true })
        } else if self.processing.is_some() || !self.file.is_empty() {
            Some(Wake { reason: "Greffier traite une réunion", screen: false })
        } else {
            None
        }
    }

    // --- Commands ------------------------------------------------------------------------

    pub fn execute(&mut self, command: Command) -> Result<Vec<Event>, Rejection> {
        let mut ev = Vec::new();
        let affected = match &command {
            Command::SpeakerCount { meeting, .. }
            | Command::Redo { meeting, .. }
            | Command::NameSpeaker { meeting, .. }
            | Command::MergeSpeakers { meeting, .. }
            | Command::SplitSpeaker { meeting, .. }
            | Command::Resolve { meeting, .. }
            | Command::CorrectUtterance { meeting, .. }
            | Command::CheckAction { meeting, .. } => Some(*meeting),
            Command::Process(id) | Command::KeepShortRecording(id) | Command::StopProcessing(id) | Command::MarkRead(id) => Some(*id),
            _ => None,
        };
        match command {
            Command::StartRecording => self.start_recording()?,
            Command::StopRecording => self.stop_recording(&mut ev)?,
            Command::AddMarker => {
                let Some(e) = &mut self.recording else { return impossible("aucun enregistrement en cours") };
                e.markers.push(e.duration);
                self.write_recording_sheet();
            }
            Command::RecordingSpeakerCount(n) => {
                let Some(e) = &mut self.recording else { return impossible("aucun enregistrement en cours") };
                e.speaker_count = n;
                self.write_recording_sheet();
            }
            Command::SpeakerCount { meeting, number } => self.meeting_mut(meeting)?.speaker_count = number,
            Command::Process(id) => {
                let r = self.meeting_mut(id)?;
                let stage = match &r.status {
                    Status::Failed { stage, .. } => *stage,
                    Status::ToProcess | Status::RecordingInterrupted => match r.completed_stage {
                        None => Stage::Transcription,
                        Some(e) => match e.next() {
                            Some(s) => s,
                            None => return impossible("la réunion est déjà traitée"),
                        },
                    },
                    s => return impossible(format!("la réunion ne peut pas être traitée ({s:?})")),
                };
                self.enqueue(id, stage);
            }
            Command::KeepShortRecording(id) => {
                let r = self.meeting_mut(id)?;
                if r.status == Status::ToProcess && r.completed_stage.is_none() && self.settings.process_on_stop {
                    self.enqueue(id, Stage::Transcription);
                }
            }
            Command::Redo { meeting, stage } => {
                let r = self.meeting_mut(meeting)?;
                if !r.can_redo(stage) {
                    return impossible(if r.status.in_queue() { "la réunion est déjà dans la file" } else { "les étapes précédentes ne sont pas faites" });
                }
                r.completed_stage = stage.previous();
                self.enqueue(meeting, stage);
            }
            Command::StopProcessing(id) => {
                let r = self.meeting_mut(id)?;
                if !r.status.in_queue() {
                    return impossible("la réunion n'est pas en traitement");
                }
                r.status = Status::ToProcess;
                self.remove_from_queue(id);
            }
            Command::Delete(id) => self.delete(id)?,
            Command::NameSpeaker { meeting, speaker, name } => {
                let name = name.trim();
                if name.is_empty() {
                    return impossible("le nom est vide");
                }
                let r = self.meeting_mut(meeting)?;
                speaker_mut(r, speaker)?;
                // Naming a merged speaker names the one replacing it: a split leaves that name to
                // the one that stays, and the merged speaker finds its own again.
                r.name(speaker, name.to_string());
                let i = r.displayed_speaker(speaker);
                r.speakers[i].suggested_merge = None;
            }
            Command::MergeSpeakers { meeting, speaker, into } => {
                let r = self.meeting_mut(meeting)?;
                speaker_mut(r, speaker)?;
                speaker_mut(r, into)?;
                let (i, j) = (r.displayed_speaker(speaker), r.displayed_speaker(into));
                if !r.merge(i, j) {
                    return impossible("ces deux locuteurs sont déjà la même voix");
                }
                r.speakers[speaker].suggested_merge = None;
                r.speakers[i].suggested_merge = None;
            }
            Command::SplitSpeaker { meeting, speaker } => {
                let r = self.meeting_mut(meeting)?;
                speaker_mut(r, speaker)?;
                if !r.split(speaker) {
                    return impossible("ce locuteur n'est pas fusionné");
                }
            }
            Command::Resolve { meeting, passage, resolution } => {
                let r = self.meeting_mut(meeting)?;
                transcription_editable(r)?;
                resolve(r, passage, resolution)?;
            }
            Command::CorrectUtterance { meeting, utterance, text } => {
                let r = self.meeting_mut(meeting)?;
                transcription_editable(r)?;
                correct_utterance(r, utterance, text)?;
            }
            Command::CheckAction { meeting, action, done } => {
                let r = self.meeting(meeting).ok_or(Rejection::UnknownMeeting(meeting))?;
                if action >= summary::checkbox_count(&r.summary) {
                    return impossible("action inconnue");
                }
                // The summary file is the agent's: only the checkbox is changed there, and it is authoritative.
                let name = r.folder_name.clone();
                let file = self.folder.tick_in_summary(&name, action, done).map_err(Rejection::Impossible)?;
                let r = self.meeting_mut(meeting)?;
                match file {
                    Some(md) => r.set_summary(md),
                    None => _ = summary::tick(&mut r.summary, action, done),
                }
                self.note_end(&name, &[Role::Summary]);
            }
            Command::MarkRead(id) => self.meeting_mut(id)?.is_new = false,
            Command::ChangeTitle { meeting, title } => {
                let title = title.trim();
                if title.is_empty() {
                    return impossible("le titre est vide");
                }
                let r = self.meeting_mut(meeting)?;
                if matches!(r.status, Status::Running { .. }) {
                    return impossible("le titre pourra changer à la fin de l'étape en cours");
                }
                r.title = title.to_string();
                self.rename_from_title(meeting, &mut ev);
            }
            Command::ChangeSettings(r) => {
                let cache = (r.search_cache != self.settings.search_cache).then(|| r.search_cache.clone());
                self.settings = r;
                if let Some(cache) = cache {
                    self.use_search_cache(cache);
                }
            }
            Command::RequestPermission(a) => self.adapters.capture.request_permission(a),
            Command::InstallResources => {
                if self.installation.is_none() {
                    // Installation also reruns the checks.
                    if let Some(mut v) = self.verification.take() {
                        v.stop();
                    }
                    self.installation = Some(self.adapters.resources.install());
                    self.resources = self.adapters.resources.state();
                }
            }
            Command::CheckResources => {
                if self.installation.is_none() && self.verification.is_none() {
                    self.verification = Some(self.adapters.resources.check());
                    self.resources = self.adapters.resources.state();
                }
            }
            Command::Import(import) => {
                let id = self.next_id;
                let r = self.folder.import(&import, id, self.adapters.mp4_muxing.as_ref()).map_err(Rejection::Impossible)?;
                // Files copied by Greffier are not changes made outside the app.
                self.note_end(&r.folder_name.clone(), &Role::ALL);
                self.next_id += 1;
                // Most recent first.
                let storage = self.meetings.iter().position(|x| x.start <= r.start).unwrap_or(self.meetings.len());
                self.meetings.insert(storage, r);
                ev.push(Event::MeetingImported(id));
                self.persist(id, &mut ev);
            }
        }
        if let Some(id) = affected {
            self.persist(id, &mut ev);
        }
        self.indexer();
        Ok(ev)
    }

    /// Writes the meeting's files (sheet, transcriptions) and notes those that are present.
    fn persist(&mut self, id: MeetingId, ev: &mut Vec<Event>) {
        let Some(r) = self.meeting(id) else { return };
        let name = r.folder_name.clone();
        let result = self.folder.write_files(r);
        let files = self.folder.present_roles(&name);
        if let Ok(r) = self.meeting_mut(id) {
            r.files = files;
        }
        self.note_end(&name, result.as_deref().unwrap_or_default());
        if let Err(message) = result {
            ev.push(Event::WriteFailed { meeting: id, message });
        }
    }

    fn meeting_mut(&mut self, id: MeetingId) -> Result<&mut Meeting, Rejection> {
        self.meetings.iter_mut().find(|r| r.id == id).ok_or(Rejection::UnknownMeeting(id))
    }

    fn start_recording(&mut self) -> Result<(), Rejection> {
        if self.recording.is_some() {
            return impossible("un enregistrement est déjà en cours");
        }
        let sources = self.settings.sources;
        if sources.none_enabled() {
            return impossible("Aucune source à enregistrer : active l'écran, le son du Mac ou le micro dans les réglages.");
        }
        let permissions = self.adapters.capture.permissions();
        for a in sources.permissions() {
            match permissions.state(a) {
                PermissionState::Granted => {}
                PermissionState::ToRequest => {
                    self.adapters.capture.request_permission(a);
                    return impossible(format!("Greffier demande l'autorisation pour « {} » : accepte la demande de macOS, puis relance l'enregistrement.", a.name().to_lowercase()));
                }
                PermissionState::Denied => {
                    return impossible(format!("Greffier n'a pas l'autorisation pour « {} » : Réglages Système › Confidentialité et sécurité › {}, active Greffier.", a.name(), a.section()));
                }
            }
        }
        let start = self.adapters.clock.now();
        let name = self.folder.available_name(start, "Réunion");
        self.folder.create(&name).map_err(|e| Rejection::Impossible(format!("dossier de la réunion : {e}")))?;
        let request = CaptureRequest { file: self.folder.file(&name, Role::Recording), sources };
        self.adapters.capture.start(request).map_err(Rejection::Impossible)?;
        self.recording = Some(Recording { start, duration: 0.0, levels: Levels::default(), markers: vec![], speaker_count: None, folder_name: name, sources });
        self.write_recording_sheet();
        Ok(())
    }

    /// The sheet of the recording in progress, rewritten at each marker: if the app or the Mac
    /// stops dead, the meeting is reread as "recording interrupted" with its markers and its
    /// speaker count (the duration then comes from the recording's fragments already written).
    fn write_recording_sheet(&self) {
        let Some(e) = &self.recording else { return };
        let mut r = recorded_meeting(0, e, 0.0);
        r.status = Status::Recording;
        let _ = self.folder.write(&r);
    }

    fn stop_recording(&mut self, ev: &mut Vec<Event>) -> Result<(), Rejection> {
        let Some(e) = self.recording.take() else { return impossible("aucun enregistrement en cours") };
        let duration = match self.adapters.capture.stop() {
            Ok(duration) => duration,
            Err(message) => {
                self.keep_interrupted_recording(&e, None, message, ev);
                return Ok(());
            }
        };
        let id = self.next_id;
        self.next_id += 1;
        self.meetings.insert(0, recorded_meeting(id, &e, duration));
        ev.push(Event::MeetingRecorded(id));
        if duration < MIN_DURATION {
            // Not queued while the user decides: a false start is neither transcribed nor sent to
            // the agent before being kept (`Command::KeepShortRecording`).
            ev.push(Event::DeletionProposed(id));
        } else if self.settings.process_on_stop {
            self.enqueue(id, Stage::Transcription);
        }
        self.persist(id, ev);
        Ok(())
    }

    /// Keeps the meeting of a recording that did not end normally, as "recording interrupted",
    /// with what was captured. The duration is the one the capture returned, otherwise that of
    /// the fragments already written, otherwise the last known one. It is not queued: it is
    /// processed whenever the user wants.
    fn keep_interrupted_recording(&mut self, e: &Recording, duration: Option<f64>, message: String, ev: &mut Vec<Event>) {
        let file = self.folder.file(&e.folder_name, Role::Recording);
        let duration = duration.or_else(|| folder::mp4_duration(&file)).unwrap_or(e.duration);
        let id = self.next_id;
        self.next_id += 1;
        let mut r = recorded_meeting(id, e, duration);
        r.status = Status::RecordingInterrupted;
        self.meetings.insert(0, r);
        ev.push(Event::RecordingInterrupted { meeting: id, message });
        self.persist(id, ev);
    }

    fn enqueue(&mut self, id: MeetingId, stage: Stage) {
        if let Ok(r) = self.meeting_mut(id) {
            r.status = Status::Pending(stage);
            self.file.push_back(id);
        }
    }

    /// Stops the meeting's processing if it is running, and removes it from the queue.
    fn remove_from_queue(&mut self, id: MeetingId) {
        if let Some(mut t) = self.processing.take_if(|t| t.meeting == id) {
            t.job.stop();
        }
        self.file.retain(|x| *x != id);
    }

    // --- Time ----------------------------------------------------------------------------

    /// Moves `elapsed` seconds forward: the capture, the resource installation and the
    /// processing queue advance. Only one meeting is processed at a time; during a recording, no
    /// stage starts (the one running goes to the end).
    pub fn advance(&mut self, elapsed: f64) -> Vec<Event> {
        let mut ev = Vec::new();
        if let Some(e) = &mut self.recording {
            let state = self.adapters.capture.poll(elapsed);
            e.duration = state.duration;
            e.levels = state.levels;
            if let Some(message) = state.error
                && let Some(e) = self.recording.take()
            {
                // Finish the file with what was captured.
                let duration = self.adapters.capture.stop().ok();
                self.keep_interrupted_recording(&e, duration, message, &mut ev);
            }
        }
        if let Some(t) = &mut self.installation {
            match t.poll(elapsed) {
                TaskProgress::Running(_) => {}
                TaskProgress::Done(()) => {
                    self.installation = None;
                    ev.push(Event::ResourcesInstalled);
                }
                TaskProgress::Failed(m) => {
                    self.installation = None;
                    ev.push(Event::InstallationFailed(m));
                }
            }
        }
        if let Some(t) = &mut self.verification
            && !matches!(t.poll(elapsed), TaskProgress::Running(_))
        {
            self.verification = None;
        }
        self.resources = self.adapters.resources.state();
        if self.processing.is_none()
            && self.recording.is_none()
            && let Some(id) = self.file.pop_front()
            && let Some(Status::Pending(stage)) = self.meeting(id).map(|r| r.status.clone())
        {
            self.start_stage(id, stage, &mut ev);
        }
        self.advance_processing(elapsed, &mut ev);
        // A stage done, a recording stopped: meetings have changed.
        if !ev.is_empty() {
            self.indexer();
        }
        ev
    }

    fn start_stage(&mut self, id: MeetingId, stage: Stage, ev: &mut Vec<Event>) {
        match self.prepare(id, stage) {
            Ok(job) => {
                if let Ok(r) = self.meeting_mut(id) {
                    r.status = Status::Running { stage, progress: 0.0 };
                }
                self.processing = Some(Processing { meeting: id, job });
                self.persist(id, ev);
            }
            Err(message) => self.failure(id, stage, message, ev),
        }
    }

    fn prepare(&mut self, id: MeetingId, stage: Stage) -> Result<Job, String> {
        let r = self.meeting(id).ok_or("réunion inconnue")?.clone();
        let audio = self.folder.recording(&r.folder_name).unwrap_or_else(|| self.folder.file(&r.folder_name, Role::Recording));
        let a = &mut self.adapters;
        self.resources = a.resources.state();
        let missing: Vec<&str> = self.resources.iter().filter(|x| x.stages.contains(&stage) && x.state != ResourceState::Ready).map(|x| x.name.as_str()).collect();
        if !missing.is_empty() {
            return Err(format!("Ressources manquantes : {}.", missing.join(", ")));
        }
        Ok(match stage {
            Stage::Transcription => {
                let vocabulary = self.settings.lexicon.as_ref().and_then(|l| std::fs::read_to_string(l).ok()).map(|md| lexicon::vocabulary(&md)).unwrap_or_default();
                let words = a.transcriber.transcribe(TranscriptionRequest { audio: audio.clone(), duration: r.duration, vocabulary });
                let turns = a.diarizer.diarize(DiarizationRequest { audio, duration: r.duration, speaker_count: r.speaker_count });
                Job::Transcription { words: Part::Running(words), turns: Part::Running(turns) }
            }
            Stage::Review => {
                let lexicon = self.settings.lexicon.as_ref().and_then(|l| std::fs::read_to_string(l).ok()).unwrap_or_default();
                let request = ReviewRequest {
                    recording: audio,
                    meetings_folder: self.folder.root().to_path_buf(),
                    folder_name: r.folder_name.clone(),
                    title: r.title.clone(),
                    start: r.start,
                    speaker_count: r.speaker_count,
                    // A redone review starts again from the raw transcription.
                    utterances: for_agent(&r, if r.raw.is_empty() { &r.utterances } else { &r.raw }),
                    vocabulary: lexicon::vocabulary(&lexicon),
                    lexicon,
                };
                Job::Review(Part::Running(a.agent.review(request)))
            }
            Stage::Summary => {
                self.folder.create(&r.folder_name)?;
                let request = SummaryRequest {
                    meetings_folder: self.folder.root().to_path_buf(),
                    context_folders: self.settings.context_folders.clone(),
                    summary_file: self.folder.file(&r.folder_name, Role::Summary),
                    sheet: self.folder.file(&r.folder_name, Role::Sheet),
                    transcription: self.folder.file(&r.folder_name, Role::ReadableTranscription),
                    title: r.title.clone(),
                    start: r.start,
                    duration: r.duration,
                    utterances: for_agent(&r, &r.utterances),
                    speakers: r
                        .speakers
                        .iter()
                        .map(|l| AgentSpeaker {
                            label: l.label.clone(),
                            // A merged speaker's own name stays out of sight until split.
                            name: l.name.clone().filter(|_| l.merged_into.is_none()),
                            proposal: l.proposal.clone(),
                            justification: l.justification.clone(),
                            proposed_same_voice: l.suggested_merge.map(|j| r.speakers[j].label.clone()),
                            merged_into: l.merged_into.map(|j| r.speakers[r.displayed_speaker(j)].label.clone()),
                        })
                        .collect(),
                    markers: r.markers.clone(),
                    current_summary: (!r.summary.is_empty()).then(|| r.summary.clone()),
                };
                Job::Summary { result: Part::Running(a.agent.summarize(request)) }
            }
        })
    }

    fn advance_processing(&mut self, elapsed: f64, ev: &mut Vec<Event>) {
        let Some(t) = &mut self.processing else { return };
        let (id, stage) = (t.meeting, t.job.stage());
        let issue = |p: Result<f64, String>, on_done: bool| match p {
            Err(e) => TaskProgress::Failed(e),
            Ok(_) if on_done => TaskProgress::Done(()),
            Ok(x) => TaskProgress::Running(x),
        };
        let issue = match &mut t.job {
            // Both halves run at the same time: the stage advances as fast as the slower one.
            Job::Transcription { words, turns } => match (words.poll(elapsed), turns.poll(elapsed)) {
                (Ok(a), Ok(b)) => issue(Ok(a.min(b)), words.on_done() && turns.on_done()),
                (Err(e), _) | (_, Err(e)) => TaskProgress::Failed(e),
            },
            Job::Review(p) => {
                let a = p.poll(elapsed);
                issue(a, p.on_done())
            }
            Job::Summary { result, .. } => {
                let a = result.poll(elapsed);
                issue(a, result.on_done())
            }
        };
        match issue {
            TaskProgress::Running(p) => {
                if let Ok(r) = self.meeting_mut(id) {
                    r.status = Status::Running { stage, progress: p };
                }
            }
            TaskProgress::Failed(message) => {
                if let Some(mut t) = self.processing.take() {
                    t.job.stop();
                }
                self.failure(id, stage, message, ev);
            }
            TaskProgress::Done(()) => {
                let Some(t) = self.processing.take() else { return };
                let produced = match t.job {
                    Job::Transcription { words, turns } => StageOutput::Transcription(words.take().unwrap_or_default(), turns.take().unwrap_or_default()),
                    Job::Review(p) => StageOutput::Review(p.take().unwrap_or_default()),
                    Job::Summary { result } => match result.take() {
                        Some(r) => StageOutput::Summary(r),
                        None => return,
                    },
                };
                self.apply(id, stage, produced, ev);
            }
        }
    }

    /// Applies the output of a finished stage, then starts the next one (or makes it pending
    /// during a recording).
    fn apply(&mut self, id: MeetingId, stage: Stage, produced: StageOutput, ev: &mut Vec<Event>) {
        // The agent's session is kept even if its summary is rejected: it can be resumed.
        if let StageOutput::Summary(result) = &produced
            && let Ok(r) = self.meeting_mut(id)
        {
            r.sessions.summary = result.session.clone();
        }
        let name = self.meeting(id).map(|r| r.folder_name.clone()).unwrap_or_default();
        let contents = match &produced {
            StageOutput::Summary(..) => match self.folder.read_summary(&name) {
                Ok(md) => Some(md),
                Err(e) => return self.failure(id, stage, format!("L'agent n'a pas écrit le bilan ({e})."), ev),
            },
            _ => None,
        };
        if let Some(missing) = contents.as_deref().and_then(summary::missing_parts) {
            return self.failure(id, stage, missing, ev);
        }
        // The summary was just written by the agent for Greffier: it is not a change made
        // outside the app.
        if contents.is_some()
            && let Some(name) = self.meeting(id).map(|r| r.folder_name.clone())
        {
            self.note_end(&name, &[Role::Summary]);
        }
        let added = match &produced {
            StageOutput::Review(result) => self.enrich_lexicon(id, &result.lexicon_additions, ev),
            _ => Vec::new(),
        };
        let is_recording = self.recording.is_some();
        let Ok(r) = self.meeting_mut(id) else { return };
        match produced {
            StageOutput::Transcription(words, turns) => {
                let a = attribution::attribute(attribution::without_noises(words), turns);
                r.words = a.words;
                r.turns = a.turns;
                r.speakers = a.speakers;
                r.utterances = a.utterances;
                r.passages.clear();
                r.corrections = 0;
                r.raw.clear();
                r.review = None;
                r.manual_corrections.clear();
            }
            StageOutput::Review(result) => apply_review(r, result, added),
            StageOutput::Summary(result) => {
                r.set_summary(contents.unwrap_or_default());
                r.title = result.title;
                r.brief = result.brief;
                r.key_actions = result.actions.into_iter().take(3).collect();
                for p in result.speakers {
                    let index = |label: &str| r.speakers.iter().position(|l| l.label == label);
                    let Some(i) = index(&p.speaker) else { continue };
                    let same = p.same_voice_as.as_deref().and_then(index).filter(|j| r.displayed_speaker(*j) != r.displayed_speaker(i));
                    let l = &mut r.speakers[i];
                    // An empty name: the agent could not tell who it is, its justification says why.
                    l.proposal = Some(p.proposed_name).filter(|n| !n.trim().is_empty());
                    l.justification = p.justification;
                    l.suggested_merge = same;
                }
            }
        }
        r.completed_stage = Some(stage);
        ev.push(Event::StageDone { meeting: id, stage });
        match stage.next() {
            None => {
                r.status = Status::Done;
                r.is_new = true;
                ev.push(Event::SummaryReady { meeting: id, title: r.title.clone() });
                self.rename_from_title(id, ev);
            }
            Some(next) if is_recording => {
                r.status = Status::Pending(next);
                self.file.push_front(id);
                self.persist(id, ev);
            }
            Some(next) => self.start_stage(id, next, ev),
        }
    }

    /// Writes to the lexicon the errors learned by the review that it does not know yet;
    /// returns those that were written. Without a lexicon, nothing is written.
    fn enrich_lexicon(&mut self, id: MeetingId, additions: &[LexiconAddition], ev: &mut Vec<Event>) -> Vec<LexiconAddition> {
        let Some(path) = self.settings.lexicon.clone() else { return Vec::new() };
        if additions.is_empty() {
            return Vec::new();
        }
        let md = std::fs::read_to_string(&path).unwrap_or_default();
        let (md, added) = lexicon::add_errors(&md, additions);
        if added.is_empty() {
            return added;
        }
        match lexicon::write(&path, &md) {
            Ok(()) => added,
            Err(e) => {
                ev.push(Event::WriteFailed { meeting: id, message: format!("{} : {e}", path.display()) });
                Vec::new()
            }
        }
    }

    fn failure(&mut self, id: MeetingId, stage: Stage, message: String, ev: &mut Vec<Event>) {
        if let Ok(r) = self.meeting_mut(id) {
            r.status = Status::Failed { stage, message: message.clone() };
        }
        ev.push(Event::StageFailed { meeting: id, stage, message });
        self.persist(id, ev);
    }
}

/// The meeting of a recording: its folder, its start, its markers and its speaker count.
/// Duration and markers to the hundredth of a second, as in the sheet.
fn recorded_meeting(id: MeetingId, e: &Recording, duration: f64) -> Meeting {
    let hundredth = |t: f64| (t * 100.0).round() / 100.0;
    let mut r = Meeting::new(id, e.folder_name.clone(), e.start, hundredth(duration));
    r.speaker_count = e.speaker_count;
    r.markers = e.markers.iter().enumerate().map(|(i, t)| Marker { instant: hundredth(*t), label: format!("Repère {}", i + 1) }).collect();
    r
}

fn speaker_mut(r: &mut Meeting, i: usize) -> Result<&mut Speaker, Rejection> {
    r.speakers.get_mut(i).ok_or(Rejection::Impossible("locuteur inconnu".into()))
}

/// The transcription cannot be corrected while a stage is about to redo it: the transcription
/// starts from scratch, the review reopens every passage to check.
fn transcription_editable(r: &Meeting) -> Result<(), Rejection> {
    match r.status {
        Status::Running { stage, .. } | Status::Pending(stage) if stage <= Stage::Review => {
            impossible("la transcription est en cours de traitement : attends la fin de la relecture")
        }
        _ => Ok(()),
    }
}

/// Keeps the change to an utterance's text as a manual correction.
fn correct_utterance(r: &mut Meeting, utterance: usize, text: String) -> Result<(), Rejection> {
    let text = text.trim();
    if text.is_empty() {
        return impossible("une réplique ne peut pas être vide");
    }
    let Some(x) = r.utterances.get(utterance) else { return impossible("réplique inconnue") };
    let Some((start, end, after)) = word_difference(&x.text, text) else { return Ok(()) };
    // The utterance's passages to check are resolved in their tab.
    for p in x.passages.iter().filter_map(|&i| r.passages.get(i)) {
        let shown = p.text(&p.status);
        if let Some(a) = x.text.find(shown)
            && start < a + shown.len()
            && a < end
        {
            return impossible("ce passage est à vérifier : il se tranche dans l'onglet À vérifier");
        }
    }
    let before = x.text[start..end].to_string();
    r.utterances[utterance].text = text.to_string();
    r.manual_corrections.push(ManualCorrection { utterance, before, after });
    Ok(())
}

/// What changes from `before` to `after`, widened to whole words: the byte range of `before`
/// and the text replacing it. `None` if they are identical.
fn word_difference(before: &str, after: &str) -> Option<(usize, usize, String)> {
    if before == after {
        return None;
    }
    // The common start, then the common end without overlapping the start.
    let mut start = before.char_indices().zip(after.chars()).find(|((_, a), b)| a != b).map_or(before.len().min(after.len()), |((i, _), _)| i);
    while !before.is_char_boundary(start) || !after.is_char_boundary(start) {
        start -= 1;
    }
    let mut common = 0;
    for (a, b) in before[start..].chars().rev().zip(after[start..].chars().rev()) {
        if a != b {
            break;
        }
        common += a.len_utf8();
    }
    let (mut end_before, mut end_after) = (before.len() - common, after.len() - common);
    // To whole words: the start moves back and the end forward (within the common part) up to a blank.
    while let Some(c) = before[..start].chars().next_back().filter(|c| !c.is_whitespace()) {
        start -= c.len_utf8();
    }
    while let Some(c) = before[end_before..].chars().next().filter(|c| !c.is_whitespace()) {
        end_before += c.len_utf8();
        end_after += c.len_utf8();
    }
    Some((start, end_before, after[start..end_after].to_string()))
}

/// Resolves a passage to check: the utterance's text goes from what the passage shows in it
/// (the excerpt, the hypothesis or the correction) to what the resolution puts there.
fn resolve(r: &mut Meeting, passage: usize, resolution: Resolution) -> Result<(), Rejection> {
    let Some(p) = r.passages.get(passage) else { return impossible("passage inconnu") };
    let status = match resolution {
        Resolution::AcceptHypothesis if p.hypothesis.is_none() => return impossible("ce passage n'a pas d'hypothèse"),
        Resolution::AcceptHypothesis => PassageStatus::HypothesisAccepted,
        Resolution::KeepOriginal => PassageStatus::OriginalKept,
        Resolution::Correct(text) if text.trim().is_empty() => return impossible("la correction est vide"),
        Resolution::Correct(text) => PassageStatus::CorrectedManually(text.trim().to_string()),
        Resolution::Reopen => PassageStatus::Pending,
    };
    let (current, new) = (p.text(&p.status).to_string(), p.text(&status).to_string());
    let utterance = p.utterance;
    let Some(x) = r.utterances.get_mut(utterance) else { return impossible("réplique inconnue") };
    if current != new && !replace(&mut x.text, &current, &new) {
        return impossible(format!("« {current} » n'est plus dans la réplique : elle a été corrigée à la main"));
    }
    r.passages[passage].status = status;
    Ok(())
}

/// Replaces the first occurrence of `before` with `after`; returns `false` if `before` is empty
/// or absent.
fn replace(text: &mut String, before: &str, after: &str) -> bool {
    match text.find(before).filter(|_| !before.is_empty()) {
        Some(i) => {
            text.replace_range(i..i + before.len(), after);
            true
        }
        None => false,
    }
}

/// A hypothesis that is not one (empty, "?", "? (peut-être…)") is not kept.
fn hypothesis(h: Option<String>) -> Option<String> {
    h.map(|h| h.trim().to_string()).filter(|h| !h.is_empty() && !h.starts_with('?'))
}

/// Applies the review result, always from the raw transcription (a redone review does not
/// stack on the previous one). What does not apply (unknown utterance or speaker, missing
/// text) is discarded and does not go into the log. `added`: the errors actually written to
/// the lexicon.
fn apply_review(r: &mut Meeting, result: ReviewResult, added: Vec<LexiconAddition>) {
    if r.raw.is_empty() {
        r.raw = r.utterances.iter().map(|x| Utterance { passages: vec![], ..x.clone() }).collect();
    }
    r.utterances = r.raw.clone();
    let mut log = ReviewLog { lexicon_additions: added, ..Default::default() };
    for c in result.corrections {
        let Some(x) = r.utterances.get_mut(c.utterance) else { continue };
        if c.before != c.after && replace(&mut x.text, &c.before, &c.after) {
            log.corrections.push(c);
        }
    }
    for c in result.speaker_changes {
        let Some(i) = r.speakers.iter().position(|l| l.label == c.speaker) else { continue };
        let Some(x) = r.utterances.get(c.utterance) else { continue };
        if r.displayed_speaker(x.speaker) == r.displayed_speaker(i) {
            continue;
        }
        r.utterances[c.utterance].speaker = i;
        log.speaker_changes.push(c);
    }
    let mut to_check = Vec::new();
    for p in result.corrected_passages {
        let Some(x) = r.utterances.get_mut(p.utterance) else { continue };
        let after = p.after.trim();
        if after.is_empty() || after.starts_with('?') || after == p.excerpt.trim() {
            continue;
        }
        if replace(&mut x.text, &p.excerpt, after) {
            log.corrected_passages.push(CorrectedPassage { after: after.to_string(), ..p });
        } else {
            // The excerpt is no longer in the utterance (a sure correction touched it): to check.
            to_check.push(FlaggedPassage { utterance: p.utterance, excerpt: p.excerpt, hypothesis: Some(after.to_string()), relisten: p.relisten });
        }
    }
    r.corrections = (log.corrections.len() + log.corrected_passages.len()) as u32;
    r.passages.clear();
    for p in result.passages_to_check.into_iter().chain(to_check) {
        let Some(x) = r.utterances.get_mut(p.utterance) else { continue };
        // Each flagged passage counts, even several in one utterance; the same one twice
        // counts as one.
        if p.excerpt.trim().is_empty() || x.passages.iter().any(|&i| r.passages[i].excerpt == p.excerpt) {
            continue;
        }
        x.passages.push(r.passages.len());
        r.passages.push(PassageToCheck { utterance: p.utterance, excerpt: p.excerpt, hypothesis: hypothesis(p.hypothesis), relisten: p.relisten, status: PassageStatus::Pending });
    }
    // Manual corrections made on the previous review are reapplied if their text is still
    // there.
    let manual = std::mem::take(&mut r.manual_corrections);
    for c in manual {
        if let Some(x) = r.utterances.get_mut(c.utterance)
            && replace(&mut x.text, &c.before, &c.after)
        {
            r.manual_corrections.push(c);
        }
    }
    r.sessions.review = result.session;
    r.review = Some(log);
}

fn for_agent(r: &Meeting, utterances: &[Utterance]) -> Vec<AgentUtterance> {
    utterances
        .iter()
        .enumerate()
        .map(|(number, x)| {
            let l = &r.speakers[r.displayed_speaker(x.speaker)];
            AgentUtterance { number, speaker: l.label.clone(), name: l.name.clone(), start: x.start, end: x.end, text: x.text.clone() }
        })
        .collect()
}
