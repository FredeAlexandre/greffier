//! The ⌘K search across the titles, summaries and transcriptions of all meetings, and the agents'
//! search (`greffier search`) on the same index, with its own filters and limits.
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
use crate::{DateTime, Greffier};

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

/// What an agent searches for (`greffier search`). The default searches everywhere, within the
/// command line's limits.
#[derive(Clone, Debug, PartialEq)]
pub struct SearchQuery {
    /// Words and "quoted phrases": a line is found if it contains all of them, ignoring case and
    /// accents (`bookmark` finds `bookmarks`).
    pub text: String,
    /// Only the meetings started from this day, until this one (both included; the time of day
    /// is ignored).
    pub since: Option<DateTime>,
    pub until: Option<DateTime>,
    /// Only the utterances of the speakers whose displayed name contains this.
    pub speaker: Option<String>,
    /// Where to search; empty: everywhere.
    pub locations: Vec<Location>,
    /// At most this many summary lines and utterances per meeting, and results in total.
    pub summary_lines: usize,
    pub utterances: usize,
    pub results: usize,
    /// The number of utterances given before and after each utterance found.
    pub context: usize,
    /// The excerpt's width, in characters: a longer line is cut around the first word found.
    pub width: usize,
}

impl Default for SearchQuery {
    fn default() -> SearchQuery {
        SearchQuery { text: String::new(), since: None, until: None, speaker: None, locations: Vec::new(), summary_lines: 5, utterances: 5, results: 30, context: 0, width: 200 }
    }
}

/// What the agents' search found.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SearchReport {
    /// Within the query's limits.
    pub results: Vec<Found>,
    /// The lines found without the limits, and the number of meetings they come from.
    pub total: usize,
    pub meetings: usize,
}

/// A line found by the agents' search.
#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    pub meeting: MeetingId,
    pub location: Location,
    /// As in [`SearchResult`].
    pub instant: Option<f64>,
    pub speaker: Option<String>,
    /// The utterance's index in the transcription (`Meeting::utterances`).
    pub utterance: Option<usize>,
    /// The line, cut around the first word found if it is wider than the query's width.
    pub excerpt: String,
    /// Every word and phrase found in `excerpt`, in bytes, in order and without overlap.
    pub highlights: Vec<(usize, usize)>,
    /// The utterances just before and after the one found (see [`SearchQuery::context`]).
    pub before: Vec<NearbyUtterance>,
    pub after: Vec<NearbyUtterance>,
}

/// An utterance around the one found, whole.
#[derive(Clone, Debug, PartialEq)]
pub struct NearbyUtterance {
    /// Its index in the transcription.
    pub index: usize,
    pub start: f64,
    pub speaker: Option<String>,
    pub text: String,
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

    /// Whether the line contains all the `terms` (already folded).
    fn contains(&self, terms: &[String]) -> bool {
        terms.iter().all(|t| self.folded.contains(t.as_str()))
    }

