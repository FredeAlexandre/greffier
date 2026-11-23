//! The fingerprints of a meeting's files: enough to see that a file changed since the core's
//! last pass without reading it again (modification date, size, inode).
//!
//! The core remembers the fingerprint of each meeting folder. When it writes a file itself, it
//! records the new fingerprint right away: its own writes are never mistaken for changes made
//! outside the app.

use std::collections::{BTreeMap, HashMap};
use std::io::{Seek, SeekFrom, Write};
use std::time::SystemTime;

use super::{MeetingsFolder, Role};
use crate::{Operation, Reason};

/// What changes when a file is rewritten: modification date, size, and inode (an atomic write
/// replaces the file).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Fingerprint {
    modified: Option<SystemTime>,
    size: u64,
    inode: u64,
}

/// The fingerprints of the files present in a meeting's folder.
pub type Fingerprints = BTreeMap<Role, Fingerprint>;

impl MeetingsFolder {
    /// The fingerprints of the existing files of meeting `name`.
    pub fn fingerprints(&self, name: &str) -> Fingerprints {
        Role::ALL
            .into_iter()
            .filter_map(|role| {
                // The recording may be an imported media file that keeps its extension (`.m4a`…).
                let path = if role == Role::Recording { self.recording(name)? } else { self.file(name, role) };
                let m = std::fs::metadata(path).ok().filter(|m| m.is_file())?;
                Some((role, Fingerprint { modified: m.modified().ok(), size: m.len(), inode: inode(&m) }))
            })
            .collect()
    }

    /// The fingerprints of all meeting folders, by name.
    pub fn all_fingerprints(&self) -> HashMap<String, Fingerprints> {
        self.names()
            .into_iter()
            .map(|name| {
                let e = self.fingerprints(&name);
                (name, e)
            })
            .collect()
    }

    /// Ticks or unticks the summary's `index`-th checkbox in its file, rewriting only the
    /// character between the brackets: the rest of the file (line endings, formatting) does not
    /// move. Returns the modified summary, or `None` if the meeting has no summary file.
    pub fn tick_in_summary(&self, name: &str, index: usize, done: bool) -> Result<Option<String>, Reason> {
        let path = self.file(name, Role::Summary);
        if !path.is_file() {
            return Ok(None);
        }
        let error = |e: std::io::Error| Reason::Failed { what: Operation::CheckAction(path.clone()), detail: e.to_string() };
        let mut md = std::fs::read_to_string(&path).map_err(error)?;
        let Some(position) = crate::summary::checkbox_position(&md, index) else { return Err(Reason::UnknownAction) };
        if crate::summary::tick(&mut md, index, done) {
            let mut f = std::fs::OpenOptions::new().write(true).open(&path).map_err(error)?;
            f.seek(SeekFrom::Start(position as u64)).map_err(error)?;
            f.write_all(&md.as_bytes()[position..position + 1]).map_err(error)?;
        }
        Ok(Some(md))
    }
}

#[cfg(unix)]
fn inode(m: &std::fs::Metadata) -> u64 {
    std::os::unix::fs::MetadataExt::ino(m)
}

#[cfg(not(unix))]
fn inode(_: &std::fs::Metadata) -> u64 {
    0
}
