//! A meeting and everything derived from it: speakers, utterances, passages to check, markers,
//! summary, and its status in processing. Only the core modifies them; the interface reads them.

use crate::adapters::{LexiconAddition, SpeakerChange, ConfidentCorrection, CorrectedPassage};
use crate::folder::Role;
use crate::time::DateTime;

/// A meeting's id in the core, stable while the app runs.
pub type MeetingId = u32;

/// A link in the processing chain, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Stage {
    Transcription,
    Review,
    Summary,
}

impl Stage {
    pub const ALL: [Stage; 3] = [Stage::Transcription, Stage::Review, Stage::Summary];

    pub fn next(self) -> Option<Stage> {
        match self {
            Stage::Transcription => Some(Stage::Review),
            Stage::Review => Some(Stage::Summary),
            Stage::Summary => None,
        }
    }

    pub fn previous(self) -> Option<Stage> {
        match self {
            Stage::Transcription => None,
            Stage::Review => Some(Stage::Transcription),
            Stage::Summary => Some(Stage::Review),
        }
    }

    /// The stage's relative share of a processing run's duration, to estimate the time remaining.
    pub fn weight(self) -> f64 {
        match self {
            Stage::Transcription => 6.0,
            Stage::Review => 4.5,
            Stage::Summary => 4.5,
        }
    }
}

/// Where a meeting stands.
#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    Recording,
    RecordingInterrupted,
    /// Recorded (or imported), processing has not started.
    ToProcess,
    /// In the queue, waiting for its turn; processing will start at this stage.
    Pending(Stage),
    /// The stage is running; `progress` goes from 0 to 1.
    Running { stage: Stage, progress: f64 },
    Failed { stage: Stage, message: String },
    Done,
}

impl Status {
    pub fn is_processing(&self) -> bool {
        matches!(self, Status::Running { .. })
    }
    pub fn in_queue(&self) -> bool {
        matches!(self, Status::Running { .. } | Status::Pending(_))
    }
    pub fn progress(&self) -> f64 {
        match self {
            Status::Running { progress, .. } => *progress,
            Status::Done => 1.0,
            _ => 0.0,
        }
    }
}

/// A transcribed word, timestamped, with its confidence (0 to 1).
#[derive(Clone, Debug, PartialEq)]
pub struct Word {
    pub start: f64,
    pub end: f64,
    pub text: String,
    pub confidence: f32,
    /// The index of its speaker in [`Meeting::speakers`], once attributed (the transcriber
    /// returns words without a speaker).
    pub speaker: Option<usize>,
}

/// A speaker turn returned by diarization: one speaker at a time.
#[derive(Clone, Debug, PartialEq)]
pub struct SpeakerTurn {
    pub start: f64,
    pub end: f64,
    /// « SPEAKER_00 »
    pub speaker: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Speaker {
    /// "SPEAKER_00", as diarization named it.
    pub label: String,
    /// The name confirmed or given by the user.
    pub name: Option<String>,
    /// The name proposed by the agent, and why.
    pub proposal: Option<String>,
    pub justification: String,
    /// The hue of its color in the interface (oklch).
    pub tint: f32,
    /// Merged into another speaker (index), which replaces it everywhere.
    pub merged_into: Option<usize>,
    /// The agent thinks it is the same voice as another speaker (index).
    pub suggested_merge: Option<usize>,
}

impl Speaker {
    /// The n-th speaker's label: "SPEAKER_00", "SPEAKER_01"…
    pub fn numbered_label(n: usize) -> String {
        format!("SPEAKER_{n:02}")
    }

    pub fn new(label: impl Into<String>, tint: f32) -> Speaker {
        Speaker { label: label.into(), name: None, proposal: None, justification: String::new(), tint, merged_into: None, suggested_merge: None }
    }

