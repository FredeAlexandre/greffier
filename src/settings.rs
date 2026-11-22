//! Ce que l'interface sait des réglages retenus : le dossier de réunions (que
//! `GREFFIER_DOSSIER=/chemin` remplace le temps d'un lancement, sans toucher aux réglages : essais
//! sur une copie, réunions d'exemple), les vrais adaptateurs et l'heure du Mac. Le fichier des
//! réglages et le lexique sont lus et écrits par le cœur ([`greffier_coeur::reglages`],
//! [`greffier_coeur::Greffier::lexique`]).

use std::path::{Path, PathBuf};

use greffier_core::settings::{self, PersistedSettings};

pub use greffier_core::settings::{app_folder, suggested_folder};

/// Les réglages retenus dans le dossier de l'app.
pub fn remembered() -> PersistedSettings {
    PersistedSettings::read(&app_folder())
}

/// Le dossier de réunions choisi, s'il l'a été.
pub fn meetings_folder() -> Option<PathBuf> {
    match std::env::var_os("GREFFIER_DOSSIER") {
        Some(d) => Some(PathBuf::from(d)),
        None => remembered().meetings_folder,
    }
}

/// Retient le dossier de réunions (et le crée s'il n'existe pas) ; avec `GREFFIER_DOSSIER`, le
/// crée seulement.
pub fn choose_meetings_folder(folder: &Path) -> Result<(), String> {
    if std::env::var_os("GREFFIER_DOSSIER").is_some() {
        return settings::create_meetings_folder(folder);
    }
    settings::choose_meetings_folder(&app_folder(), folder)
}

/// Retient les réglages d'enregistrement du cœur et le raccourci global (vide : aucun).
pub fn remember_recording_settings(r: &greffier_core::Settings, shortcut: &str) -> Result<(), String> {
    let mut remembered = remembered();
    remembered.from_meeting(r);
    remembered.shortcut = shortcut.to_string();
    remembered.remember(&app_folder())
}

/// Un chemin à afficher : `~` pour le dossier personnel.
pub fn display(path: &Path) -> String {
    match path.strip_prefix(settings::home()) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

/// L'heure locale du Mac, pour dater les enregistrements et savoir quel jour on est.
pub struct MacClock;

impl greffier_core::adapters::Clock for MacClock {
    fn now(&self) -> greffier_core::DateTime {
        use chrono::{Datelike, Timelike};
        let t = chrono::Local::now();
        greffier_core::DateTime::new(t.year(), t.month() as u8, t.day() as u8, t.hour() as u8, t.minute() as u8)
    }
    fn to_local_time(&self, instant: std::time::SystemTime) -> greffier_core::DateTime {
        use chrono::{Datelike, Timelike};
        let t: chrono::DateTime<chrono::Local> = instant.into();
        greffier_core::DateTime::new(t.year(), t.month() as u8, t.day() as u8, t.hour() as u8, t.minute() as u8)
    }
}

/// Les adaptateurs de l'app : la vraie transcription (whisper.cpp, modèles du dossier de l'app),
/// la vraie séparation des locuteurs (pyannote dans le Python privé), la relecture et le bilan
/// par la CLI `claude` (réécoute par whisper.cpp), la vraie capture (ScreenCaptureKit), l'heure
/// du Mac, les vraies ressources et la corbeille du Mac.
pub fn adapters() -> greffier_core::adapters::Adapters {
    use greffier_core::adapters::Adapters;
    use greffier_tools::claude::{AgentClaude, MacClaude};
    use greffier_core::resources::{Catalogue, AppResources};
    use std::sync::Arc;
    let app = app_folder();
    let resources = AppResources::new(app.clone(), Catalogue::for_greffier(), greffier_tools::mac::MacTools::new());
    Adapters {
        clock: Box::new(MacClock),
        resources: Box::new(resources),
        transcriber: Box::new(greffier_engines::WhisperTranscriber::from_app(&app)),
        diarizer: Box::new(greffier_engines::PyannoteDiarizer::from_app(&app)),
        agent: Box::new(AgentClaude::new(Arc::new(MacClaude::default()), Arc::new(greffier_engines::WhisperRelisten::from_app(&app)))),
        capture: Box::new(greffier_capture::MacCapture::new()),
        trash: Box::new(greffier_capture::MacTrash),
        mp4_muxing: Box::new(greffier_capture::MacMp4Muxer),
    }
}

/// Le cache de recherche, dans le dossier de l'app.
pub fn search_cache() -> PathBuf {
    settings::search_cache(&app_folder())
}

/// Les personnes du lexique, proposées pour nommer les locuteurs ; sans fenêtre, celles des
/// données fictives.
pub fn people(core: &greffier_core::Greffier) -> Vec<String> {
    if rune::is_headless() {
        return greffier_core::demo::PEOPLE.map(String::from).to_vec();
    }
    core.people()
}

/// Le chemin du lexique du dossier de l'app (le lexique livré y est écrit s'il manque).
pub fn lexicon_path() -> Option<PathBuf> {
    greffier_core::lexicon::install(&app_folder()).map_err(|e| eprintln!("greffier : lexique : {e}")).ok()
}
