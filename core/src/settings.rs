//! Les réglages de l'app qui survivent à un redémarrage, dans son dossier
//! (`~/Library/Application Support/Greffier/reglages.json`) : le dossier de réunions, le raccourci
//! global, les sources, le démarrage automatique du traitement et les dossiers de contexte de
//! l'agent. Rien n'est écrit dans le dossier de réunions lui-même. Les clés inconnues du fichier
//! sont gardées (un réglage d'une version plus récente survit à un passage par celle-ci).
//!
//! C'est aussi le seul endroit qui sait où sont le dossier personnel et le dossier de l'app.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::Settings;
use crate::adapters::Sources;

/// Le dossier personnel de l'utilisateur (`$HOME`).
pub fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

/// Le dossier de l'app : `~/Library/Application Support/Greffier` (modèles, Python privé,
/// lexique, cache de recherche, réglages).
pub fn app_folder() -> PathBuf {
    home().join("Library/Application Support/Greffier")
}

/// Le dossier de réunions proposé au premier lancement.
pub fn suggested_folder() -> PathBuf {
    home().join("Documents/Réunions")
}

/// Le cache de recherche, dans le dossier de l'app.
pub fn search_cache(app_folder: &Path) -> PathBuf {
    app_folder.join("search")
}

/// Le raccourci global d'enregistrement par défaut (au format de Rune).
pub const DEFAULT_SHORTCUT: &str = "cmd+shift+r";

/// Les réglages retenus d'un lancement à l'autre.
#[derive(Clone, Debug, PartialEq)]
pub struct PersistedSettings {
    /// Le dossier de réunions choisi, s'il l'a été.
    pub meetings_folder: Option<PathBuf>,
    /// Le raccourci global d'enregistrement (« cmd+shift+r ») ; vide : aucun.
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

/// Le fichier des réglages tel quel ; absent ou illisible : un objet vide.
fn read_file(app_folder: &Path) -> Value {
    std::fs::read_to_string(file(app_folder)).ok().and_then(|j| serde_json::from_str::<Value>(&j).ok()).filter(Value::is_object).unwrap_or_else(|| json!({}))
}

impl PersistedSettings {
    /// Les réglages retenus dans ce dossier de l'app ; ce qui manque (ou tout, si le fichier
    /// manque ou est illisible) prend sa valeur par défaut. N'écrit rien.
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

    /// Écrit ces réglages dans le dossier de l'app (créé s'il manque), en gardant les clés que
    /// cette version ne connaît pas. Écriture atomique.
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

    /// Les réglages du cœur avec ceux-ci (sources, démarrage du traitement, dossiers de
    /// contexte) ; lexique et cache de recherche ne changent pas.
    pub fn apply(&self, settings: Settings) -> Settings {
        Settings { process_on_stop: self.process_on_stop, sources: self.sources, context_folders: self.context_folders.clone(), ..settings }
    }

    /// Reprend sources, démarrage du traitement et dossiers de contexte des réglages du cœur.
    pub fn from_meeting(&mut self, settings: &Settings) {
        self.process_on_stop = settings.process_on_stop;
        self.sources = settings.sources;
        self.context_folders = settings.context_folders.clone();
    }
}

/// Crée le dossier de réunions s'il n'existe pas.
pub fn create_meetings_folder(folder: &Path) -> Result<(), String> {
    std::fs::create_dir_all(folder).map_err(|e| format!("{} : {e}", folder.display()))
}

/// Crée le dossier de réunions s'il n'existe pas et le retient pour les lancements suivants.
pub fn choose_meetings_folder(app_folder: &Path, folder: &Path) -> Result<(), String> {
    create_meetings_folder(folder)?;
    let mut r = PersistedSettings::read(app_folder);
    r.meetings_folder = Some(folder.to_path_buf());
    r.remember(app_folder)
}
