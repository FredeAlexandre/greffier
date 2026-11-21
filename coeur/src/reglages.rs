//! Les réglages de l'app qui survivent à un redémarrage, dans son dossier
//! (`~/Library/Application Support/Greffier/reglages.json`) : le dossier de réunions, le raccourci
//! global, les sources, le démarrage automatique du traitement et les dossiers de contexte de
//! l'agent. Rien n'est écrit dans le dossier de réunions lui-même. Les clés inconnues du fichier
//! sont gardées (un réglage d'une version plus récente survit à un passage par celle-ci).
//!
//! C'est aussi le seul endroit qui sait où sont le dossier personnel et le dossier de l'app.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::Reglages;
use crate::adaptateurs::Sources;

/// Le dossier personnel de l'utilisateur (`$HOME`).
pub fn maison() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

/// Le dossier de l'app : `~/Library/Application Support/Greffier` (modèles, Python privé,
/// lexique, cache de recherche, réglages).
pub fn dossier_de_l_app() -> PathBuf {
    maison().join("Library/Application Support/Greffier")
}

/// Le dossier de réunions proposé au premier lancement.
pub fn dossier_propose() -> PathBuf {
    maison().join("Documents/Réunions")
}

/// Le cache de recherche, dans le dossier de l'app.
pub fn cache_de_recherche(dossier_de_l_app: &Path) -> PathBuf {
    dossier_de_l_app.join("recherche")
}

/// Le raccourci global d'enregistrement par défaut (au format de Rune).
pub const RACCOURCI_PAR_DEFAUT: &str = "cmd+shift+r";

/// Les réglages retenus d'un lancement à l'autre.
#[derive(Clone, Debug, PartialEq)]
pub struct ReglagesRetenus {
    /// Le dossier de réunions choisi, s'il l'a été.
    pub dossier_de_reunions: Option<PathBuf>,
    /// Le raccourci global d'enregistrement (« cmd+shift+r ») ; vide : aucun.
    pub raccourci: String,
    pub traiter_a_l_arret: bool,
    pub sources: Sources,
    pub dossiers_de_contexte: Vec<PathBuf>,
}

impl Default for ReglagesRetenus {
    fn default() -> ReglagesRetenus {
        let r = Reglages::default();
        ReglagesRetenus { dossier_de_reunions: None, raccourci: RACCOURCI_PAR_DEFAUT.into(), traiter_a_l_arret: r.traiter_a_l_arret, sources: r.sources, dossiers_de_contexte: r.dossiers_de_contexte }
    }
}

fn fichier(dossier_de_l_app: &Path) -> PathBuf {
    dossier_de_l_app.join("reglages.json")
}

/// Le fichier des réglages tel quel ; absent ou illisible : un objet vide.
fn lire_le_fichier(dossier_de_l_app: &Path) -> Value {
    std::fs::read_to_string(fichier(dossier_de_l_app)).ok().and_then(|j| serde_json::from_str::<Value>(&j).ok()).filter(Value::is_object).unwrap_or_else(|| json!({}))
}

impl ReglagesRetenus {
    /// Les réglages retenus dans ce dossier de l'app ; ce qui manque (ou tout, si le fichier
    /// manque ou est illisible) prend sa valeur par défaut. N'écrit rien.
    pub fn lire(dossier_de_l_app: &Path) -> ReglagesRetenus {
        let v = lire_le_fichier(dossier_de_l_app);
        let d = ReglagesRetenus::default();
        let source = |nom: &str, defaut: bool| v.get("sources").and_then(|s| s.get(nom)).and_then(Value::as_bool).unwrap_or(defaut);
        ReglagesRetenus {
            dossier_de_reunions: v.get("dossier_de_reunions").and_then(Value::as_str).map(PathBuf::from),
            raccourci: v.get("raccourci").and_then(Value::as_str).map_or(d.raccourci, str::to_string),
            traiter_a_l_arret: v.get("traiter_a_l_arret").and_then(Value::as_bool).unwrap_or(d.traiter_a_l_arret),
            sources: Sources { ecran: source("ecran", d.sources.ecran), son_du_mac: source("son_du_mac", d.sources.son_du_mac), micro: source("micro", d.sources.micro) },
            dossiers_de_contexte: v.get("dossiers_de_contexte").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(PathBuf::from).collect()).unwrap_or(d.dossiers_de_contexte),
        }
    }

    /// Écrit ces réglages dans le dossier de l'app (créé s'il manque), en gardant les clés que
    /// cette version ne connaît pas. Écriture atomique.
    pub fn retenir(&self, dossier_de_l_app: &Path) -> Result<(), String> {
        let mut v = lire_le_fichier(dossier_de_l_app);
        let chemin = |p: &Path| Value::String(p.to_string_lossy().into_owned());
        v["dossier_de_reunions"] = self.dossier_de_reunions.as_deref().map_or(Value::Null, chemin);
        v["raccourci"] = self.raccourci.clone().into();
        v["traiter_a_l_arret"] = self.traiter_a_l_arret.into();
        v["sources"] = json!({ "ecran": self.sources.ecran, "son_du_mac": self.sources.son_du_mac, "micro": self.sources.micro });
        v["dossiers_de_contexte"] = self.dossiers_de_contexte.iter().map(|d| chemin(d)).collect();
        if self.dossier_de_reunions.is_none()
            && let Some(o) = v.as_object_mut()
        {
            o.remove("dossier_de_reunions");
        }
        std::fs::create_dir_all(dossier_de_l_app).map_err(|e| format!("{} : {e}", dossier_de_l_app.display()))?;
        let mut texte = serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?;
        texte.push('\n');
        let f = fichier(dossier_de_l_app);
        let temporaire = dossier_de_l_app.join(".reglages.json.greffier");
        std::fs::write(&temporaire, texte).and_then(|()| std::fs::rename(&temporaire, &f)).map_err(|e| format!("{} : {e}", f.display()))
    }

    /// Les réglages du cœur avec ceux-ci (sources, démarrage du traitement, dossiers de
    /// contexte) ; lexique et cache de recherche ne changent pas.
    pub fn appliquer(&self, reglages: Reglages) -> Reglages {
        Reglages { traiter_a_l_arret: self.traiter_a_l_arret, sources: self.sources, dossiers_de_contexte: self.dossiers_de_contexte.clone(), ..reglages }
    }

    /// Reprend sources, démarrage du traitement et dossiers de contexte des réglages du cœur.
    pub fn depuis(&mut self, reglages: &Reglages) {
        self.traiter_a_l_arret = reglages.traiter_a_l_arret;
        self.sources = reglages.sources;
        self.dossiers_de_contexte = reglages.dossiers_de_contexte.clone();
    }
}

/// Crée le dossier de réunions s'il n'existe pas.
pub fn creer_dossier_de_reunions(dossier: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dossier).map_err(|e| format!("{} : {e}", dossier.display()))
}

/// Crée le dossier de réunions s'il n'existe pas et le retient pour les lancements suivants.
pub fn choisir_dossier_de_reunions(dossier_de_l_app: &Path, dossier: &Path) -> Result<(), String> {
    creer_dossier_de_reunions(dossier)?;
    let mut r = ReglagesRetenus::lire(dossier_de_l_app);
    r.dossier_de_reunions = Some(dossier.to_path_buf());
    r.retenir(dossier_de_l_app)
}
