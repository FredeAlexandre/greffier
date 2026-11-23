//! `greffier search <requête>`: what was said, found in the titles, summaries and transcriptions
//! of all the meetings, for an agent that must answer "what did we say about X, when, who" in
//! as few lines as possible.
//!
//! It runs the core's agents' search ([`Greffier::find`]) on a core opened read-only: adapters
//! that refuse everything ([`ReadOnly`]), no command sent, and the app's search cache read
//! without ever being written ([`Greffier::read_search_cache`]); entries missing or stale there
//! are built in memory.
//!
//! The text groups the lines by meeting, most recent first, the summary's before the
//! transcription's; the JSON gives the same lines flat, with their highlights.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use greffier_core::adapters::{
    Adapters, Agent, BoxedTask, Capture, CaptureRequest, CaptureState, Clock, DiarizationRequest, Diarizer, Mp4Muxer, Resource, Resources, ReviewRequest, ReviewResult, SummaryRequest, SummaryResult,
    Task, TaskProgress, Transcriber, TranscriptionRequest, Trash,
};
use greffier_core::search::{Found, Location, NearbyUtterance, SearchQuery};
use greffier_core::{DateTime, Greffier, MeetingId, SpeakerTurn, Word, playback_timestamp};
use serde::Serialize;

use crate::library::Library;
use crate::text::{self, Day};
use crate::{Context, Failure, HELP, args};

/// At most this many results in all, by default.
const LIMIT: usize = 30;

/// At most this many summary lines and as many utterances per meeting when the results come
/// from many meetings (the core's default), so that one meeting does not hide the others. When
/// fewer lines are found, the cap rises as far as the limit allows ([`share`]).
const PER_MEETING: usize = 5;

/// The other meetings named on the last line, at most.
const LISTED: usize = 10;

