//! La surveillance du dossier de réunions : macOS (FSEvents) signale qu'un fichier a changé,
//! on réveille l'interface, et son `poll` demande au cœur de relire le dossier
//! (`Greffier::relire_le_dossier`). Le cœur seul décide de ce qui a vraiment changé : les
//! écritures de Greffier lui-même ne rechargent rien.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use notify::{RecursiveMode, Watcher};
use rune::Waker;

pub struct Surveillance {
    _watcher: notify::RecommendedWatcher,
    change: Arc<AtomicBool>,
}

impl Surveillance {
    /// Surveille `dossier` (et ses sous-dossiers) ; `waker` réveille l'interface à chaque changement.
    pub fn new(dossier: &Path, waker: Waker) -> Result<Surveillance, String> {
        let change = Arc::new(AtomicBool::new(false));
        let signal = change.clone();
        let mut watcher = notify::recommended_watcher(move |ev: notify::Result<notify::Event>| {
            // Les fichiers provisoires des écritures atomiques (`.<fichier>.greffier`) ne comptent pas.
            let provisoire = |p: &std::path::PathBuf| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with('.') && n.ends_with(".greffier"));
            let compte = match &ev {
                Ok(e) => e.need_rescan() || !e.paths.iter().all(provisoire),
                Err(_) => true,
            };
            if compte && !signal.swap(true, Ordering::AcqRel) {
                waker();
            }
        })
        .map_err(|e| e.to_string())?;
        watcher.watch(dossier, RecursiveMode::Recursive).map_err(|e| format!("{} : {e}", dossier.display()))?;
        Ok(Surveillance { _watcher: watcher, change })
    }

    /// Vrai si le dossier a changé depuis le dernier appel.
    pub fn a_change(&self) -> bool {
        self.change.swap(false, Ordering::AcqRel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU32;
    use std::time::{Duration, Instant};

    #[test]
    fn un_fichier_modifie_dans_le_dossier_reveille_l_interface() {
        let dossier = std::env::temp_dir().join(format!("greffier-surveillance-{}", std::process::id()));
        let reunion = dossier.join("2026-11-20 10h00 - Revue");
        std::fs::create_dir_all(&reunion).unwrap();
        let reveils = Arc::new(AtomicU32::new(0));
        let compteur = reveils.clone();
        let surveillance = Surveillance::new(&dossier, Arc::new(move || _ = compteur.fetch_add(1, Ordering::SeqCst))).unwrap();
        std::thread::sleep(Duration::from_millis(200));
        std::fs::write(reunion.join("2026-11-20 10h00 - Revue - bilan.md"), "## En bref\n").unwrap();

        let debut = Instant::now();
        while reveils.load(Ordering::SeqCst) == 0 && debut.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = std::fs::remove_dir_all(&dossier);
        assert!(reveils.load(Ordering::SeqCst) >= 1, "pas de réveil");
        assert!(surveillance.a_change());
        assert!(!surveillance.a_change(), "le changement n'est rendu qu'une fois");
    }
}
