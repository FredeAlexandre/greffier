//! [`CorbeilleDuMac`] : la vraie corbeille, par `NSFileManager.trashItemAtURL`. Le dossier jeté
//! va dans la corbeille de son volume (`~/.Trash` pour le disque du Mac), d'où on peut le
//! ressortir dans le Finder.

use std::path::Path;

use greffier_coeur::adaptateurs::Corbeille;
use objc2_foundation::{NSFileManager, NSString, NSURL};

pub struct CorbeilleDuMac;

impl Corbeille for CorbeilleDuMac {
    fn jeter(&self, chemin: &Path) -> Result<(), String> {
        let chemin = chemin.to_str().ok_or_else(|| format!("chemin illisible : {}", chemin.display()))?;
        let url = NSURL::fileURLWithPath(&NSString::from_str(chemin));
        NSFileManager::defaultManager().trashItemAtURL_resultingItemURL_error(&url, None).map_err(|e| e.localizedDescription().to_string())
    }
}
