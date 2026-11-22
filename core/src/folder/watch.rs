//! Les empreintes des fichiers d'une réunion : de quoi voir qu'un fichier a changé depuis le
//! dernier passage du cœur sans le relire (date de modification, taille, inode).
//!
//! Le cœur retient l'empreinte de chaque dossier de réunion. Quand il écrit lui-même un fichier,
//! il en note aussitôt la nouvelle empreinte : ses propres écritures ne passent jamais pour des
//! modifications faites hors de l'app.

use std::collections::{BTreeMap, HashMap};
use std::io::{Seek, SeekFrom, Write};
use std::time::SystemTime;

use super::{MeetingsFolder, Role};

/// Ce qui change quand un fichier est réécrit : date de modification, taille, et inode (une
/// écriture atomique remplace le fichier).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Fingerprint {
    modified: Option<SystemTime>,
    size: u64,
    inode: u64,
}

/// Les empreintes des fichiers présents dans le dossier d'une réunion.
pub type Fingerprints = BTreeMap<Role, Fingerprint>;

impl MeetingsFolder {
    /// Les empreintes des fichiers de la réunion `nom` qui existent.
    pub fn fingerprints(&self, name: &str) -> Fingerprints {
        Role::ALL
            .into_iter()
            .filter_map(|role| {
                // L'enregistrement peut être un média importé qui garde son extension (`.m4a`…).
                let path = if role == Role::Recording { self.recording(name)? } else { self.file(name, role) };
                let m = std::fs::metadata(path).ok().filter(|m| m.is_file())?;
                Some((role, Fingerprint { modified: m.modified().ok(), size: m.len(), inode: inode(&m) }))
            })
            .collect()
    }

    /// Les empreintes de tous les dossiers de réunion, par nom.
    pub fn all_fingerprints(&self) -> HashMap<String, Fingerprints> {
        self.names()
            .into_iter()
            .map(|name| {
                let e = self.fingerprints(&name);
                (name, e)
            })
            .collect()
    }

    /// Coche ou décoche la `index`-ième case du bilan dans son fichier, en ne réécrivant que le
    /// caractère entre crochets : le reste du fichier (fins de ligne, mise en forme) ne bouge pas.
    /// Rend le bilan modifié, ou `None` si la réunion n'a pas de fichier de bilan.
    pub fn tick_in_summary(&self, name: &str, index: usize, done: bool) -> Result<Option<String>, String> {
        let path = self.file(name, Role::Summary);
        if !path.is_file() {
            return Ok(None);
        }
        let error = |e: std::io::Error| format!("{} : {e}", path.display());
        let mut md = std::fs::read_to_string(&path).map_err(error)?;
        let Some(position) = crate::summary::checkbox_position(&md, index) else { return Err("action inconnue".into()) };
        if crate::summary::tick(&mut md, index, done) {
            let mut f = std::fs::OpenOptions::new().write(true).open(&path).map_err(error)?;
            f.seek(SeekFrom::Start(position as u64)).map_err(error)?;
            f.write_all(&md.as_bytes()[position..position + 1]).map_err(error)?;
        }
        Ok(Some(md))
    }
}

#[cfg(unix)]
fn inode(m: &std::fs::Metadata) -> u64 {
    std::os::unix::fs::MetadataExt::ino(m)
}

#[cfg(not(unix))]
fn inode(_: &std::fs::Metadata) -> u64 {
    0
}
