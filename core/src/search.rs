//! The ⌘K search across the titles, summaries and transcriptions of all meetings.
//!
//! The core keeps an index: for each meeting folder, its title, the lines of its summary (with
//! the instant of their topic) and its utterances (with their start and speaker). An entry is
//! rebuilt when the fingerprints of the meeting's files change (a write by Greffier or a change
//! outside the app); it disappears with the meeting (deletion, renaming).
//!
//! The search ignores case and accents ("equipe" finds "Équipe") and the typographic
//! apostrophe. A result says where to open the meeting: the utterance, or the topic (the
//! decision) of a summary line.
//!
//! **The cache** (ADR 0001: the app's only state outside the meetings folder, rebuildable): one
//! JSON file per meeting, `<cache>/<folder name>.json`, with the fingerprints of the files it
//! comes from. At launch, a cache entry is used if its fingerprints match the folder's;
//! otherwise (files changed while the app was closed, cache damaged, missing, or from another
//! version) it is rebuilt from the meeting. After that, each rebuilt entry is rewritten and each
//! removed entry is erased.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::summary::first_timestamp;
use crate::folder::watch::Fingerprints;
use crate::meeting::{MeetingId, Meeting};
use crate::Greffier;

/// Where the word was found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Location {
    Title,
    Summary,
    Transcription,
}

/// A passage found.
#[derive(Clone, Debug, PartialEq)]
pub struct SearchResult {
    pub meeting: MeetingId,
    pub location: Location,
    /// The instant at which to open the meeting, in seconds: the start of the utterance, or that
    /// of the topic (of the decision) for a summary line. None: the start of the meeting.
    pub instant: Option<f64>,
    /// The utterance's speaker, as displayed.
    pub speaker: Option<String>,
    /// The passage around the word found (cut with "…").
    pub excerpt: String,
    /// The word found in `excerpt`, in bytes.
    pub highlight: (usize, usize),
}

/// At most this many summary lines and utterances per meeting, and results in total.
const SUMMARY_LINES: usize = 3;
const UTTERANCES: usize = 4;
const RESULTS: usize = 40;

/// The cache format's version: another version is rebuilt.
const VERSION: u32 = 1;

/// A searchable line: the text, its folded version (same number of characters), its instant.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Line {
    text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    instant: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    speaker: Option<String>,
    #[serde(skip)]
    folded: String,
}

impl Line {
    fn new(text: String, instant: Option<f64>, speaker: Option<String>) -> Line {
        let folded = fold_text(&text);
        Line { text, instant, speaker, folded }
    }

    /// The word `query` (already folded) in the line: the excerpt and the highlighted passage.
    fn find(&self, query: &str) -> Option<(String, (usize, usize))> {
        let i = self.folded.find(query)?;
        let start = self.folded[..i].chars().count();
        let end = start + query.chars().count();
        let byte = |n: usize| self.text.char_indices().nth(n).map_or(self.text.len(), |(o, _)| o);
        Some(excerpt(&self.text, byte(start), byte(end)))
    }
}

/// What the index keeps of a meeting (and the cache file).
#[derive(Clone, Debug, Serialize, Deserialize)]
struct IndexEntry {
    version: u32,
    /// The files' fingerprints when the entry was made.
    fingerprints: Fingerprints,
    title: Option<Line>,
    summary: Vec<Line>,
    transcription: Vec<Line>,
}

impl IndexEntry {
    fn from(r: &Meeting, fingerprints: Fingerprints) -> IndexEntry {
        let title = (!r.title.trim().is_empty() && r.title != "Réunion").then(|| Line::new(r.title.clone(), None, None));
        let transcription = r
            .utterances
            .iter()
            .map(|u| {
                let speaker = r.speakers.get(r.displayed_speaker(u.speaker)).map(|l| l.display_name());
                Line::new(u.text.clone(), Some(u.start), speaker)
            })
            .collect();
        IndexEntry { version: VERSION, fingerprints, title, summary: summary_lines(&r.summary), transcription }
    }

