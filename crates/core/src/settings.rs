//! The app's settings that survive a restart, in its folder
//! (`~/Library/Application Support/Greffier/settings.json`): the meetings folder, the global
//! shortcut, the sources, automatic start of processing and the agent's context folders.
//! Nothing is written into the meetings folder itself. Unknown keys in the file are kept (a
//! setting from a newer version survives a round trip through this one).
//!
//! It is also the only place that knows where the home folder, the app's folder and the
//! meetings folder are, so that the app and the command line agree on them. Two variables
//! replace them for a single launch, without touching the settings (trials on a copy, tests):
//! `GREFFIER_APP_FOLDER` the app folder (and so the settings and caches in it),
//! `GREFFIER_FOLDER` the meetings folder.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::Settings;
use crate::adapters::Sources;

/// The user's home folder (`$HOME`).
pub fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

/// Replaces the app folder for a single launch.
pub const APP_FOLDER_VARIABLE: &str = "GREFFIER_APP_FOLDER";

/// Replaces the meetings folder for a single launch.
pub const FOLDER_VARIABLE: &str = "GREFFIER_FOLDER";

/// A variable naming a folder; empty counts as unset.
fn folder_variable(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from)
}

/// The app's folder (models, private Python, lexicon, search cache, settings):
/// `GREFFIER_APP_FOLDER`, otherwise `~/Library/Application Support/Greffier`.
pub fn app_folder() -> PathBuf {
    app_folder_for(folder_variable(APP_FOLDER_VARIABLE), &home())
}

/// The app's folder: `replacement` if given, otherwise `<home>/Library/Application Support/Greffier`.
pub fn app_folder_for(replacement: Option<PathBuf>, home: &Path) -> PathBuf {
    replacement.unwrap_or_else(|| home.join("Library/Application Support/Greffier"))
}

/// The meetings folder `GREFFIER_FOLDER` gives for this launch, if any.
pub fn replaced_meetings_folder() -> Option<PathBuf> {
    folder_variable(FOLDER_VARIABLE)
}

/// The meetings folder: `GREFFIER_FOLDER`, otherwise the one remembered in the app folder;
/// `None` until one is chosen. Writes nothing.
pub fn meetings_folder() -> Option<PathBuf> {
    meetings_folder_for(replaced_meetings_folder(), &app_folder())
}

/// The meetings folder: `replacement` if given, otherwise the one remembered in `app_folder`.
pub fn meetings_folder_for(replacement: Option<PathBuf>, app_folder: &Path) -> Option<PathBuf> {
    replacement.or_else(|| PersistedSettings::read(app_folder).meetings_folder)
}

/// The meetings folder suggested on first launch.
pub fn suggested_folder() -> PathBuf {
    home().join("Documents/Réunions")
}

/// The search cache, in the app's folder.
pub fn search_cache(app_folder: &Path) -> PathBuf {
    app_folder.join("search")
}

/// The default global recording shortcut (in Rune's format).
pub const DEFAULT_SHORTCUT: &str = "cmd+shift+r";

/// The settings kept from one launch to the next.
#[derive(Clone, Debug, PartialEq)]
pub struct PersistedSettings {
    /// The chosen meetings folder, if one was chosen.
    pub meetings_folder: Option<PathBuf>,
    /// The global recording shortcut ("cmd+shift+r"); empty: none.
    pub shortcut: String,
    pub process_on_stop: bool,
    pub sources: Sources,
    pub context_folders: Vec<PathBuf>,
}

impl Default for PersistedSettings {
    fn default() -> PersistedSettings {
        let r = Settings::default();
        PersistedSettings { meetings_folder: None, shortcut: DEFAULT_SHORTCUT.into(), process_on_stop: r.process_on_stop, sources: r.sources, context_folders: r.context_folders }
    }
}

fn file(app_folder: &Path) -> PathBuf {
    app_folder.join("settings.json")
}

