//! Le renommage d'une réunion quand son titre change : le dossier et chacun de ses fichiers
//! (`<nom> - <rôle>`) prennent le nouveau nom, et les liens internes des fichiers Markdown qui
//! portaient l'ancien nom sont réécrits.

use super::{MeetingsFolder, folder_name};
use crate::time::DateTime;

impl MeetingsFolder {
    /// Renomme le dossier `ancien` et ses fichiers d'après le nouveau titre ; rend le nouveau
    /// nom (l'ancien si le titre donne le même nom). Les caractères interdits deviennent des
    /// tirets, et un nom déjà pris reçoit « (2) », « (3) »…
    ///
    /// Les fichiers sont renommés un à un puis le dossier ; au premier échec, ce qui a été fait
    /// est défait et rien n'a changé. Les fichiers Markdown (bilan, transcription lisible) qui
    /// nomment l'ancien `<nom> - …` sont ensuite réécrits avec le nouveau nom ; la fiche, elle,
    /// est réécrite par le cœur.
    pub fn rename(&self, old: &str, start: DateTime, title: &str) -> Result<String, String> {
        let base = folder_name(start, title);
        if old == base || collision_suffix(old, &base) {
            return Ok(old.to_string());
        }
        let source = self.folder(old);
        if !source.is_dir() {
            return Err(format!("{} : le dossier de la réunion n'existe plus", source.display()));
        }
        // Le disque du Mac ignore la casse : « réunion » → « Réunion » est le même dossier.
        let available = |name: &str| !self.folder(name).exists() || name.to_lowercase() == old.to_lowercase();
        let mut new = base.clone();
        let mut n = 2;
        while !available(&new) {
            new = format!("{base} ({n})");
            n += 1;
        }

        let prefix = format!("{old} - ");
        let entries = std::fs::read_dir(&source).map_err(|e| format!("{} : {e}", source.display()))?;
        let to_rename: Vec<String> = entries.flatten().filter_map(|e| e.file_name().into_string().ok()).filter(|f| f.starts_with(&prefix)).collect();
        let mut done: Vec<(String, String)> = Vec::new();
        let undo = |done: &[(String, String)]| {
            for (before, after) in done.iter().rev() {
                let _ = std::fs::rename(source.join(after), source.join(before));
            }
        };
        for file in to_rename {
            let target = format!("{new} - {}", &file[prefix.len()..]);
            if let Err(e) = std::fs::rename(source.join(&file), source.join(&target)) {
                undo(&done);
                return Err(format!("{file} : {e}"));
            }
            done.push((file, target));
        }
        if let Err(e) = std::fs::rename(&source, self.folder(&new)) {
            undo(&done);
            return Err(format!("{} : {e}", source.display()));
        }

        // Les liens vers les fichiers renommés (`[[<nom> - réunion]]`, `<nom> - transcription.md`).
        let new_prefix = format!("{new} - ");
        for (_, file) in &done {
            let path = self.folder(&new).join(file);
            if !file.ends_with(".md") {
                continue;
            }
            if let Ok(md) = std::fs::read_to_string(&path)
                && md.contains(&prefix)
            {
                let _ = super::write_atomically(&path, &md.replace(&prefix, &new_prefix));
            }
        }
        Ok(new)
    }
}

/// `nom` est `base (n)` : le même titre, avec le numéro d'une collision.
fn collision_suffix(name: &str, base: &str) -> bool {
    name.strip_prefix(base).and_then(|r| r.strip_prefix(" (")).and_then(|r| r.strip_suffix(')')).is_some_and(|n| !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit()))
}
