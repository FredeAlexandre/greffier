//! Le dossier de réunions (ADR 0001) : un sous-dossier par réunion, nommé
//! `AAAA-MM-JJ HHhMM - Titre`, dont chaque fichier reprend le nom suivi de son rôle.
//!
//! Ce module nomme les dossiers et les fichiers, et lit tout ce qui existe, y compris les
//! réunions incomplètes (un enregistrement seul, un enregistrement et sa transcription…).
//! Les empreintes des fichiers (`surveillance`) disent ce qui a changé hors de l'app ; le
//! `renommage` suit le titre. Une réunion supprimée part entière à la corbeille du Mac
//! (adaptateur `Corbeille`, voir `Greffier::supprimer`).

pub mod sheet;
pub(crate) mod import;
mod mp4;
mod renaming;
pub mod watch;
pub mod transcription;

pub use mp4::duration as mp4_duration;

use std::path::{Path, PathBuf};

use crate::meeting::{Stage, MeetingId, Meeting, Status};
use sheet::Sheet;
use transcription::StructuredTranscription;
pub use import::{MEDIA_EXTENSIONS, Import};
use crate::time::DateTime;

/// Le rôle d'un fichier dans le dossier d'une réunion.
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

/// `AAAA-MM-JJ HHhMM - Titre`, les caractères interdits du titre devenus des tirets.
pub fn folder_name(start: DateTime, title: &str) -> String {
    let title: String = title.chars().map(|c| if FORBIDDEN.contains(&c) || c.is_control() { '-' } else { c }).collect();
    format!("{} - {}", start.timestamp(), title.trim())
}

/// Lit `AAAA-MM-JJ HHhMM - Titre` : le début et le titre (sans le « (2) » d'une collision).
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

    /// Le dossier d'une réunion.
    pub fn folder(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }

    /// Un fichier d'une réunion : `<nom>/<nom> - <rôle>`.
    pub fn file(&self, name: &str, role: Role) -> PathBuf {
        self.folder(name).join(format!("{name} - {}", role.suffix()))
    }

    /// Crée le dossier d'une réunion s'il n'existe pas.
    pub fn create(&self, name: &str) -> Result<(), String> {
        let d = self.folder(name);
        std::fs::create_dir_all(&d).map_err(|e| format!("{} : {e}", d.display()))
    }

    /// Le bilan d'une réunion, tel qu'il est sur le disque.
    pub fn read_summary(&self, name: &str) -> Result<String, String> {
        std::fs::read_to_string(self.file(name, Role::Summary)).map_err(|e| e.to_string())
    }

    /// Les noms des dossiers de réunion, les plus récents d'abord. Ce qui ne suit pas la règle
    /// de nommage est ignoré.
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

    /// Les fichiers présents dans le dossier d'une réunion.
    pub fn present_roles(&self, name: &str) -> Vec<Role> {
        Role::ALL.into_iter().filter(|r| if *r == Role::Recording { self.recording(name).is_some() } else { self.file(name, *r).is_file() }).collect()
    }

    /// L'enregistrement d'une réunion tel qu'il est sur le disque : `… - enregistrement.mp4`,
    /// ou un autre média importé qui garde son extension (`… - enregistrement.m4a`).
    pub fn recording(&self, name: &str) -> Option<PathBuf> {
        let mp4 = self.file(name, Role::Recording);
        if mp4.is_file() {
            return Some(mp4);
        }
        MEDIA_EXTENSIONS.iter().map(|e| self.recording_file(name, e)).find(|f| f.is_file())
    }

    /// `<nom>/<nom> - enregistrement.<extension>`.
    pub fn recording_file(&self, name: &str, extension: &str) -> PathBuf {
        self.folder(name).join(format!("{name} - enregistrement.{extension}"))
    }

    /// Toutes les réunions du dossier, les plus récentes d'abord, numérotées à partir de `premier_id`.
    pub fn read_all(&self, first_id: MeetingId) -> Vec<Meeting> {
        self.names().iter().zip(first_id..).filter_map(|(name, id)| self.read(name, id)).collect()
    }

    /// La réunion d'un dossier, avec ce qu'il contient. Un fichier illisible compte comme
    /// absent : la réunion s'affiche avec le reste.
    ///
    /// - Sans fiche : début et titre viennent du nom du dossier ; terminée s'il y a un bilan,
    ///   à traiter sinon.
    /// - La dernière étape faite se déduit des fichiers (transcription structurée, relecture
    ///   dans son journal, bilan), bornée par celle de la fiche et par l'étape en attente ou en
    ///   échec.
    /// - Un enregistrement en cours dans la fiche est relu interrompu (l'app s'est arrêtée).
    /// - La durée : celle de la fiche, sinon celle de l'enregistrement, sinon la fin de la
    ///   dernière réplique.
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

    /// Écrit la fiche et, s'il y a une transcription, la transcription structurée et la
    /// transcription lisible. Le bilan (écrit par l'agent) et l'enregistrement n'y sont pas.
    /// Chaque fichier est écrit à côté puis renommé, pour qu'un lecteur ne le voie jamais à moitié.
    pub fn write(&self, r: &Meeting) -> Result<(), String> {
        self.write_files(r).map(|_| ())
    }

    /// Comme [`DossierDeReunions::ecrire`], et rend les rôles des fichiers écrits.
    pub fn write_files(&self, r: &Meeting) -> Result<Vec<Role>, String> {
        let name = &r.folder_name;
        let folder = self.folder(name);
        std::fs::create_dir_all(&folder).map_err(|e| format!("{} : {e}", folder.display()))?;
        write_atomically(&self.file(name, Role::Sheet), &Sheet::from_meeting(r).markdown(name))?;
        if r.completed_stage.is_some() || !r.utterances.is_empty() {
            write_atomically(&self.file(name, Role::StructuredTranscription), &StructuredTranscription::from_meeting(r).json())?;
            write_atomically(&self.file(name, Role::ReadableTranscription), &transcription::readable_transcription(r))?;
            return Ok(vec![Role::Sheet, Role::StructuredTranscription, Role::ReadableTranscription]);
        }
        Ok(vec![Role::Sheet])
    }

    /// Un nom de dossier libre pour une nouvelle réunion (« (2) », « (3) »… si besoin).
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
