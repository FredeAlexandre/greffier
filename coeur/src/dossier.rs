//! Le dossier de réunions (ADR 0001) : un sous-dossier par réunion, nommé
//! `AAAA-MM-JJ HHhMM - Titre`, dont chaque fichier reprend le nom suivi de son rôle.
//!
//! Ce module ne fait encore que nommer : la lecture, l'écriture de la fiche et des
//! transcriptions, le renommage et la surveillance viennent avec les tickets suivants.

use std::path::{Path, PathBuf};

use crate::temps::DateHeure;

/// Le rôle d'un fichier dans le dossier d'une réunion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Fiche,
    Bilan,
    TranscriptionLisible,
    TranscriptionStructuree,
    Enregistrement,
}

impl Role {
    pub fn suffixe(self) -> &'static str {
        match self {
            Role::Fiche => "réunion.md",
            Role::Bilan => "bilan.md",
            Role::TranscriptionLisible => "transcription.md",
            Role::TranscriptionStructuree => "transcription.json",
            Role::Enregistrement => "enregistrement.mp4",
        }
    }
}

const INTERDITS: [char; 9] = ['/', '\\', ':', '*', '?', '"', '<', '>', '|'];

/// `AAAA-MM-JJ HHhMM - Titre`, les caractères interdits du titre devenus des tirets.
pub fn nom_de_dossier(debut: DateHeure, titre: &str) -> String {
    let titre: String = titre.chars().map(|c| if INTERDITS.contains(&c) || c.is_control() { '-' } else { c }).collect();
    format!("{} - {}", debut.horodatage(), titre.trim())
}

#[derive(Clone, Debug)]
pub struct DossierDeReunions {
    racine: PathBuf,
}

impl DossierDeReunions {
    pub fn new(racine: impl Into<PathBuf>) -> DossierDeReunions {
        DossierDeReunions { racine: racine.into() }
    }

    pub fn racine(&self) -> &Path {
        &self.racine
    }

    /// Le dossier d'une réunion.
    pub fn dossier(&self, nom: &str) -> PathBuf {
        self.racine.join(nom)
    }

    /// Un fichier d'une réunion : `<nom>/<nom> - <rôle>`.
    pub fn fichier(&self, nom: &str, role: Role) -> PathBuf {
        self.dossier(nom).join(format!("{nom} - {}", role.suffixe()))
    }

    /// Un nom de dossier libre pour une nouvelle réunion (« (2) », « (3) »… si besoin).
    pub fn nom_libre(&self, debut: DateHeure, titre: &str) -> String {
        let base = nom_de_dossier(debut, titre);
        let mut nom = base.clone();
        let mut n = 2;
        while self.dossier(&nom).exists() {
            nom = format!("{base} ({n})");
            n += 1;
        }
        nom
    }
}
