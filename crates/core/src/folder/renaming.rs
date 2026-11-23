//! Renaming a meeting when its title changes: the folder and each of its files
//! (`<name> - <role>`) take the new name, and the internal links of Markdown files that carried
//! the old name are rewritten.

use super::{MeetingsFolder, folder_name};
use crate::time::DateTime;

impl MeetingsFolder {
    /// Renames the `old` folder and its files after the new title; returns the new name (the
    /// old one if the title gives the same name). Forbidden characters become dashes, and a name
    /// already taken gets "(2)", "(3)"…
    ///
    /// The files are renamed one by one, then the folder; on the first failure, what was done is
    /// undone and nothing has changed. The Markdown files (summary, readable transcription) that
    /// name the old `<name> - …` are then rewritten with the new name; the sheet is rewritten by
    /// the core.
    pub fn rename(&self, old: &str, start: DateTime, title: &str) -> Result<String, String> {
        let base = folder_name(start, title);
        if old == base || collision_suffix(old, &base) {
            return Ok(old.to_string());
        }
        let source = self.folder(old);
        if !source.is_dir() {
            return Err(format!("{} : le dossier de la réunion n'existe plus", source.display()));
        }
        // The Mac's disk is case-insensitive: "réunion" → "Réunion" is the same folder.
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

        // Links to the renamed files (`[[<name> - réunion]]`, `<name> - transcription.md`).
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

/// `name` is `base (n)`: the same title, with a collision's number.
fn collision_suffix(name: &str, base: &str) -> bool {
    name.strip_prefix(base).and_then(|r| r.strip_prefix(" (")).and_then(|r| r.strip_suffix(')')).is_some_and(|n| !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit()))
}
