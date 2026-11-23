//! A meeting's identifier (glossary: Identifier): its short, stable name for the command line
//! and agents, taken from its folder's date and time (`2026-11-20 09h39 - Titre` gives
//! `2026-11-20-0939`). Several meetings in the same minute are told apart by their folder
//! names: the first in name order keeps the base, the next get `-2`, `-3`… The identifier only
//! depends on the folders of its own minute, so it survives a change of title elsewhere and new
//! meetings at other times.
//!
//! [`Identifiers::resolve`] reads what a user or an agent types to name a meeting.

use std::fmt;

use crate::folder::read_folder_name;
use crate::time::DateTime;

/// The identifier's base for a meeting starting at `start`: `2026-11-20-0939`.
pub fn base(start: DateTime) -> String {
    format!("{:04}-{:02}-{:02}-{:02}{:02}", start.year, start.month, start.day, start.hour, start.minute)
}

/// The identifiers of a meetings folder's meetings.
#[derive(Clone, Debug, Default)]
pub struct Identifiers {
    /// (identifier, folder name), in the order the folder names were given.
    entries: Vec<(String, String)>,
}

impl Identifiers {
    /// The identifiers of these meeting folders (as [`crate::MeetingsFolder::names`] gives them);
    /// a name that does not follow the naming rule gets none.
    pub fn new<S: AsRef<str>>(folder_names: impl IntoIterator<Item = S>) -> Identifiers {
        let named: Vec<(String, String)> = folder_names
            .into_iter()
            .filter_map(|n| {
                let n = n.as_ref();
                Some((base(read_folder_name(n)?.0), n.to_string()))
            })
            .collect();
        let mut entries = named.clone();
        for (i, (id, name)) in named.iter().enumerate() {
            let before = named.iter().filter(|(other, other_name)| other == id && other_name < name).count();
            if before > 0 {
                entries[i].0 = format!("{id}-{}", before + 1);
            }
        }
        Identifiers { entries }
    }

    /// The identifier of the meeting in this folder.
    pub fn of(&self, folder_name: &str) -> Option<&str> {
        self.entries.iter().find(|(_, n)| n == folder_name).map(|(id, _)| id.as_str())
    }

    /// (identifier, folder name), in the order the folder names were given.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.entries.iter().map(|(id, n)| (id.as_str(), n.as_str()))
    }

    /// The folder name of the meeting `query` names, in this order: an exact identifier; an
    /// exact folder name, or a path to the folder; a date (`2026-11-20`) or the start of an
    /// identifier that matches a single meeting.
    pub fn resolve(&self, query: &str) -> Result<&str, Unresolved> {
        let q = query.trim();
        let unresolved = |candidates: Vec<(String, String)>| Unresolved { query: q.to_string(), candidates };
        if q.is_empty() {
            return Err(unresolved(vec![]));
        }
        if let Some((_, n)) = self.entries.iter().find(|(id, _)| id == q) {
            return Ok(n);
        }
        let last = q.trim_end_matches('/').rsplit('/').next().unwrap_or(q);
        if let Some((_, n)) = self.entries.iter().find(|(_, n)| n == last) {
            return Ok(n);
        }
        let matching: Vec<&(String, String)> = self.entries.iter().filter(|(id, _)| id.starts_with(q)).collect();
        match matching.as_slice() {
            [(_, n)] => Ok(n),
            _ => Err(unresolved(matching.into_iter().map(|(id, n)| (id.clone(), read_folder_name(n).map_or_else(|| n.clone(), |(_, t)| t))).collect())),
        }
    }
}

/// What a user or an agent typed names no meeting, or several.
#[derive(Clone, Debug, PartialEq)]
pub struct Unresolved {
    pub query: String,
    /// (identifier, title) of the meetings it could name; empty: none.
    pub candidates: Vec<(String, String)>,
}

impl fmt::Display for Unresolved {
    /// « réunion introuvable : « 2026-12-01 » », or the candidates, one per line:
    /// `2026-11-20-0939 · Titre`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.candidates.is_empty() {
            return write!(f, "réunion introuvable : « {} »", self.query);
        }
        write!(f, "plusieurs réunions pour « {} » :", self.query)?;
        for (id, title) in &self.candidates {
            write!(f, "\n{id} · {title}")?;
        }
        Ok(())
    }
}

impl std::error::Error for Unresolved {}
