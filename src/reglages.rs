//! Ce que l'interface sait des réglages retenus : le dossier de réunions (que
//! `GREFFIER_DOSSIER=/chemin` remplace le temps d'un lancement, sans toucher aux réglages : essais
//! sur une copie, réunions d'exemple), les vrais adaptateurs et l'heure du Mac. Le fichier des
//! réglages et le lexique sont lus et écrits par le cœur ([`greffier_coeur::reglages`],
//! [`greffier_coeur::Greffier::lexique`]).

use std::path::{Path, PathBuf};

use greffier_coeur::reglages::{self, ReglagesRetenus};

pub use greffier_coeur::reglages::{dossier_de_l_app, dossier_propose};

/// Les réglages retenus dans le dossier de l'app.
pub fn retenus() -> ReglagesRetenus {
    ReglagesRetenus::lire(&dossier_de_l_app())
}

/// Le dossier de réunions choisi, s'il l'a été.
pub fn dossier_de_reunions() -> Option<PathBuf> {
    match std::env::var_os("GREFFIER_DOSSIER") {
        Some(d) => Some(PathBuf::from(d)),
        None => retenus().dossier_de_reunions,
    }
}

/// Retient le dossier de réunions (et le crée s'il n'existe pas) ; avec `GREFFIER_DOSSIER`, le
/// crée seulement.
pub fn choisir_dossier_de_reunions(dossier: &Path) -> Result<(), String> {
    if std::env::var_os("GREFFIER_DOSSIER").is_some() {
        return reglages::creer_dossier_de_reunions(dossier);
    }
    reglages::choisir_dossier_de_reunions(&dossier_de_l_app(), dossier)
}

/// Retient les réglages d'enregistrement du cœur et le raccourci global (vide : aucun).
pub fn retenir_enregistrement(r: &greffier_coeur::Reglages, raccourci: &str) -> Result<(), String> {
    let mut retenus = retenus();
    retenus.depuis(r);
    retenus.raccourci = raccourci.to_string();
    retenus.retenir(&dossier_de_l_app())
}

/// Un chemin à afficher : `~` pour le dossier personnel.
pub fn afficher(chemin: &Path) -> String {
    match chemin.strip_prefix(reglages::maison()) {
        Ok(reste) => format!("~/{}", reste.display()),
        Err(_) => chemin.display().to_string(),
    }
}

/// L'heure locale du Mac, pour dater les enregistrements et savoir quel jour on est.
pub struct HorlogeDuMac;

impl greffier_coeur::adaptateurs::Horloge for HorlogeDuMac {
    fn maintenant(&self) -> greffier_coeur::DateHeure {
        use chrono::{Datelike, Timelike};
        let t = chrono::Local::now();
        greffier_coeur::DateHeure::new(t.year(), t.month() as u8, t.day() as u8, t.hour() as u8, t.minute() as u8)
    }
    fn en_heure_locale(&self, instant: std::time::SystemTime) -> greffier_coeur::DateHeure {
        use chrono::{Datelike, Timelike};
        let t: chrono::DateTime<chrono::Local> = instant.into();
        greffier_coeur::DateHeure::new(t.year(), t.month() as u8, t.day() as u8, t.hour() as u8, t.minute() as u8)
    }
}

/// Les adaptateurs de l'app : la vraie transcription (whisper.cpp, modèles du dossier de l'app),
/// la vraie séparation des locuteurs (pyannote dans le Python privé), la relecture et le bilan
/// par la CLI `claude` (réécoute par whisper.cpp), la vraie capture (ScreenCaptureKit), l'heure
/// du Mac, les vraies ressources et la corbeille du Mac.
pub fn adaptateurs() -> greffier_coeur::adaptateurs::Adaptateurs {
    use greffier_coeur::adaptateurs::Adaptateurs;
    use greffier_outils::claude::{AgentClaude, ClaudeDuMac};
    use greffier_coeur::ressources::{Catalogue, RessourcesDeLApp};
    use std::sync::Arc;
    let app = dossier_de_l_app();
    let ressources = RessourcesDeLApp::new(app.clone(), Catalogue::de_greffier(), greffier_outils::mac::OutilsDuMac::new());
    Adaptateurs {
        horloge: Box::new(HorlogeDuMac),
        ressources: Box::new(ressources),
        transcripteur: Box::new(greffier_moteurs::TranscripteurWhisper::de_l_app(&app)),
        separateur: Box::new(greffier_moteurs::SeparateurPyannote::de_l_app(&app)),
        agent: Box::new(AgentClaude::new(Arc::new(ClaudeDuMac::default()), Arc::new(greffier_moteurs::ReecouteWhisper::de_l_app(&app)))),
        capture: Box::new(greffier_capture::CaptureDuMac::new()),
        corbeille: Box::new(greffier_capture::CorbeilleDuMac),
        mise_en_mp4: Box::new(greffier_capture::MiseEnMp4DuMac),
    }
}

/// Le cache de recherche, dans le dossier de l'app.
pub fn cache_de_recherche() -> PathBuf {
    reglages::cache_de_recherche(&dossier_de_l_app())
}

/// Les personnes du lexique, proposées pour nommer les locuteurs ; sans fenêtre, celles des
/// données fictives.
pub fn personnes(core: &greffier_coeur::Greffier) -> Vec<String> {
    if rune::is_headless() {
        return greffier_coeur::fictif::PERSONNES.map(String::from).to_vec();
    }
    core.personnes()
}

/// Le chemin du lexique du dossier de l'app (le lexique livré y est écrit s'il manque).
pub fn chemin_du_lexique() -> Option<PathBuf> {
    greffier_coeur::lexique::installer(&dossier_de_l_app()).map_err(|e| eprintln!("greffier : lexique : {e}")).ok()
}
