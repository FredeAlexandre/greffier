//! Le dossier de réunions (ADR 0001) : un sous-dossier par réunion, nommé
//! `AAAA-MM-JJ HHhMM - Titre`, dont chaque fichier reprend le nom suivi de son rôle.
//!
//! Ce module nomme les dossiers et les fichiers, et lit tout ce qui existe, y compris les
//! réunions incomplètes (un enregistrement seul, un enregistrement et sa transcription…).
//! Les empreintes des fichiers (`surveillance`) disent ce qui a changé hors de l'app. Le
//! renommage et la corbeille viennent avec les tickets suivants.

pub mod fiche;
mod mp4;
pub mod surveillance;
pub mod transcription;

use std::path::{Path, PathBuf};

use crate::reunion::{Etape, IdReunion, Reunion, Statut};
use fiche::Fiche;
use transcription::TranscriptionStructuree;
use crate::temps::DateHeure;

/// Le rôle d'un fichier dans le dossier d'une réunion.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Role {
    Fiche,
    Bilan,
    TranscriptionLisible,
    TranscriptionStructuree,
    Enregistrement,
}

impl Role {
    pub const TOUS: [Role; 5] = [Role::Fiche, Role::Bilan, Role::TranscriptionLisible, Role::TranscriptionStructuree, Role::Enregistrement];

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

/// Lit `AAAA-MM-JJ HHhMM - Titre` : le début et le titre (sans le « (2) » d'une collision).
pub fn lire_nom_de_dossier(nom: &str) -> Option<(DateHeure, String)> {
    let b = nom.as_bytes();
    if nom.get(16..19) != Some(" - ") {
        return None;
    }
    let n = |a: usize, z: usize| -> Option<u32> { nom.get(a..z).filter(|x| x.bytes().all(|c| c.is_ascii_digit()))?.parse().ok() };
    if b[4] != b'-' || b[7] != b'-' || b[10] != b' ' || b[13] != b'h' {
        return None;
    }
    let debut = DateHeure::new(n(0, 4)? as i32, n(5, 7)? as u8, n(8, 10)? as u8, n(11, 13)? as u8, n(14, 16)? as u8);
    if !(1..=12).contains(&debut.mois) || !(1..=31).contains(&debut.jour) || debut.heure > 23 || debut.minute > 59 {
        return None;
    }
    let mut titre = nom[19..].trim();
    if let Some(sans) = titre.strip_suffix(')').and_then(|t| t.rsplit_once(" (")).filter(|(_, n)| !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit())) {
        titre = sans.0;
    }
    Some((debut, titre.to_string()))
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

    /// Les noms des dossiers de réunion, les plus récents d'abord. Ce qui ne suit pas la règle
    /// de nommage est ignoré.
    pub fn noms(&self) -> Vec<String> {
        let Ok(entrees) = std::fs::read_dir(&self.racine) else { return vec![] };
        let mut noms: Vec<(DateHeure, String)> = entrees
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .filter_map(|e| e.file_name().into_string().ok())
            .filter_map(|nom| Some((lire_nom_de_dossier(&nom)?.0, nom)))
            .collect();
        noms.sort_by(|a, b| b.cmp(a));
        noms.into_iter().map(|(_, nom)| nom).collect()
    }

    /// Les fichiers présents dans le dossier d'une réunion.
    pub fn roles_presents(&self, nom: &str) -> Vec<Role> {
        Role::TOUS.into_iter().filter(|r| self.fichier(nom, *r).is_file()).collect()
    }