pub(crate) fn run(cx: &Context, args: &[String]) -> Result<String, Failure> {
    let a = args::parse(args, &["--json"], &["--since", "--until", "--date", "--speaker", "--in", "--context", "--limit"])?;
    if a.help() {
        return Ok(HELP.into());
    }
    let text = a.positionals.join(" ");
    if text.replace('"', "").trim().is_empty() {
        return Err(Failure::Usage("il manque la requête : des mots, ou une \"expression exacte\"".into()));
    }
    let days = |option: &str| -> Result<Option<(Day, Day)>, Failure> {
        a.value(option).map(|d| text::days(d).ok_or_else(|| Failure::Usage(format!("date invalide : « {d} » (AAAA-MM-JJ ou AAAA-MM)")))).transpose()
    };
    let (date, since, until) = (days("--date")?, days("--since")?, days("--until")?);
    let first = since.map(|d| d.0).into_iter().chain(date.map(|d| d.0)).max();
    let last = until.map(|d| d.1).into_iter().chain(date.map(|d| d.1)).min();
    let mut locations = Vec::new();
    for place in a.values("--in").iter().flat_map(|v| v.split(',')).map(str::trim).filter(|p| !p.is_empty()) {
        let l = match place {
            "bilan" => Location::Summary,
            "transcription" => Location::Transcription,
            "titre" => Location::Title,
            other => return Err(Failure::Usage(format!("--in : « {other} » (bilan, transcription ou titre)"))),
        };
        if !locations.contains(&l) {
            locations.push(l);
        }
    }
    let speaker = a.value("--speaker").map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
    if speaker.is_some() && !locations.is_empty() && !locations.contains(&Location::Transcription) {
        return Err(Failure::Usage("--speaker ne cherche que dans la transcription".into()));
    }
    let number = |option: &str, min: usize| -> Result<Option<usize>, Failure> {
        a.value(option).map(|n| n.parse::<usize>().ok().filter(|n| *n >= min).ok_or_else(|| Failure::Usage(format!("{option} attend un nombre : « {n} »")))).transpose()
    };
    let limit = number("--limit", 1)?.unwrap_or(LIMIT);
    let context = number("--context", 0)?.unwrap_or(0);

    let library = Library::open(cx)?;
    let mut g = Greffier::open(library.folder.root(), read_only());
    if let Some(app) = &cx.app_folder {
        g.read_search_cache(&greffier_core::settings::search_cache(app));
    }
    let day = |d: Day| DateTime::new(d.0, d.1, d.2, 0, 0);
    let mut query = SearchQuery { text: text.clone(), since: first.map(day), until: last.map(day), speaker, locations, results: limit, context, ..SearchQuery::default() };
    // Every line found, to share the limit and count what it leaves out, meeting by meeting.
    let everything = g.find(&SearchQuery { summary_lines: usize::MAX, utterances: usize::MAX, results: usize::MAX, context: 0, width: usize::MAX, ..query.clone() });
    let mut found: Vec<(MeetingId, Count)> = Vec::new();
    for f in &everything.results {
        if found.last().is_none_or(|(m, _)| *m != f.meeting) {
            found.push((f.meeting, Count::default()));
        }
        if let Some((_, count)) = found.last_mut() {
            count.add(f.location);
        }
    }
    query.summary_lines = share(found.iter().map(|(_, c)| c), limit);
    query.utterances = query.summary_lines;
    let report = g.find(&query);
    let results: Vec<Line> = report.results.into_iter().map(|f| Line::new(&g, &library, f)).collect();

    if a.flag("--json") {
        return Ok(crate::json(&SearchJson { resultats: results.iter().map(Line::json).collect(), total: report.total, reunions: report.meetings }));
    }
    if results.is_empty() {
        return Ok(format!("Aucun résultat pour « {text} ».\n"));
    }
    let mut s = String::new();
    let mut shown: HashMap<MeetingId, usize> = HashMap::new();
    let mut printed: HashSet<usize> = HashSet::new();
    for (i, r) in results.iter().enumerate() {
        if i == 0 || results[i - 1].found.meeting != r.found.meeting {
            s += &format!("{} · {}\n", r.id, r.title);
            printed.clear();
        }
        *shown.entry(r.found.meeting).or_default() += 1;
        let nearby = |u: &NearbyUtterance, printed: &mut HashSet<usize>| if printed.insert(u.index) { format!("    {}\n", utterance(u.start, u.speaker.as_deref(), &u.text)) } else { String::new() };
        s += &r.found.before.iter().map(|u| nearby(u, &mut printed)).collect::<String>();
        let at = r.found.instant.map(|t| format!("[{}] ", playback_timestamp(t))).unwrap_or_default();
        s += &match r.found.location {
            Location::Title => "  titre\n".to_string(),
            Location::Summary => format!("  {at}bilan · {}\n", r.excerpt),
            Location::Transcription => {
                printed.extend(r.found.utterance);
                format!("  {at}{} : {}\n", r.found.speaker.as_deref().unwrap_or("?"), r.excerpt)
            }
        };
        s += &r.found.after.iter().map(|u| nearby(u, &mut printed)).collect::<String>();
    }
    // What the limits left out, meeting by meeting, most recent first.
    let left: Vec<(MeetingId, usize)> = found.iter().map(|(m, c)| (*m, c.total() - shown.get(m).copied().unwrap_or(0))).filter(|(_, n)| *n > 0).collect();
    if !left.is_empty() {
        let hidden: usize = left.iter().map(|(_, n)| n).sum();
        let id = |m: MeetingId| g.meeting(m).and_then(|m| library.identifier(&m.folder_name)).unwrap_or("?").to_string();
        let mut listed: Vec<String> = left.iter().take(LISTED).map(|(m, n)| format!("{} ({n})", id(*m))).collect();
        if left.len() > LISTED {
            listed.push("…".into());
        }
        s += &format!(
            "… {hidden} {} dans {} {} : {}\nAffinez (--date, --since, --until, --speaker, --in) ou augmentez --limit.\n",
            if hidden == 1 { "autre résultat" } else { "autres résultats" },
            left.len(),
            if left.len() == 1 { "réunion" } else { "réunions" },
            listed.join(", ")
        );
    }
    Ok(s)
}

/// A meeting's lines found, by place.
#[derive(Clone, Copy, Default)]
struct Count {
    title: usize,
    summary: usize,
    transcription: usize,
}