    /// The name to display: confirmed, otherwise proposed ("Khalid ?"), otherwise the label.
    pub fn display_name(&self) -> String {
        match (&self.name, &self.proposal) {
            (Some(n), _) => n.clone(),
            (None, Some(p)) => format!("{p} ?"),
            (None, None) => self.label.clone(),
        }
    }
}

/// Hues given to detected speakers, in order.
pub const HUES: [f32; 8] = [160.0, 255.0, 320.0, 200.0, 35.0, 80.0, 120.0, 280.0];

#[derive(Clone, Debug, PartialEq)]
pub struct Utterance {
    /// Index in `Meeting::speakers` (before merging: see `Meeting::displayed_speaker`).
    pub speaker: usize,
    pub start: f64,
    pub end: f64,
    pub text: String,
    /// This utterance's passages to check (indexes in `Meeting::passages`, in order).
    pub passages: Vec<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PassageStatus {
    Pending,
    HypothesisAccepted,
    OriginalKept,
    CorrectedManually(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct PassageToCheck {
    pub utterance: usize,
    pub excerpt: String,
    pub hypothesis: Option<String>,
    /// What the relisten heard.
    pub relisten: String,
    pub status: PassageStatus,
}

/// The relisten's margin around the utterance, and the maximum length of an excerpt (Whisper
/// hears 30 s at once), in seconds.
const MARGIN: f64 = 1.5;
const MAX_EXCERPT: f64 = 28.0;

/// The excerpt to relisten to (or to listen to in order to decide) for a passage of an
/// utterance: ±1.5 s around the utterance; beyond 28 s, 26 s centered on where the passage sits
/// in the utterance.
pub fn excerpt_to_relisten(start: f64, end: f64, text: &str, passage: &str) -> (f64, f64) {
    let (mut from, mut a) = (start - MARGIN, end + MARGIN);
    if a - from > MAX_EXCERPT {
        let position = text.find(passage).map_or(0, |i| text[..i].chars().count());
        let center = start + position as f64 / text.chars().count().max(1) as f64 * (end - start);
        (from, a) = (center - 12.0, center + 14.0);
    }
    (from.max(0.0), a)
}

impl PassageToCheck {
    /// What the passage puts into its utterance with this status: the excerpt while pending or
    /// kept, otherwise the accepted hypothesis or the correction.
    pub fn text<'a>(&'a self, status: &'a PassageStatus) -> &'a str {
        match status {
            PassageStatus::HypothesisAccepted => self.hypothesis.as_deref().unwrap_or(&self.excerpt),
            PassageStatus::CorrectedManually(t) => t,
            PassageStatus::Pending | PassageStatus::OriginalKept => &self.excerpt,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Marker {
    pub instant: f64,
    pub label: String,
}

/// A topic of the summary: a chapter of the timeline.
#[derive(Clone, Debug, PartialEq)]
pub struct Topic {
    pub start: f64,
    pub title: String,
}

/// A timestamped decision of the summary.
#[derive(Clone, Debug, PartialEq)]
pub struct Decision {
    pub instant: f64,
    pub text: String,
}

/// What the review changed, kept in the structured transcription.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReviewLog {
    /// The confident corrections applied.
    pub corrections: Vec<ConfidentCorrection>,
    pub speaker_changes: Vec<SpeakerChange>,
    /// The doubtful passages corrected after the relisten.
    pub corrected_passages: Vec<CorrectedPassage>,
    /// The errors written into the lexicon.
    pub lexicon_additions: Vec<LexiconAddition>,
}

/// A correction made by hand in the transcription: `before` becomes `after`.
#[derive(Clone, Debug, PartialEq)]
pub struct ManualCorrection {
    pub utterance: usize,
    pub before: String,
    pub after: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sessions {
    pub review: Option<String>,
    pub summary: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Meeting {
    pub id: MeetingId,
    /// The name of the meeting's folder in the meetings folder.
    pub folder_name: String,
    pub start: DateTime,
    pub duration: f64,
    pub title: String,
    pub status: Status,
    /// The last stage completed: processing resumes at the next one.
    pub completed_stage: Option<Stage>,
    pub speaker_count: Option<u8>,
    pub markers: Vec<Marker>,
    pub words: Vec<Word>,
    pub turns: Vec<SpeakerTurn>,
    pub speakers: Vec<Speaker>,
    pub utterances: Vec<Utterance>,
    pub passages: Vec<PassageToCheck>,
    /// Confident corrections applied by the review.
    pub corrections: u32,
    /// The utterances from before the review (empty until it is done).
    pub raw: Vec<Utterance>,
    /// The review log, once it is done.
    pub review: Option<ReviewLog>,
    pub manual_corrections: Vec<ManualCorrection>,
    /// The summary in Markdown, as the agent wrote it (ticked checkboxes included).
    pub summary: String,
    /// Taken from the summary: the timeline's chapters and decisions.
    pub topics: Vec<Topic>,
    pub decisions: Vec<Decision>,
    pub brief: String,
    pub key_actions: Vec<String>,
    pub sessions: Sessions,
    /// The summary is ready and the meeting has not been opened yet.
    pub is_new: bool,
    /// The files present in its folder, at the core's last pass.
    pub files: Vec<Role>,
}

impl Meeting {
    /// An empty meeting, to process.
    pub fn new(id: MeetingId, folder_name: String, start: DateTime, duration: f64) -> Meeting {
        Meeting {
            id,
            folder_name,
            start,
            duration,
            title: "Réunion".into(),
            status: Status::ToProcess,
            completed_stage: None,
            speaker_count: None,
            markers: vec![],
            words: vec![],
            turns: vec![],
            speakers: vec![],
            utterances: vec![],
            passages: vec![],
            corrections: 0,
            raw: vec![],
            review: None,
            manual_corrections: vec![],
            summary: String::new(),
            topics: vec![],
            decisions: vec![],
            brief: String::new(),
            key_actions: vec![],
            sessions: Sessions::default(),
            is_new: false,
            files: vec![],
        }
    }

    /// Replaces the summary and derives the topics and decisions from it.
    pub(crate) fn set_summary(&mut self, md: String) {
        self.topics = crate::summary::topics(&md);
        self.decisions = crate::summary::decisions(&md);
        self.summary = md;
    }

    /// Builds an already processed meeting (sample data, fixtures).
    pub fn with_summary(mut self, md: impl Into<String>) -> Meeting {
        self.set_summary(md.into());
        self
    }

    /// Whether `stage` can be redone ([`crate::Command::Redo`]): the meeting is neither in the
    /// queue nor recording, and the previous stages are done.
    pub fn can_redo(&self, stage: Stage) -> bool {
        !self.status.in_queue() && self.status != Status::Recording && stage.previous().is_none_or(|p| self.completed_stage >= Some(p))
    }

    /// The start and end of the excerpt to listen to in order to decide this passage: the same
    /// as the review's relisten.
    pub fn excerpt_to_listen(&self, passage: usize) -> Option<(f64, f64)> {
        let p = self.passages.get(passage)?;
        let x = self.utterances.get(p.utterance)?;
        Some(excerpt_to_relisten(x.start, x.end, &x.text, p.text(&p.status)))
    }

    pub fn pending_passages(&self) -> usize {
        self.passages.iter().filter(|p| p.status == PassageStatus::Pending).count()
    }

    /// The displayed speaker for a raw index, following merges.
    pub fn displayed_speaker(&self, mut i: usize) -> usize {
        while let Some(j) = self.speakers[i].merged_into {
            i = j;
        }
        i
    }

    /// The name given to the displayed speaker for a raw index (merges followed).
    pub fn speaker_name(&self, i: usize) -> Option<&str> {
        self.speakers[self.displayed_speaker(i)].name.as_deref()
    }

    /// After reading back names and merges: a merged speaker has no name of its own (the files
    /// give it the name of the voice that replaces it).
    pub(crate) fn forget_merged_names(&mut self) {
        for l in &mut self.speakers {
            if l.merged_into.is_some() {
                l.name = None;
            }
        }
    }

    /// Merges speaker `i` into `j`: the one `i` displays is from now on replaced everywhere by
    /// the one `j` displays (merges already made follow). Returns `false`, changing nothing, if it
    /// is already the same voice or if an index is unknown: a merge never makes a loop.
    pub fn merge(&mut self, i: usize, j: usize) -> bool {
        if i >= self.speakers.len() || j >= self.speakers.len() {
            return false;
        }
        let (i, j) = (self.displayed_speaker(i), self.displayed_speaker(j));
        if i == j {
            return false;
        }
        self.speakers[i].merged_into = Some(j);
        true
    }

    /// Reads back a written merge (sheet, structured transcription) as is: `i` points to `j`,
    /// unless `i` is already merged or it would make a loop.
    pub(crate) fn restore_merge(&mut self, i: usize, j: usize) {
        if i < self.speakers.len() && j < self.speakers.len() && self.speakers[i].merged_into.is_none() && self.displayed_speaker(j) != i {
            self.speakers[i].merged_into = Some(j);
        }
    }

    /// The speakers not merged into another.
    pub fn active_speakers(&self) -> impl Iterator<Item = usize> + '_ {
        (0..self.speakers.len()).filter(|&i| self.speakers[i].merged_into.is_none())
    }

    pub fn utterance_at(&self, t: f64) -> Option<usize> {
        let i = self.utterances.partition_point(|u| u.start <= t);
        if i == 0 { None } else { Some(i - 1) }
    }

    pub fn topic_at(&self, t: f64) -> Option<usize> {
        let i = self.topics.partition_point(|x| x.start <= t);
        if i == 0 { None } else { Some(i - 1) }
    }

    /// A speaker's share of speaking time (merges included), from 0 to 1.
    pub fn speaking_time(&self, speaker: usize) -> f64 {
        let total: f64 = self.utterances.iter().map(|u| u.end - u.start).sum();
        let own: f64 = self.utterances.iter().filter(|u| self.displayed_speaker(u.speaker) == speaker).map(|u| u.end - u.start).sum();
        if total > 0.0 { own / total } else { 0.0 }
    }

    /// Estimated processing time remaining, in seconds, for a meeting in the queue. About 90 s
    /// per 10 min of meeting, plus a fixed 4 min, split according to the stages' weights.
    pub fn estimated_time_remaining(&self) -> Option<f64> {
        let (from_meeting, progress) = match &self.status {
            Status::Running { stage, progress } => (*stage, *progress),
            Status::Pending(stage) => (*stage, 0.0),
            _ => return None,
        };
        let total: f64 = Stage::ALL.iter().map(|e| e.weight()).sum();
        let rest: f64 = Stage::ALL.iter().filter(|e| **e >= from_meeting).map(|e| e.weight()).sum::<f64>() - progress * from_meeting.weight();
        Some(rest / total * (self.duration / 600.0 * 90.0 + 240.0))
    }
}
