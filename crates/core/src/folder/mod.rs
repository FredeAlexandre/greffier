//! The meetings folder (ADR 0001): one subfolder per meeting, named
//! `YYYY-MM-DD HHhMM - Title`, whose files each repeat that name followed by their role.
//!
//! This module names the folders and files, and reads everything that exists, including
//! incomplete meetings (a recording alone, a recording and its transcription…).
//! File fingerprints (`watch`) tell what changed outside the app; `renaming` follows the
//! title. A deleted meeting goes whole to the Mac's trash (`Trash` adapter, see
//! `Greffier::delete`).

pub mod sheet;
pub(crate) mod import;
pub(crate) mod mp4;
mod renaming;
pub mod watch;
pub mod transcription;

pub use mp4::{duration as mp4_duration, leading_video_gap as mp4_leading_video_gap};

/// Whether a recording file has a picture to show: an MP4 or QuickTime file with a video
/// track. An audio file (`.wav`, `.mp3`…) or an MP4 with sound only (an `.m4a` turned into
/// `.mp4`) has none, and plays as audio. A file whose `moov` cannot be read is judged by its
/// extension.
pub fn has_picture(path: &Path) -> bool {
    mp4::has_video(path).unwrap_or_else(|| import::VIDEO_EXTENSIONS.contains(&import::extension(path).as_str()))
}

use std::path::{Path, PathBuf};

use crate::meeting::{Stage, MeetingId, Meeting, Status};
use sheet::Sheet;
use transcription::StructuredTranscription;
pub use import::{MEDIA_EXTENSIONS, Import};
use crate::time::DateTime;

/// The role of a file in a meeting's folder.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
pub enum Role {
    Sheet,
    Summary,
    ReadableTranscription,
    StructuredTranscription,
    Recording,
}

impl Role {
    pub const ALL: [Role; 5] = [Role::Sheet, Role::Summary, Role::ReadableTranscription, Role::StructuredTranscription, Role::Recording];

    pub fn suffix(self) -> &'static str {
        match self {
            Role::Sheet => "réunion.md",
            Role::Summary => "bilan.md",
            Role::ReadableTranscription => "transcription.md",
            Role::StructuredTranscription => "transcription.json",
            Role::Recording => "enregistrement.mp4",
        }
    }
}

const FORBIDDEN: [char; 9] = ['/', '\\', ':', '*', '?', '"', '<', '>', '|'];

/// `YYYY-MM-DD HHhMM - Title`, with the title's forbidden characters turned into dashes.
pub fn folder_name(start: DateTime, title: &str) -> String {
    let title: String = title.chars().map(|c| if FORBIDDEN.contains(&c) || c.is_control() { '-' } else { c }).collect();
    format!("{} - {}", start.timestamp(), title.trim())
}

/// Reads `YYYY-MM-DD HHhMM - Title`: the start and the title (without a collision's "(2)").
pub fn read_folder_name(name: &str) -> Option<(DateTime, String)> {
    let b = name.as_bytes();
    if name.get(16..19) != Some(" - ") {
        return None;
    }
    let n = |a: usize, z: usize| -> Option<u32> { name.get(a..z).filter(|x| x.bytes().all(|c| c.is_ascii_digit()))?.parse().ok() };
    if b[4] != b'-' || b[7] != b'-' || b[10] != b' ' || b[13] != b'h' {
        return None;
    }
    let start = DateTime::new(n(0, 4)? as i32, n(5, 7)? as u8, n(8, 10)? as u8, n(11, 13)? as u8, n(14, 16)? as u8);
    if !(1..=12).contains(&start.month) || !(1..=31).contains(&start.day) || start.hour > 23 || start.minute > 59 {
        return None;
    }
    let mut title = name[19..].trim();
    if let Some(without) = title.strip_suffix(')').and_then(|t| t.rsplit_once(" (")).filter(|(_, n)| !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit())) {
        title = without.0;
    }
    Some((start, title.to_string()))
}

#[derive(Clone, Debug)]
pub struct MeetingsFolder {
    root: PathBuf,
}