impl Count {
    fn add(&mut self, l: Location) {
        match l {
            Location::Title => self.title += 1,
            Location::Summary => self.summary += 1,
            Location::Transcription => self.transcription += 1,
        }
    }

    fn total(&self) -> usize {
        self.title + self.summary + self.transcription
    }

    /// The lines shown with at most `cap` summary lines and `cap` utterances.
    fn within(&self, cap: usize) -> usize {
        self.title + self.summary.min(cap) + self.transcription.min(cap)
    }
}

/// The summary lines and utterances shown per meeting: the most whose lines all fit in the
/// limit, at least [`PER_MEETING`]. Everything found is shown when it fits; otherwise the
/// limit is shared between the meetings found rather than spent on the most recent.
fn share<'a>(counts: impl Iterator<Item = &'a Count> + Clone, limit: usize) -> usize {
    let most = counts.clone().map(|c| c.summary.max(c.transcription)).max().unwrap_or(0).min(limit);
    (PER_MEETING..=most).rev().find(|&cap| counts.clone().map(|c| c.within(cap)).sum::<usize>() <= limit).unwrap_or(PER_MEETING)
}

/// `[0:47:30] Jérôme ? : texte`
fn utterance(start: f64, speaker: Option<&str>, text: &str) -> String {
    format!("[{}] {} : {text}", playback_timestamp(start), speaker.unwrap_or("?"))
}

/// A line found, with its meeting's identifier and title, and its excerpt without the
/// summary's own "(h:mm:ss)", already given by its instant.
struct Line {
    id: String,
    title: String,
    excerpt: String,
    highlights: Vec<(usize, usize)>,
    found: Found,
}

impl Line {
    fn new(g: &Greffier, library: &Library, found: Found) -> Line {
        let meeting = g.meeting(found.meeting);
        let id = meeting.and_then(|m| library.identifier(&m.folder_name)).unwrap_or("?").to_string();
        let title = meeting.map(|m| m.title.clone()).unwrap_or_default();
        let (excerpt, highlights) = match (found.location, found.instant) {
            (Location::Summary, Some(t)) => without_stamp(&found.excerpt, &found.highlights, t),
            _ => (found.excerpt.clone(), found.highlights.clone()),
        };
        Line { id, title, excerpt, highlights, found }
    }

    fn json(&self) -> ResultJson {
        let f = &self.found;
        let nearby = |u: &NearbyUtterance| NearbyJson { replique: u.index + 1, debut: u.start, horodatage: playback_timestamp(u.start), locuteur: u.speaker.clone(), texte: u.text.clone() };
        ResultJson {
            id: self.id.clone(),
            titre: self.title.clone(),
            emplacement: match f.location {
                Location::Title => "titre",
                Location::Summary => "bilan",
                Location::Transcription => "transcription",
            },
            debut: f.instant,
            horodatage: f.instant.map(playback_timestamp),
            locuteur: f.speaker.clone(),
            replique: f.utterance.map(|i| i + 1),
            extrait: self.excerpt.clone(),
            surlignes: self.highlights.clone(),
            avant: f.before.iter().map(nearby).collect(),
            apres: f.after.iter().map(nearby).collect(),
        }
    }
}

/// The summary line without the "(h:mm:ss)" of `instant` (« (0:44:12) On vend… » becomes « On
/// vend… », « Bookmarks (1:58:31) » becomes « Bookmarks »), unless a highlight touches it; the
/// highlights follow.
fn without_stamp(excerpt: &str, highlights: &[(usize, usize)], instant: f64) -> (String, Vec<(usize, usize)>) {
    let stamp = format!("({})", playback_timestamp(instant));
    let unchanged = || (excerpt.to_string(), highlights.to_vec());
    let Some(i) = excerpt.find(&stamp) else { return unchanged() };
    let end = i + stamp.len();
    let (a, b) = if excerpt[end..].starts_with(' ') && (i == 0 || excerpt[..i].ends_with('…')) {
        (i, end + 1)
    } else if excerpt[..i].ends_with(' ') {
        (i - 1, end)
    } else {
        (i, end)
    };
    if highlights.iter().any(|&(x, y)| x < b && y > a) {
        return unchanged();
    }
    let shift = |x: usize| if x >= b { x - (b - a) } else { x };
    (format!("{}{}", &excerpt[..a], &excerpt[b..]), highlights.iter().map(|&(x, y)| (shift(x), shift(y))).collect())
}

