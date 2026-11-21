//! Les réglages de l'app qui survivent à un redémarrage, dans son dossier
//! (`~/Library/Application Support/Greffier/reglages.json`) : le dossier de réunions, le raccourci
//! global, les sources, le démarrage automatique du traitement et les dossiers de contexte de
//! l'agent. Rien n'est écrit dans le dossier
//! de réunions lui-même. Les clés inconnues sont gardées.
//!
//! `GREFFIER_DOSSIER=/chemin` remplace le dossier choisi le temps d'un lancement, sans toucher
//! aux réglages (essais sur une copie, réunions d'exemple).

use std::path::{Path, PathBuf};

fn maison() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

/// Le dossier de l'app : modèles, environnement Python, lexique, cache, réglages.
pub fn dossier_de_l_app() -> PathBuf {
    greffier_coeur::ressources::dossier_de_l_app()
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
    modifier(|v| v["dossier_de_reunions"] = serde_json::Value::String(dossier.to_string_lossy().into_owned()))
}

/// Le raccourci global d'enregistrement par défaut.
pub const RACCOURCI_PAR_DEFAUT: &str = "cmd+shift+r";

/// Les réglages d'enregistrement retenus (les défauts du cœur pour ce qui manque) et le raccourci
/// global (au format de Rune, « cmd+shift+r »).
pub fn enregistrement() -> (greffier_coeur::Reglages, String) {
    let v = lire();
    let mut r = greffier_coeur::Reglages::default();
    if let Some(b) = v.get("traiter_a_l_arret").and_then(serde_json::Value::as_bool) {
        r.traiter_a_l_arret = b;
    }
    let source = |nom: &str, defaut: bool| v.get("sources").and_then(|s| s.get(nom)).and_then(serde_json::Value::as_bool).unwrap_or(defaut);
    r.sources = greffier_coeur::adaptateurs::Sources { ecran: source("ecran", r.sources.ecran), son_du_mac: source("son_du_mac", r.sources.son_du_mac), micro: source("micro", r.sources.micro) };
    r.dossiers_de_contexte = v.get("dossiers_de_contexte").and_then(serde_json::Value::as_array).map(|a| a.iter().filter_map(serde_json::Value::as_str).map(PathBuf::from).collect()).unwrap_or_default();
    let raccourci = v.get("raccourci").and_then(serde_json::Value::as_str).unwrap_or(RACCOURCI_PAR_DEFAUT).to_string();
    (r, raccourci)
}

/// Retient les réglages d'enregistrement et le raccourci global (vide : aucun).
pub fn retenir_enregistrement(r: &greffier_coeur::Reglages, raccourci: &str) -> Result<(), String> {
    modifier(|v| {
        v["traiter_a_l_arret"] = r.traiter_a_l_arret.into();
        v["sources"] = serde_json::json!({ "ecran": r.sources.ecran, "son_du_mac": r.sources.son_du_mac, "micro": r.sources.micro });
        v["raccourci"] = raccourci.into();
        v["dossiers_de_contexte"] = r.dossiers_de_contexte.iter().map(|d| serde_json::Value::String(d.to_string_lossy().into_owned())).collect();
    })
}

fn lire() -> serde_json::Value {
    std::fs::read_to_string(fichier()).ok().and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_else(|| serde_json::json!({}))
}

/// Change le fichier des réglages en gardant les autres clés.
fn modifier(f: impl FnOnce(&mut serde_json::Value)) -> Result<(), String> {
    let mut v = lire();
    if !v.is_object() {
        v = serde_json::json!({});
    }
    f(&mut v);
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
    fn en_heure_locale(&self, instant: std::time::SystemTime) -> greffier_coeur::DateHeure {
        use chrono::{Datelike, Timelike};
        let t: chrono::DateTime<chrono::Local> = instant.into();
        greffier_coeur::DateHeure::new(t.year(), t.month() as u8, t.day() as u8, t.hour() as u8, t.minute() as u8)
    }
}

/// Les adaptateurs de l'app : la vraie transcription (whisper.cpp, modèles du dossier de l'app),
/// la vraie séparation des locuteurs (pyannote dans le Python privé), la relecture et le bilan
/// par la CLI `claude` (réécoute par whisper.cpp), la vraie capture (ScreenCaptureKit), l'heure
/// du Mac et les vraies ressources.
pub fn adaptateurs() -> greffier_coeur::adaptateurs::Adaptateurs {
    use greffier_coeur::adaptateurs::Adaptateurs;
    use greffier_coeur::claude::{AgentClaude, ClaudeDuMac};
    use greffier_coeur::ressources::{Catalogue, RessourcesDeLApp};
    use std::sync::Arc;
    let app = dossier_de_l_app();
    let ressources = RessourcesDeLApp::new(app.clone(), Catalogue::de_greffier(), greffier_coeur::mac::OutilsDuMac::new());
    Adaptateurs {
        horloge: Box::new(HorlogeDuMac),
        ressources: Box::new(ressources),
        transcripteur: Box::new(greffier_moteurs::TranscripteurWhisper::de_l_app(&app)),
        separateur: Box::new(greffier_moteurs::SeparateurPyannote::de_l_app(&app)),
        agent: Box::new(AgentClaude::new(Arc::new(ClaudeDuMac::default()), Arc::new(greffier_moteurs::ReecouteWhisper::de_l_app(&app)))),
        capture: Box::new(greffier_capture::CaptureDuMac::new()),
    }
}

/// Le chemin du lexique du dossier de l'app (le lexique livré y est écrit s'il manque).
pub fn chemin_du_lexique() -> Option<PathBuf> {
    greffier_coeur::lexique::installer(&dossier_de_l_app()).map_err(|e| eprintln!("greffier : lexique : {e}")).ok()
}

/// Le lexique : celui du dossier de l'app (le lexique livré y est écrit s'il manque) ; sans
/// fenêtre, le lexique livré, sans rien écrire.
pub fn lexique() -> greffier_coeur::lexique::Resume {
    use greffier_coeur::lexique;
    if rune::is_headless() {
        return lexique::resume(lexique::LEXIQUE_INITIAL);
    }
    match lexique::installer(&dossier_de_l_app()).and_then(std::fs::read_to_string) {
        Ok(md) => lexique::resume(&md),
        Err(e) => {
            eprintln!("greffier : lexique : {e}");
            lexique::resume(lexique::LEXIQUE_INITIAL)
        }
    }
}

/// Les personnes du lexique, proposées pour nommer les locuteurs ; sans fenêtre, celles des
/// données fictives.
pub fn personnes() -> Vec<String> {
    if rune::is_headless() {
        return greffier_coeur::fictif::PERSONNES.map(String::from).to_vec();
    }
    chemin_du_lexique().and_then(|c| std::fs::read_to_string(c).ok()).map(|md| greffier_coeur::lexique::personnes(&md)).unwrap_or_default()
}
