//! What the UI knows of the remembered settings: the meetings folder (which
//! `GREFFIER_FOLDER=/path` replaces for a single launch, without touching the settings: trials
//! on a copy, sample meetings), the real adapters and the Mac's clock. The settings file and the
//! lexicon are read and written by the core ([`greffier_core::settings`],
//! [`greffier_core::Greffier::lexicon`]).

use std::path::{Path, PathBuf};

use greffier_core::settings::{self, PersistedSettings};

pub use greffier_core::settings::{app_folder, suggested_folder};

/// The settings remembered in the app folder.
pub fn remembered() -> PersistedSettings {
    PersistedSettings::read(&app_folder())
}

/// The chosen meetings folder, if one was chosen.
pub fn meetings_folder() -> Option<PathBuf> {
    match std::env::var_os("GREFFIER_FOLDER") {
        Some(d) => Some(PathBuf::from(d)),
        None => remembered().meetings_folder,
    }
}

/// Remembers the meetings folder (and creates it if it does not exist); with `GREFFIER_FOLDER`,
/// only creates it.
pub fn choose_meetings_folder(folder: &Path) -> Result<(), String> {
    if std::env::var_os("GREFFIER_FOLDER").is_some() {
        return settings::create_meetings_folder(folder);
    }
    settings::choose_meetings_folder(&app_folder(), folder)
}

/// Remembers the core's recording settings and the global shortcut (empty: none).
pub fn remember_recording_settings(r: &greffier_core::Settings, shortcut: &str) -> Result<(), String> {
    let mut remembered = remembered();
    remembered.from_meeting(r);
    remembered.shortcut = shortcut.to_string();
    remembered.remember(&app_folder())
}

/// A path for display: `~` for the home folder.
pub fn display(path: &Path) -> String {
    match path.strip_prefix(settings::home()) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

/// The Mac's local time, to date recordings and know what day it is.
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

/// The app's adapters: real transcription (whisper.cpp, models from the app folder), real
/// diarization (pyannote in the private Python), review and summary through the `claude` CLI
/// (relisten through whisper.cpp), real capture (ScreenCaptureKit), the Mac's clock, the real
/// resources and the Mac's trash.
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

/// The search cache, in the app folder.
pub fn search_cache() -> PathBuf {
    settings::search_cache(&app_folder())
}

/// The lexicon's people, offered to name speakers; headless, those of the fake data.
pub fn people(core: &greffier_core::Greffier) -> Vec<String> {
    if rune::is_headless() {
        return greffier_core::demo::PEOPLE.map(String::from).to_vec();
    }
    core.people()
}

/// The path of the app folder's lexicon (the bundled lexicon is written there if missing).
pub fn lexicon_path() -> Option<PathBuf> {
    greffier_core::lexicon::install(&app_folder()).map_err(|e| eprintln!("greffier: lexicon: {e}")).ok()
}
