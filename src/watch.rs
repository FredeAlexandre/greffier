//! La surveillance du dossier de réunions : macOS (FSEvents) signale qu'un fichier a changé,
//! on réveille l'interface, et son `poll` demande au cœur de relire le dossier
//! (`Greffier::relire_le_dossier`). Le cœur seul décide de ce qui a vraiment changé : les
//! écritures de Greffier lui-même ne rechargent rien.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use notify::{RecursiveMode, Watcher};
use rune::Waker;

pub struct FolderWatch {
    _watcher: notify::RecommendedWatcher,
    change: Arc<AtomicBool>,
}

impl FolderWatch {
    /// Surveille `dossier` (et ses sous-dossiers) ; `waker` réveille l'interface à chaque changement.
    pub fn new(folder: &Path, waker: Waker) -> Result<FolderWatch, String> {
        let change = Arc::new(AtomicBool::new(false));
        let signal = change.clone();
        let mut watcher = notify::recommended_watcher(move |ev: notify::Result<notify::Event>| {
            // Les fichiers provisoires des écritures atomiques (`.<fichier>.greffier`) ne comptent pas.
            let temporary = |p: &std::path::PathBuf| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with('.') && n.ends_with(".greffier"));
            let account = match &ev {
                Ok(e) => e.need_rescan() || !e.paths.iter().all(temporary),
                Err(_) => true,
            };
            if account && !signal.swap(true, Ordering::AcqRel) {
                waker();
            }
        })
        .map_err(|e| e.to_string())?;
        watcher.watch(folder, RecursiveMode::Recursive).map_err(|e| format!("{} : {e}", folder.display()))?;
        Ok(FolderWatch { _watcher: watcher, change })
    }

    /// Vrai si le dossier a changé depuis le dernier appel.
    pub fn has_changed(&self) -> bool {
        self.change.swap(false, Ordering::AcqRel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU32;
    use std::time::{Duration, Instant};

    #[test]
    fn a_file_modified_in_the_folder_wakes_the_ui() {
        let folder = std::env::temp_dir().join(format!("greffier-watch-{}", std::process::id()));
        let meeting = folder.join("2026-11-20 10h00 - Revue");
        std::fs::create_dir_all(&meeting).unwrap();
        let wakes = Arc::new(AtomicU32::new(0));
        let counter = wakes.clone();
        let watch = FolderWatch::new(&folder, Arc::new(move || _ = counter.fetch_add(1, Ordering::SeqCst))).unwrap();
        std::thread::sleep(Duration::from_millis(200));
        std::fs::write(meeting.join("2026-11-20 10h00 - Revue - bilan.md"), "## En bref\n").unwrap();

        let start = Instant::now();
        while wakes.load(Ordering::SeqCst) == 0 && start.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = std::fs::remove_dir_all(&folder);
        assert!(wakes.load(Ordering::SeqCst) >= 1, "pas de réveil");
        assert!(watch.has_changed());
        assert!(!watch.has_changed(), "le changement n'est rendu qu'une fois");
    }
}
