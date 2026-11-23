//! The meetings as the command line reads them: from the meetings folder through the core,
//! each with its identifier and, once the process is reachable, what it knows live.

use greffier_core::identifier::Identifiers;
use greffier_core::{Meeting, MeetingsFolder};
use serde::Serialize;

use crate::{Context, Failure, Live, LiveSource};

/// A meeting with its identifier and its live data, if any (its status is then the live one).
pub(crate) struct Entry {
    pub id: String,
    pub meeting: Meeting,
    pub live: Option<Live>,
}

pub(crate) struct Library<'a> {
    pub folder: MeetingsFolder,
    names: Vec<String>,
    ids: Identifiers,
    live: Option<&'a dyn LiveSource>,
}

impl<'a> Library<'a> {
    /// The context's meetings folder, which must exist.
    pub fn open(cx: &'a Context) -> Result<Library<'a>, Failure> {
        let Some(root) = &cx.meetings_folder else {
            return Err(Failure::Failed("aucun dossier de réunions : ouvrez Greffier une fois pour le choisir, ou définissez GREFFIER_FOLDER.".into()));
        };
        if !root.is_dir() {
            return Err(Failure::Failed(format!("dossier de réunions introuvable : {}", root.display())));
        }
        let folder = MeetingsFolder::new(root);
        let names = folder.names();
        let ids = Identifiers::new(&names);
        Ok(Library { folder, names, ids, live: cx.live.as_deref() })
    }

    fn entry(&self, index: usize, name: &str) -> Option<Entry> {
        let mut meeting = self.folder.read(name, index as u32)?;
        let live = self.live.and_then(|l| l.live(name));
        if let Some(l) = &live {
            meeting.status = l.status.clone();
        }
        Some(Entry { id: self.ids.of(name)?.to_string(), meeting, live })
    }

    /// All the meetings, most recent first.
    pub fn all(&self) -> Vec<Entry> {
        self.names.iter().enumerate().filter_map(|(i, n)| self.entry(i, n)).collect()
    }

    /// The meeting `query` names (see [`Identifiers::resolve`]).
    pub fn find(&self, query: &str) -> Result<Entry, Failure> {
        let name = self.ids.resolve(query).map_err(|e| Failure::Failed(e.to_string()))?;
        let index = self.names.iter().position(|n| n == name).unwrap_or(0);
        self.entry(index, name).ok_or_else(|| Failure::Failed(format!("réunion illisible : {name}")))
    }
}

/// A meeting in JSON, in the sheet's vocabulary.
#[derive(Serialize)]
pub(crate) struct MeetingJson {
    pub id: String,
    pub dossier: String,
    pub titre: String,
    /// `2026-11-20`
    pub date: String,
    /// `09:39`
    pub debut: String,
    /// In seconds.
    pub duree: u64,
    /// `2:34:22`
    pub duree_texte: String,
    pub statut: &'static str,
    /// The stage pending, running or failed.
    pub etape: Option<&'static str>,
    pub etape_faite: Option<&'static str>,
    pub erreur: Option<String>,
    /// The passages to check still pending.
    pub a_verifier: usize,
}

impl MeetingJson {
    pub fn new(e: &Entry, folder: &MeetingsFolder) -> MeetingJson {
        use greffier_core::folder::sheet::{stage_name, status_name};
        use greffier_core::{Status, playback_timestamp};
        let m = &e.meeting;
        let (etape, erreur) = match &m.status {
            Status::Pending(s) | Status::Running { stage: s, .. } => (Some(stage_name(*s)), None),
            Status::Failed { stage, message } => (Some(stage_name(*stage)), Some(message.clone())),
            _ => (None, None),
        };
        MeetingJson {
            id: e.id.clone(),
            dossier: folder.folder(&m.folder_name).to_string_lossy().into_owned(),
            titre: m.title.clone(),
            date: format!("{:04}-{:02}-{:02}", m.start.year, m.start.month, m.start.day),
            debut: format!("{:02}:{:02}", m.start.hour, m.start.minute),
            duree: m.duration.max(0.0).round() as u64,
            duree_texte: playback_timestamp(m.duration),
            statut: status_name(&m.status),
            etape,
            etape_faite: m.completed_stage.map(stage_name),
            erreur,
            a_verifier: m.pending_passages(),
        }
    }
}