    /// The cache entry, if it is readable and of this version.
    fn read(path: &Path) -> Option<IndexEntry> {
        let mut e: IndexEntry = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
        if e.version != VERSION {
            return None;
        }
        for l in e.title.iter_mut().chain(&mut e.summary).chain(&mut e.transcription) {
            l.folded = fold_text(&l.text);
        }
        Some(e)
    }
}

/// The summary's lines, without the section headings. A line takes its own timestamp (a
/// decision), otherwise that of the `### Title (h:mm:ss)` topic containing it.
fn summary_lines(md: &str) -> Vec<Line> {
    let mut topic = None;
    let mut lines = Vec::new();
    for line in md.lines() {
        let t = line.trim();
        if t.is_empty() || t == "---" {
            continue;
        }
        if t.starts_with("# ") || t.starts_with("## ") {
            topic = None;
            continue;
        }
        if let Some(title) = t.strip_prefix("### ") {
            topic = first_timestamp(title);
        }
        let mut text = t.trim_start_matches(['#', '-', '*', '>', '|', ' ']);
        for case in ["[ ] ", "[x] ", "[X] "] {
            text = text.strip_prefix(case).unwrap_or(text);
        }
        let text = text.replace("**", "");
        if !text.trim().is_empty() {
            lines.push(Line::new(text.trim().to_string(), first_timestamp(t).or(topic), None));
        }
    }
    lines
}

/// Lowercase, without accents, straight apostrophe: one character for one character, to find
/// the passage again in the original text.
fn fold_text(s: &str) -> String {
    s.chars().map(fold_char).collect()
}

fn fold_char(c: char) -> char {
    let mut l = c.to_lowercase();
    let c = match (l.next(), l.next()) {
        (Some(x), None) => x,
        _ => c,
    };
    match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => 'a',
        'ç' => 'c',
        'è' | 'é' | 'ê' | 'ë' => 'e',
        'ì' | 'í' | 'î' | 'ï' => 'i',
        'ñ' => 'n',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' => 'o',
        'ù' | 'ú' | 'û' | 'ü' => 'u',
        'ý' | 'ÿ' => 'y',
        '’' | '‘' => '\'',
        _ => c,
    }
}

/// An excerpt of `s` around `[a, b)` (in bytes), cut between two words, and the passage's
/// position in the excerpt.
fn excerpt(s: &str, a: usize, b: usize) -> (String, (usize, usize)) {
    let mut start = a.saturating_sub(50);
    while !s.is_char_boundary(start) {
        start -= 1;
    }
    let mut end = (b + 90).min(s.len());
    while !s.is_char_boundary(end) {
        end += 1;
    }
    // Cut between two words rather than in the middle of one.
    if start > 0 && let Some(i) = s[start..a].find(' ') {
        start += i + 1;
    }
    if end < s.len() && let Some(i) = s[b..end].rfind(' ') {
        end = b + i;
    }
    let before = if start > 0 { "…" } else { "" };
    let after = if end < s.len() { "…" } else { "" };
    let offset = before.len() + a - start;
    (format!("{before}{}{after}", &s[start..end]), (offset, offset + (b - a)))
}

/// The search index, by meeting folder name.
#[derive(Default)]
pub(crate) struct Index {
    entries: HashMap<String, IndexEntry>,
    /// The cache folder, if there is one.
    cache: Option<PathBuf>,
}

impl Index {
    fn file(cache: &Path, name: &str) -> PathBuf {
        cache.join(format!("{name}.json"))
    }

    /// Writes the entry to the cache (no error: the cache can be rebuilt).
    fn write(&self, name: &str, e: &IndexEntry) {
        let Some(cache) = &self.cache else { return };
        let Ok(json) = serde_json::to_vec(e) else { return };
        let partial = cache.join(format!(".{name}.json.partial"));
        if std::fs::create_dir_all(cache).is_ok() && std::fs::write(&partial, json).is_ok() {
            let _ = std::fs::rename(&partial, Index::file(cache, name));
        }
    }