impl MeetingsFolder {
    pub fn new(root: impl Into<PathBuf>) -> MeetingsFolder {
        MeetingsFolder { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// A meeting's folder.
    pub fn folder(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }

    /// A meeting's file: `<name>/<name> - <role>`.
    pub fn file(&self, name: &str, role: Role) -> PathBuf {
        self.folder(name).join(format!("{name} - {}", role.suffix()))
    }

    /// Creates a meeting's folder if it does not exist.
    pub fn create(&self, name: &str) -> Result<(), String> {
        let d = self.folder(name);
        std::fs::create_dir_all(&d).map_err(|e| format!("{} : {e}", d.display()))
    }

    /// A meeting's summary, as it is on disk.
    pub fn read_summary(&self, name: &str) -> Result<String, String> {
        std::fs::read_to_string(self.file(name, Role::Summary)).map_err(|e| e.to_string())
    }

    /// The names of the meeting folders, most recent first. Anything that does not follow the
    /// naming rule is ignored.
    pub fn names(&self) -> Vec<String> {
        let Ok(entries) = std::fs::read_dir(&self.root) else { return vec![] };
        let mut names: Vec<(DateTime, String)> = entries
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .filter_map(|e| e.file_name().into_string().ok())
            .filter_map(|name| Some((read_folder_name(&name)?.0, name)))
            .collect();
        names.sort_by(|a, b| b.cmp(a));
        names.into_iter().map(|(_, name)| name).collect()
    }

    /// The files present in a meeting's folder.
    pub fn present_roles(&self, name: &str) -> Vec<Role> {
        Role::ALL.into_iter().filter(|r| if *r == Role::Recording { self.recording(name).is_some() } else { self.file(name, *r).is_file() }).collect()
    }

    /// A meeting's recording as it is on disk: `… - enregistrement.mp4`, or another imported
    /// media file that keeps its extension (`… - enregistrement.m4a`).
    pub fn recording(&self, name: &str) -> Option<PathBuf> {
        let mp4 = self.file(name, Role::Recording);
        if mp4.is_file() {
            return Some(mp4);
        }
        MEDIA_EXTENSIONS.iter().map(|e| self.recording_file(name, e)).find(|f| f.is_file())
    }

    /// `<name>/<name> - enregistrement.<extension>`.
    pub fn recording_file(&self, name: &str, extension: &str) -> PathBuf {
        self.folder(name).join(format!("{name} - enregistrement.{extension}"))
    }

    /// All the meetings in the folder, most recent first, numbered from `first_id`.
    pub fn read_all(&self, first_id: MeetingId) -> Vec<Meeting> {
        self.names().iter().zip(first_id..).filter_map(|(name, id)| self.read(name, id)).collect()
    }

    /// A folder's meeting, with what it contains. An unreadable file counts as absent: the
    /// meeting shows with the rest.
    ///
    /// - Without a sheet: start and title come from the folder name; done if there is a
    ///   summary, to process otherwise.
    /// - The last completed stage is inferred from the files (structured transcription, review
    ///   in its log, summary), capped by the sheet's and by the pending or failed stage.
    /// - A recording in progress in the sheet is read back as interrupted (the app stopped).
    /// - The duration: the sheet's, otherwise the recording's, otherwise the end of the last
    ///   utterance.
    pub fn read(&self, name: &str, id: MeetingId) -> Option<Meeting> {
        let (start, title) = read_folder_name(name)?;
        let files = self.present_roles(name);
        let read = |role: Role| if files.contains(&role) { std::fs::read_to_string(self.file(name, role)).ok() } else { None };
        let mut r = Meeting::new(id, name.to_string(), start, 0.0);
        r.title = title;
        let mut done = None;
        if let Some(t) = read(Role::StructuredTranscription).and_then(|j| StructuredTranscription::read(&j).ok()) {
            t.apply(&mut r);
            done = Some(if r.review.is_some() { Stage::Review } else { Stage::Transcription });
        }
        if let Some(md) = read(Role::Summary) {
            r.brief = crate::summary::brief(&md);
            r.set_summary(md);
            done = Some(Stage::Summary);
        }
        match read(Role::Sheet).and_then(|md| Sheet::read(&md).ok()) {
            Some(f) => {
                f.apply(&mut r);
                done = done.min(f.completed_stage);
                if let Status::Pending(e) | Status::Running { stage: e, .. } | Status::Failed { stage: e, .. } = r.status {
                    done = done.min(e.previous());
                }
                r.status = match r.status {
                    Status::Recording => Status::RecordingInterrupted,
                    Status::Done if done != Some(Stage::Summary) => Status::ToProcess,
                    s => s,
                };
            }
            None => r.status = if done == Some(Stage::Summary) { Status::Done } else { Status::ToProcess },
        }
        r.completed_stage = done;
        if r.duration <= 0.0 {
            let video = self.recording(name).and_then(|f| mp4::duration(&f));
            r.duration = video.or_else(|| r.utterances.iter().map(|x| x.end).reduce(f64::max)).unwrap_or(0.0);
        }
        r.files = files;
        Some(r)
    }

    /// Writes the sheet and, if there is a transcription, the structured and readable
    /// transcriptions. The summary (written by the agent) and the recording are not included.
    /// Each file is written alongside then renamed, so a reader never sees it half-written.
    pub fn write(&self, r: &Meeting) -> Result<(), String> {
        self.write_files(r).map(|_| ())
    }

    /// Like [`MeetingsFolder::write`], and returns the roles of the files written.
    pub fn write_files(&self, r: &Meeting) -> Result<Vec<Role>, String> {
        let name = &r.folder_name;
        let folder = self.folder(name);
        std::fs::create_dir_all(&folder).map_err(|e| format!("{} : {e}", folder.display()))?;
        let recording = self.recording(name).map_or_else(|| Role::Recording.suffix().to_string(), |f| format!("enregistrement.{}", import::extension(&f)));
        write_atomically(&self.file(name, Role::Sheet), &Sheet::from_meeting(r).markdown(name, &recording))?;
        if r.completed_stage.is_some() || !r.utterances.is_empty() {
            write_atomically(&self.file(name, Role::StructuredTranscription), &StructuredTranscription::from_meeting(r).json())?;
            write_atomically(&self.file(name, Role::ReadableTranscription), &transcription::readable_transcription(r))?;
            return Ok(vec![Role::Sheet, Role::StructuredTranscription, Role::ReadableTranscription]);
        }
        Ok(vec![Role::Sheet])
    }

    /// A free folder name for a new meeting ("(2)", "(3)"… if needed).
    pub fn available_name(&self, start: DateTime, title: &str) -> String {
        let base = folder_name(start, title);
        let mut name = base.clone();
        let mut n = 2;
        while self.folder(&name).exists() {
            name = format!("{base} ({n})");
            n += 1;
        }
        name
    }
}

fn write_atomically(path: &Path, content: &str) -> Result<(), String> {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("fichier");
    let temporary = path.with_file_name(format!(".{name}.greffier"));
    std::fs::write(&temporary, content).and_then(|()| std::fs::rename(&temporary, path)).map_err(|e| {
        let _ = std::fs::remove_file(&temporary);
        format!("{} : {e}", path.display())
    })
}