    /// Toutes les réunions du dossier, les plus récentes d'abord, numérotées à partir de `premier_id`.
    pub fn lire_tout(&self, premier_id: IdReunion) -> Vec<Reunion> {
        self.noms().iter().zip(premier_id..).filter_map(|(nom, id)| self.lire(nom, id)).collect()
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
    pub fn lire(&self, nom: &str, id: IdReunion) -> Option<Reunion> {
        let (debut, titre) = lire_nom_de_dossier(nom)?;
        let fichiers = self.roles_presents(nom);
        let lire = |role: Role| if fichiers.contains(&role) { std::fs::read_to_string(self.fichier(nom, role)).ok() } else { None };
        let mut r = Reunion::new(id, nom.to_string(), debut, 0.0);
        r.titre = titre;
        let mut faite = None;
        if let Some(t) = lire(Role::TranscriptionStructuree).and_then(|j| TranscriptionStructuree::lire(&j).ok()) {
            t.appliquer(&mut r);
            faite = Some(if r.relecture.is_some() { Etape::Relecture } else { Etape::Transcription });
        }
        if let Some(md) = lire(Role::Bilan) {
            r.resume = crate::bilan::en_bref(&md);
            r.poser_bilan(md);
            faite = Some(Etape::Bilan);
        }
        match lire(Role::Fiche).and_then(|md| Fiche::lire(&md).ok()) {
            Some(f) => {
                f.appliquer(&mut r);
                faite = faite.min(f.etape_faite);
                if let Statut::EnAttente(e) | Statut::EnCours { etape: e, .. } | Statut::Echec { etape: e, .. } = r.statut {
                    faite = faite.min(e.precedente());
                }
                r.statut = match r.statut {
                    Statut::EnregistrementEnCours => Statut::EnregistrementInterrompu,
                    Statut::Terminee if faite != Some(Etape::Bilan) => Statut::ATraiter,
                    s => s,
                };
            }
            None => r.statut = if faite == Some(Etape::Bilan) { Statut::Terminee } else { Statut::ATraiter },
        }
        r.etape_faite = faite;
        if r.duree <= 0.0 {
            let video = fichiers.contains(&Role::Enregistrement).then(|| mp4::duree(&self.fichier(nom, Role::Enregistrement))).flatten();
            r.duree = video.or_else(|| r.repliques.iter().map(|x| x.fin).reduce(f64::max)).unwrap_or(0.0);
        }
        r.fichiers = fichiers;
        Some(r)
    }

    /// Écrit la fiche et, s'il y a une transcription, la transcription structurée et la
    /// transcription lisible. Le bilan (écrit par l'agent) et l'enregistrement n'y sont pas.
    /// Chaque fichier est écrit à côté puis renommé, pour qu'un lecteur ne le voie jamais à moitié.
    pub fn ecrire(&self, r: &Reunion) -> Result<(), String> {
        self.ecrire_les_fichiers(r).map(|_| ())
    }

    /// Comme [`DossierDeReunions::ecrire`], et rend les rôles des fichiers écrits.
    pub fn ecrire_les_fichiers(&self, r: &Reunion) -> Result<Vec<Role>, String> {
        let nom = &r.nom_du_dossier;
        let dossier = self.dossier(nom);
        std::fs::create_dir_all(&dossier).map_err(|e| format!("{} : {e}", dossier.display()))?;
        ecrire_atomiquement(&self.fichier(nom, Role::Fiche), &Fiche::depuis(r).markdown(nom))?;
        if r.etape_faite.is_some() || !r.repliques.is_empty() {
            ecrire_atomiquement(&self.fichier(nom, Role::TranscriptionStructuree), &TranscriptionStructuree::depuis(r).json())?;
            ecrire_atomiquement(&self.fichier(nom, Role::TranscriptionLisible), &transcription::transcription_lisible(r))?;
            return Ok(vec![Role::Fiche, Role::TranscriptionStructuree, Role::TranscriptionLisible]);
        }
        Ok(vec![Role::Fiche])
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

fn ecrire_atomiquement(chemin: &Path, contenu: &str) -> Result<(), String> {
    let nom = chemin.file_name().and_then(|n| n.to_str()).unwrap_or("fichier");
    let provisoire = chemin.with_file_name(format!(".{nom}.greffier"));
    std::fs::write(&provisoire, contenu).and_then(|()| std::fs::rename(&provisoire, chemin)).map_err(|e| {
        let _ = std::fs::remove_file(&provisoire);
        format!("{} : {e}", chemin.display())
    })
}