    fn erase(&self, name: &str) {
        if let Some(cache) = &self.cache {
            let _ = std::fs::remove_file(Index::file(cache, name));
        }
    }

    /// Takes `cache` as the cache folder: the index restarts from the entries in it that are up
    /// to date (`up_to_date(name, fingerprints)`), the other files are erased.
    fn open(&mut self, cache: Option<PathBuf>, up_to_date: impl Fn(&str, &Fingerprints) -> bool) {
        self.entries.clear();
        self.cache = cache;
        let Some(cache) = &self.cache else { return };
        for f in std::fs::read_dir(cache).into_iter().flatten().flatten() {
            let path = f.path();
            let name = f.file_name().to_string_lossy().strip_suffix(".json").map(str::to_owned);
            match name.and_then(|name| Some((IndexEntry::read(&path).filter(|e| up_to_date(&name, &e.fingerprints))?, name))) {
                Some((e, name)) => _ = self.entries.insert(name, e),
                // Stale, unreadable, or an interrupted write (`.….json.partial`).
                None => _ = std::fs::remove_file(&path),
            }
        }
    }
}

impl Greffier {
    /// The passages containing `query` (ignoring case and accents), from the most recent
    /// meetings to the oldest: for each, the title, then a few summary lines, then a few
    /// utterances.
    pub fn search(&self, query: &str) -> Vec<SearchResult> {
        let q = fold_text(query.trim());
        let mut results = Vec::new();
        if q.is_empty() {
            return results;
        }
        for r in &self.meetings {
            let Some(e) = self.index.entries.get(&r.folder_name) else { continue };
            let title = e.title.as_ref().and_then(|l| Some((Location::Title, (l, l.find(&q)?))));
            let summary = e.summary.iter().filter_map(|l| Some((l, l.find(&q)?))).take(SUMMARY_LINES).map(|x| (Location::Summary, x));
            let transcription = e.transcription.iter().filter_map(|l| Some((l, l.find(&q)?))).take(UTTERANCES).map(|x| (Location::Transcription, x));
            for (location, (line, (excerpt, highlight))) in title.into_iter().chain(summary).chain(transcription) {
                results.push(SearchResult { meeting: r.id, location, instant: line.instant, speaker: line.speaker.clone(), excerpt, highlight });
            }
            if results.len() >= RESULTS {
                break;
            }
        }
        results
    }

    /// Updates the index: an entry rebuilt for each meeting whose files have changed since, an
    /// entry removed for each meeting that no longer exists.
    pub(crate) fn indexer(&mut self) {
        for r in &self.meetings {
            let fingerprints = self.fingerprints.get(&r.folder_name).cloned().unwrap_or_default();
            if self.index.entries.get(&r.folder_name).is_some_and(|e| e.fingerprints == fingerprints) {
                continue;
            }
            let e = IndexEntry::from(r, fingerprints);
            self.index.write(&r.folder_name, &e);
            self.index.entries.insert(r.folder_name.clone(), e);
        }
        let names: HashSet<&str> = self.meetings.iter().map(|r| r.folder_name.as_str()).collect();
        let gone: Vec<String> = self.index.entries.keys().filter(|name| !names.contains(name.as_str())).cloned().collect();
        for name in gone {
            self.index.erase(&name);
            self.index.entries.remove(&name);
        }
    }

    /// Keeps the index in this folder (or in memory only): see the cache above.
    pub(crate) fn use_search_cache(&mut self, cache: Option<PathBuf>) {
        let expected: HashMap<&str, Fingerprints> = self.meetings.iter().map(|r| (r.folder_name.as_str(), self.fingerprints.get(&r.folder_name).cloned().unwrap_or_default())).collect();
        self.index.open(cache, |name, e| expected.get(name) == Some(e));
        self.indexer();
    }
}
