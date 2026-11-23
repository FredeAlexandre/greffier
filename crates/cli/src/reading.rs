//! The meetings as the command line reads them: from the meetings folder through the core,
//! each with its identifier and, once the process is reachable, what it knows live.

use std::path::Path;

use greffier_core::identifier::Identifiers;
use greffier_core::{Meeting, MeetingsFolder};
use serde::Serialize;

use crate::{Context, Failure, Live, LiveSource};

/// A meeting with its identifier and its live data, if any (its status is then the live one).
pub(crate) struct Entry {
    pub identifier: String,
    pub meeting: Meeting,
    pub live: Option<Live>,
}

/// The meetings folder as the reading commands see it: its meetings, their identifiers, and
/// the live data given.
pub(crate) struct Reading<'a> {
    pub folder: MeetingsFolder,
    names: Vec<String>,
    identifiers: Identifiers,
    live: Option<&'a dyn LiveSource>,
}

impl<'a> Reading<'a> {
    /// The context's meetings folder, which must exist.
    pub fn open(cx: &'a Context) -> Result<Reading<'a>, Failure> {
        let Some(root) = &cx.meetings_folder else {
            return Err(Failure::Failed(NO_MEETINGS_FOLDER.into()));
        };
        if !root.is_dir() {
            return Err(Failure::Failed(format!("meetings folder not found: {}", root.display())));
        }
        let folder = MeetingsFolder::new(root);
        let names = folder.names();
        let identifiers = Identifiers::new(&names);
        Ok(Reading { folder, names, identifiers, live: cx.live.as_deref() })
    }

    fn entry(&self, index: usize, name: &str) -> Option<Entry> {
        let mut meeting = self.folder.read(name, index as u32)?;
        let live = self.live.and_then(|l| l.live(name));
        if let Some(l) = &live {
            meeting.status = l.status.clone();
        }
        Some(Entry { identifier: self.identifiers.of(name)?.to_string(), meeting, live })
    }

    /// A meeting's identifier, from its folder name.
    pub fn identifier(&self, folder_name: &str) -> Option<&str> {
        self.identifiers.of(folder_name)
    }

    /// All the meetings, most recent first.
    pub fn all(&self) -> Vec<Entry> {
        self.names.iter().enumerate().filter_map(|(i, n)| self.entry(i, n)).collect()
    }

    /// The meeting `query` names (see [`Identifiers::resolve`]). The candidates of an ambiguous
    /// query are listed with their sheet's titles, as `list` shows them.
    pub fn find(&self, query: &str) -> Result<Entry, Failure> {
        let entry = |name: &str| self.entry(self.names.iter().position(|n| n == name).unwrap_or(0), name);
        let query = in_meetings_folder(query, self.folder.root()).map_err(Failure::Failed)?;
        let name = self.identifiers.resolve(query).map_err(|mut e| {
            for (identifier, title) in &mut e.candidates {
                if let Some(c) = self.identifiers.iter().find(|(i, _)| i == identifier).and_then(|(_, n)| entry(n)) {
                    *title = c.meeting.title;
                }
            }
            Failure::Failed(e.to_string())
        })?;
        entry(name).ok_or_else(|| Failure::Failed(format!("unreadable meeting: {name}")))
    }
}

/// What the user typed to name a meeting, for [`Identifiers::resolve`]: as is, or for a path
/// (with a `/`), the folder's name if the folder is in the meetings folder `root` (compared
/// canonical, so that any path to it does); a path elsewhere names no meeting, even one with
/// the same folder name. A relative path is read from the current folder.
pub(crate) fn in_meetings_folder<'q>(query: &'q str, root: &Path) -> Result<&'q str, String> {
    let q = query.trim();
    if !q.contains('/') {
        return Ok(q);
    }
    let path = Path::new(q);
    let canonical = |p: &Path| p.canonicalize().ok();
    let parent = path.parent().map(|p| if p.as_os_str().is_empty() { Path::new(".") } else { p });
    let name = path.file_name().and_then(|n| n.to_str());
    match (name, parent.and_then(canonical)) {
        (Some(name), Some(parent)) if Some(&parent) == canonical(root).as_ref() => Ok(name),
        _ => Err(format!("meeting not found: \"{q}\" (not in the meetings folder {})", canonical(root).unwrap_or_else(|| root.to_path_buf()).display())),
    }
}

/// What the command line says without a meetings folder.
pub(crate) const NO_MEETINGS_FOLDER: &str = "no meetings folder: open Greffier once to choose one, or set GREFFIER_FOLDER.";

/// A meeting in JSON, in the glossary's English terms.
#[derive(Serialize)]
pub(crate) struct MeetingJson {
    pub identifier: String,
    /// The meeting's folder, absolute.
    pub folder: String,
    pub title: String,
    /// `2026-11-20`
    pub date: String,
    /// `09:39`
    pub start: String,
    /// In seconds.
    pub duration: u64,
    /// `2:34:22`
    pub duration_text: String,
    /// [`crate::text::status_name`].
    pub status: &'static str,
    /// The stage queued, running or failed ([`crate::text::stage_name`]).
    pub stage: Option<&'static str>,
    pub completed_stage: Option<&'static str>,
    pub error: Option<String>,
    /// The passages to check still pending.
    pub to_check: usize,
}

impl MeetingJson {
    pub fn new(e: &Entry, folder: &MeetingsFolder) -> MeetingJson {
        use crate::text::{stage_name, status_name};
        use greffier_core::{Status, playback_timestamp};
        let m = &e.meeting;
        let (stage, error) = match &m.status {
            Status::Pending(s) | Status::Running { stage: s, .. } => (Some(stage_name(*s)), None),
            Status::Failed { stage, message } => (Some(stage_name(*stage)), Some(message.clone())),
            _ => (None, None),
        };
        MeetingJson {
            identifier: e.identifier.clone(),
            folder: folder.folder(&m.folder_name).to_string_lossy().into_owned(),
            title: m.title.clone(),
            date: format!("{:04}-{:02}-{:02}", m.start.year, m.start.month, m.start.day),
            start: format!("{:02}:{:02}", m.start.hour, m.start.minute),
            duration: m.duration.max(0.0).round() as u64,
            duration_text: playback_timestamp(m.duration),
            status: status_name(&m.status),
            stage,
            completed_stage: m.completed_stage.map(stage_name),
            error,
            to_check: m.pending_passages(),
        }
    }
}
