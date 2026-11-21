//! Les empreintes des fichiers d'une réunion : de quoi voir qu'un fichier a changé depuis le
//! dernier passage du cœur sans le relire (date de modification, taille, inode).
//!
//! Le cœur retient l'empreinte de chaque dossier de réunion. Quand il écrit lui-même un fichier,
//! il en note aussitôt la nouvelle empreinte : ses propres écritures ne passent jamais pour des
//! modifications faites hors de l'app.

use std::collections::{BTreeMap, HashMap};
use std::io::{Seek, SeekFrom, Write};
use std::time::SystemTime;

use super::{DossierDeReunions, Role};

/// Ce qui change quand un fichier est réécrit : date de modification, taille, et inode (une
/// écriture atomique remplace le fichier).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Empreinte {
    modifie: Option<SystemTime>,
    taille: u64,
    inode: u64,
}

/// Les empreintes des fichiers présents dans le dossier d'une réunion.
pub type Empreintes = BTreeMap<Role, Empreinte>;

impl DossierDeReunions {
    /// Les empreintes des fichiers de la réunion `nom` qui existent.
    pub fn empreintes(&self, nom: &str) -> Empreintes {
        Role::TOUS
            .into_iter()
            .filter_map(|role| {
                let m = std::fs::metadata(self.fichier(nom, role)).ok().filter(|m| m.is_file())?;
                Some((role, Empreinte { modifie: m.modified().ok(), taille: m.len(), inode: inode(&m) }))
            })
            .collect()
    }

    /// Les empreintes de tous les dossiers de réunion, par nom.
    pub fn toutes_les_empreintes(&self) -> HashMap<String, Empreintes> {
        self.noms()
            .into_iter()
            .map(|nom| {
                let e = self.empreintes(&nom);
                (nom, e)
            })
            .collect()
    }

    /// Coche ou décoche la `index`-ième case du bilan dans son fichier, en ne réécrivant que le
    /// caractère entre crochets : le reste du fichier (fins de ligne, mise en forme) ne bouge pas.
    /// Rend le bilan modifié, ou `None` si la réunion n'a pas de fichier de bilan.
    pub fn cocher_dans_le_bilan(&self, nom: &str, index: usize, faite: bool) -> Result<Option<String>, String> {
        let chemin = self.fichier(nom, Role::Bilan);
        if !chemin.is_file() {
            return Ok(None);
        }
        let erreur = |e: std::io::Error| format!("{} : {e}", chemin.display());
        let mut md = std::fs::read_to_string(&chemin).map_err(erreur)?;
        let Some(position) = crate::bilan::position_de_la_case(&md, index) else { return Err("action inconnue".into()) };
        if crate::bilan::cocher(&mut md, index, faite) {
            let mut f = std::fs::OpenOptions::new().write(true).open(&chemin).map_err(erreur)?;
            f.seek(SeekFrom::Start(position as u64)).map_err(erreur)?;
            f.write_all(&md.as_bytes()[position..position + 1]).map_err(erreur)?;
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