/// The settings file as is; missing or unreadable: an empty object.
fn read_file(app_folder: &Path) -> Value {
    std::fs::read_to_string(file(app_folder)).ok().and_then(|j| serde_json::from_str::<Value>(&j).ok()).filter(Value::is_object).unwrap_or_else(|| json!({}))
}

impl PersistedSettings {
    /// The settings kept in this app folder; whatever is missing (or everything, if the file is
    /// missing or unreadable) takes its default value. Writes nothing.
    pub fn read(app_folder: &Path) -> PersistedSettings {
        let v = read_file(app_folder);
        let d = PersistedSettings::default();
        let source = |name: &str, default: bool| v.get("sources").and_then(|s| s.get(name)).and_then(Value::as_bool).unwrap_or(default);
        PersistedSettings {
            meetings_folder: v.get("meetings_folder").and_then(Value::as_str).map(PathBuf::from),
            shortcut: v.get("shortcut").and_then(Value::as_str).map_or(d.shortcut, str::to_string),
            process_on_stop: v.get("process_on_stop").and_then(Value::as_bool).unwrap_or(d.process_on_stop),
            sources: Sources { screen: source("screen", d.sources.screen), mac_audio: source("mac_audio", d.sources.mac_audio), micro: source("micro", d.sources.micro) },
            context_folders: v.get("context_folders").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(PathBuf::from).collect()).unwrap_or(d.context_folders),
        }
    }

    /// Writes these settings into the app's folder (created if missing), keeping the keys this
    /// version does not know. Atomic write.
    pub fn remember(&self, app_folder: &Path) -> Result<(), String> {
        let mut v = read_file(app_folder);
        let path = |p: &Path| Value::String(p.to_string_lossy().into_owned());
        v["meetings_folder"] = self.meetings_folder.as_deref().map_or(Value::Null, path);
        v["shortcut"] = self.shortcut.clone().into();
        v["process_on_stop"] = self.process_on_stop.into();
        v["sources"] = json!({ "screen": self.sources.screen, "mac_audio": self.sources.mac_audio, "micro": self.sources.micro });
        v["context_folders"] = self.context_folders.iter().map(|d| path(d)).collect();
        if self.meetings_folder.is_none()
            && let Some(o) = v.as_object_mut()
        {
            o.remove("meetings_folder");
        }
        std::fs::create_dir_all(app_folder).map_err(|e| format!("{} : {e}", app_folder.display()))?;
        let mut text = serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?;
        text.push('\n');
        let f = file(app_folder);
        let temp = app_folder.join(".settings.json.greffier");
        std::fs::write(&temp, text).and_then(|()| std::fs::rename(&temp, &f)).map_err(|e| format!("{} : {e}", f.display()))
    }

    /// The core's settings with these applied (sources, processing start, context folders);
    /// lexicon and search cache do not change.
    pub fn apply(&self, settings: Settings) -> Settings {
        Settings { process_on_stop: self.process_on_stop, sources: self.sources, context_folders: self.context_folders.clone(), ..settings }
    }

    /// Takes the sources, processing start and context folders from the core's settings.
    pub fn from_meeting(&mut self, settings: &Settings) {
        self.process_on_stop = settings.process_on_stop;
        self.sources = settings.sources;
        self.context_folders = settings.context_folders.clone();
    }
}

/// Creates the meetings folder if it does not exist.
pub fn create_meetings_folder(folder: &Path) -> Result<(), String> {
    std::fs::create_dir_all(folder).map_err(|e| format!("{} : {e}", folder.display()))
}

/// Creates the meetings folder if it does not exist and remembers it for later launches.
pub fn choose_meetings_folder(app_folder: &Path, folder: &Path) -> Result<(), String> {
    create_meetings_folder(folder)?;
    let mut r = PersistedSettings::read(app_folder);
    r.meetings_folder = Some(folder.to_path_buf());
    r.remember(app_folder)
}
