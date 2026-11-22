//! [`CorbeilleDuMac`] : la vraie corbeille, par `NSFileManager.trashItemAtURL`. Le dossier jeté
//! va dans la corbeille de son volume (`~/.Trash` pour le disque du Mac), d'où on peut le
//! ressortir dans le Finder.

use std::path::Path;

use greffier_core::adapters::Trash;
use objc2_foundation::{NSFileManager, NSString, NSURL};

pub struct MacTrash;

impl Trash for MacTrash {
    fn trash(&self, path: &Path) -> Result<(), String> {
        let path = path.to_str().ok_or_else(|| format!("chemin illisible : {}", path.display()))?;
        let url = NSURL::fileURLWithPath(&NSString::from_str(path));
        NSFileManager::defaultManager().trashItemAtURL_resultingItemURL_error(&url, None).map_err(|e| e.localizedDescription().to_string())
    }
}
