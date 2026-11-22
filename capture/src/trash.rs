//! [`MacTrash`]: the real Trash, through `NSFileManager.trashItemAtURL`. The trashed folder goes
//! to its volume's Trash (`~/.Trash` for the Mac's disk), from where it can be put back in the
//! Finder.

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