#[derive(Serialize)]
struct SearchJson {
    resultats: Vec<ResultJson>,
    /// The lines found without the limits, and the number of meetings they come from.
    total: usize,
    reunions: usize,
}

#[derive(Serialize)]
struct ResultJson {
    id: String,
    titre: String,
    /// `titre`, `bilan` or `transcription`.
    emplacement: &'static str,
    /// The utterance's start, or the moment of a summary line (its own, or its topic's); in
    /// seconds. `null` for the title and a summary line without a moment.
    debut: Option<f64>,
    horodatage: Option<String>,
    locuteur: Option<String>,
    /// The utterance's number, from 1 (as in `show --transcription --numeros`).
    replique: Option<usize>,
    extrait: String,
    /// The words found in `extrait`, as byte ranges.
    surlignes: Vec<(usize, usize)>,
    avant: Vec<NearbyJson>,
    apres: Vec<NearbyJson>,
}

#[derive(Serialize)]
struct NearbyJson {
    replique: usize,
    debut: f64,
    horodatage: String,
    locuteur: Option<String>,
    texte: String,
}

// --- A core that cannot write ------------------------------------------------------------

/// Every adapter, refusing everything: the command line opens the core to read the meetings and
/// search them, never sends it a command nor advances it, and if it ever did, nothing would be
/// recorded, processed, trashed or written.
struct ReadOnly;

const READ_ONLY: &str = "la ligne de commande lit les réunions sans rien écrire";

/// A task that fails at once.
struct Refused;

impl<T> Task<T> for Refused {
    fn poll(&mut self, _: f64) -> TaskProgress<T> {
        TaskProgress::Failed(READ_ONLY.into())
    }
}

fn read_only() -> Adapters {
    Adapters {
        capture: Box::new(ReadOnly),
        transcriber: Box::new(ReadOnly),
        diarizer: Box::new(ReadOnly),
        agent: Box::new(ReadOnly),
        resources: Box::new(ReadOnly),
        clock: Box::new(ReadOnly),
        trash: Box::new(ReadOnly),
        mp4_muxing: Box::new(ReadOnly),
    }
}

impl Capture for ReadOnly {
    fn start(&mut self, _: CaptureRequest) -> Result<(), String> {
        Err(READ_ONLY.into())
    }
    fn poll(&mut self, _: f64) -> CaptureState {
        CaptureState::default()
    }
    fn stop(&mut self) -> Result<f64, String> {
        Err(READ_ONLY.into())
    }
}

impl Transcriber for ReadOnly {
    fn transcribe(&mut self, _: TranscriptionRequest) -> BoxedTask<Vec<Word>> {
        Box::new(Refused)
    }
}

impl Diarizer for ReadOnly {
    fn diarize(&mut self, _: DiarizationRequest) -> BoxedTask<Vec<SpeakerTurn>> {
        Box::new(Refused)
    }
}

impl Agent for ReadOnly {
    fn review(&mut self, _: ReviewRequest) -> BoxedTask<ReviewResult> {
        Box::new(Refused)
    }
    fn summarize(&mut self, _: SummaryRequest) -> BoxedTask<SummaryResult> {
        Box::new(Refused)
    }
}

impl Resources for ReadOnly {
    fn state(&mut self) -> Vec<Resource> {
        Vec::new()
    }
    fn install(&mut self) -> BoxedTask<()> {
        Box::new(Refused)
    }
    fn check(&mut self) -> BoxedTask<()> {
        Box::new(Refused)
    }
}

impl Clock for ReadOnly {
    fn now(&self) -> DateTime {
        self.to_local_time(std::time::SystemTime::now())
    }
}

impl Trash for ReadOnly {
    fn trash(&self, _: &Path) -> Result<(), String> {
        Err(READ_ONLY.into())
    }
}

impl Mp4Muxer for ReadOnly {
    fn to_mp4(&self, _: &Path, _: &Path) -> Result<(), String> {
        Err(READ_ONLY.into())
    }
}