    /// The line cut to `width` characters around the first term found, and every term found in
    /// the excerpt.
    fn excerpt(&self, terms: &[String], width: usize) -> (String, Vec<(usize, usize)>) {
        // Each character's offset in the folded line and in the text (same characters, not
        // always the same bytes), to carry the folded occurrences over to the text.
        let (folded, text): (Vec<usize>, Vec<usize>) =
            self.folded.char_indices().zip(self.text.char_indices()).map(|((f, _), (t, _))| (f, t)).chain([(self.folded.len(), self.text.len())]).unzip();
        let to_text = |i: usize| text[folded.partition_point(|&f| f < i)];
        let mut found: Vec<(usize, usize)> = terms.iter().flat_map(|t| self.folded.match_indices(t.as_str()).map(|(i, m)| (to_text(i), to_text(i + m.len())))).collect();
        found.sort_unstable();
        let mut highlights: Vec<(usize, usize)> = Vec::new();
        for (a, b) in found {
            match highlights.last_mut() {
                Some(last) if a < last.1 => last.1 = last.1.max(b),
                _ => highlights.push((a, b)),
            }
        }
        let chars = text.len() - 1;
        let Some(&(a, b)) = highlights.first().filter(|_| chars > width) else { return (self.text.clone(), highlights) };
        // A window of `width` characters, a third of it before the first term found.
        let first = text.partition_point(|&t| t < a);
        let start = first.saturating_sub(width / 3).min(chars - width);
        let (excerpt, start, end) = cut(&self.text, text[start], text[start + width].max(b), a, b);
        let shift = |i: usize| i - start + if start > 0 { '…'.len_utf8() } else { 0 };
        let highlights = highlights.into_iter().filter(|&(x, y)| x >= start && y <= end).map(|(x, y)| (shift(x), shift(y))).collect();
        (excerpt, highlights)
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

    /// The utterances `range` of the transcription, whole.
    fn nearby(&self, range: std::ops::Range<usize>) -> Vec<NearbyUtterance> {
        let range = range.start.min(self.transcription.len())..range.end.min(self.transcription.len());
        (range.clone()).zip(&self.transcription[range]).map(|(index, l)| NearbyUtterance { index, start: l.instant.unwrap_or(0.0), speaker: l.speaker.clone(), text: l.text.clone() }).collect()
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
    let (excerpt, start, _) = cut(s, a.saturating_sub(50), (b + 90).min(s.len()), a, b);
    let offset = a - start + if start > 0 { '…'.len_utf8() } else { 0 };
    (excerpt, (offset, offset + (b - a)))
}

/// `s` cut to about `[start, end)` (in bytes) between two words, keeping `[a, b)`: the excerpt,
/// with "…" where it is cut, and the cut's bounds in `s`.
fn cut(s: &str, mut start: usize, mut end: usize, a: usize, b: usize) -> (String, usize, usize) {
    while !s.is_char_boundary(start) {
        start -= 1;
    }
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
    (format!("{before}{}{after}", &s[start..end]), start, end)
}

/// The query's words and "quoted phrases", folded.
fn terms(query: &str) -> Vec<String> {
    let mut terms = Vec::new();
    for (i, part) in query.split('"').enumerate() {
        if i % 2 == 1 {
            let phrase = part.split_whitespace().collect::<Vec<_>>().join(" ");
            terms.extend((!phrase.is_empty()).then(|| fold_text(&phrase)));
        } else {
            terms.extend(part.split_whitespace().map(fold_text));
        }
    }
    terms
}

/// A date's day, to compare meetings by day.
fn day(d: DateTime) -> (i32, u8, u8) {
    (d.year, d.month, d.day)
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
    /// to date (`up_to_date(name, fingerprints)`), the other files are erased. Read-only, the
    /// folder is only read, and the index then stays in memory.
    fn open(&mut self, cache: Option<PathBuf>, read_only: bool, up_to_date: impl Fn(&str, &Fingerprints) -> bool) {
        self.entries.clear();
        self.cache = None;
        let Some(cache) = cache else { return };
        for f in std::fs::read_dir(&cache).into_iter().flatten().flatten() {
            let path = f.path();
            let name = f.file_name().to_string_lossy().strip_suffix(".json").map(str::to_owned);
            match name.and_then(|name| Some((IndexEntry::read(&path).filter(|e| up_to_date(&name, &e.fingerprints))?, name))) {
                Some((e, name)) => _ = self.entries.insert(name, e),
                // Stale, unreadable, or an interrupted write (`.….json.partial`).
                None if !read_only => _ = std::fs::remove_file(&path),
                None => {}
            }
        }
        self.cache = (!read_only).then_some(cache);
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

    /// The lines containing all the words and phrases of `query`, with its filters, from the most
    /// recent meetings to the oldest: for each, the title, then the summary lines (they carry
    /// their topic's moment), then the utterances, within the query's limits.
    pub fn find(&self, query: &SearchQuery) -> SearchReport {
        let terms = terms(&query.text);
        let mut report = SearchReport::default();
        if terms.is_empty() {
            return report;
        }
        let speaker = query.speaker.as_deref().map(|s| fold_text(s.trim()));
        let speaks = |l: &Line| speaker.as_ref().is_none_or(|s| l.speaker.as_deref().is_some_and(|name| fold_text(name).contains(s.as_str())));
        // With a speaker, only utterances can be found.
        let searched = |l: Location| (query.locations.is_empty() || query.locations.contains(&l)) && (speaker.is_none() || l == Location::Transcription);
        for r in &self.meetings {
            if query.since.is_some_and(|d| day(r.start) < day(d)) || query.until.is_some_and(|d| day(r.start) > day(d)) {
                continue;
            }
            let Some(e) = self.index.entries.get(&r.folder_name) else { continue };
            let lines = (e.title.iter().map(|l| (Location::Title, None, l)))
                .chain(e.summary.iter().map(|l| (Location::Summary, None, l)))
                .chain(e.transcription.iter().enumerate().map(|(i, l)| (Location::Transcription, Some(i), l)))
                .filter(|&(location, _, l)| searched(location) && l.contains(&terms) && speaks(l));
            let (mut found, mut summary_lines, mut utterances) = (0, 0, 0);
            for (location, utterance, line) in lines {
                found += 1;
                let room = match location {
                    Location::Title => true,
                    Location::Summary => summary_lines < query.summary_lines,
                    Location::Transcription => utterances < query.utterances,
                };
                if !room || report.results.len() >= query.results {
                    continue;
                }
                match location {
                    Location::Title => {}
                    Location::Summary => summary_lines += 1,
                    Location::Transcription => utterances += 1,
                }
                let (before, after) = match utterance {
                    Some(i) => (e.nearby(i.saturating_sub(query.context)..i), e.nearby(i + 1..i.saturating_add(query.context).saturating_add(1))),
                    None => (Vec::new(), Vec::new()),
                };
                let (excerpt, highlights) = line.excerpt(&terms, query.width);
                report.results.push(Found { meeting: r.id, location, instant: line.instant, speaker: line.speaker.clone(), utterance, excerpt, highlights, before, after });
            }
            if found > 0 {
                report.total += found;
                report.meetings += 1;
            }
        }
        report
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
        self.open_search_cache(cache, false);
    }

    /// Takes the index from this cache folder without ever writing into it (the command line,
    /// which leaves the app folder to Greffier, ADR 0006): the entries up to date are read, the
    /// others rebuilt in memory only, and so are the following ones.
    pub fn read_search_cache(&mut self, cache: &Path) {
        self.open_search_cache(Some(cache.to_path_buf()), true);
    }

    fn open_search_cache(&mut self, cache: Option<PathBuf>, read_only: bool) {
        let expected: HashMap<&str, Fingerprints> = self.meetings.iter().map(|r| (r.folder_name.as_str(), self.fingerprints.get(&r.folder_name).cloned().unwrap_or_default())).collect();
        self.index.open(cache, read_only, |name, e| expected.get(name) == Some(e));
        self.indexer();
    }
}
