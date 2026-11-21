//! Les réglages de l'app qui survivent à un redémarrage, dans son dossier
//! (`~/Library/Application Support/Greffier/reglages.json`) : pour l'instant, le dossier de
//! réunions. Rien n'est écrit dans le dossier de réunions lui-même.
//!
//! `GREFFIER_DOSSIER=/chemin` remplace le dossier choisi le temps d'un lancement, sans toucher
//! aux réglages (essais sur une copie, réunions d'exemple).

use std::path::{Path, PathBuf};

fn maison() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

/// Le dossier de l'app : modèles, environnement Python, lexique, cache, réglages.
pub fn dossier_de_l_app() -> PathBuf {
    maison().join("Library/Application Support/Greffier")
}

fn fichier() -> PathBuf {
    dossier_de_l_app().join("reglages.json")
}

/// Le dossier proposé au premier lancement.
pub fn dossier_propose() -> PathBuf {
    maison().join("Documents/Réunions")
}

/// Le dossier de réunions choisi, s'il l'a été.
pub fn dossier_de_reunions() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("GREFFIER_DOSSIER") {
        return Some(PathBuf::from(d));
    }
    let json = std::fs::read_to_string(fichier()).ok()?;
    let v: serde_json::Value = serde_json::from_str(&json).ok()?;
    v.get("dossier_de_reunions")?.as_str().map(PathBuf::from)
}

/// Retient le dossier de réunions (et le crée s'il n'existe pas).
pub fn choisir_dossier_de_reunions(dossier: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dossier).map_err(|e| format!("{} : {e}", dossier.display()))?;
    if std::env::var_os("GREFFIER_DOSSIER").is_some() {
        return Ok(());
    }
    let mut v: serde_json::Value = std::fs::read_to_string(fichier()).ok().and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_else(|| serde_json::json!({}));
    v["dossier_de_reunions"] = serde_json::Value::String(dossier.to_string_lossy().into_owned());
    std::fs::create_dir_all(dossier_de_l_app()).map_err(|e| e.to_string())?;
    let mut json = serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?;
    json.push('\n');
    std::fs::write(fichier(), json).map_err(|e| format!("{} : {e}", fichier().display()))
}

/// Un chemin à afficher : `~` pour le dossier personnel.
pub fn afficher(chemin: &Path) -> String {
    match chemin.strip_prefix(maison()) {
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
}

/// Les adaptateurs de l'app : ceux du prototype pour le traitement (en attendant les vrais),
/// avec l'heure du Mac.
pub fn adaptateurs() -> greffier_coeur::adaptateurs::Adaptateurs {
    greffier_coeur::adaptateurs::Adaptateurs { horloge: Box::new(HorlogeDuMac), ..greffier_coeur::fictif::adaptateurs() }
}
