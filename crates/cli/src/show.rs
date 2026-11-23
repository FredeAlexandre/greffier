//! `greffier show <meeting>`: a meeting, by default its header and its summary's outline (brief,
//! topics, decisions, number of actions); or one view: the summary, the sheet, the
//! transcription, the speakers, the actions or the passages to check. Utterances, actions and
//! passages are numbered from 1 (`#n`); speakers by their index, as in their label (SPEAKER_00
//! is 0).

use greffier_core::summary::{self, checkboxes, without_front_matter};
use greffier_core::{Decision, Meeting, PassageStatus, Role, Topic, playback_timestamp};
use serde::Serialize;

use crate::reading::{Entry, MeetingJson, Reading};
use crate::{Context, Failure, HELP, args, text};

const VIEWS: [&str; 6] = ["--summary", "--transcription", "--sheet", "--speakers", "--actions", "--passages"];

pub(crate) fn run(cx: &Context, args: &[String]) -> Result<String, Failure> {
    let a = args::parse(args, &["--json", "--numbers", "--summary", "--transcription", "--sheet", "--speakers", "--actions", "--passages"], &["--from", "--to"])?;
    if a.help() {
        return Ok(HELP.into());
    }
    let views: Vec<&str> = VIEWS.into_iter().filter(|v| a.flag(v)).collect();
    if views.len() > 1 {
        return Err(Failure::Usage(format!("one view at a time: {}", views.join(" or "))));
    }
    let view = views.first().copied();
    let windowed = matches!(view, Some("--transcription" | "--passages"));
    if !windowed && (a.value("--from").is_some() || a.value("--to").is_some()) {
        return Err(Failure::Usage("--from and --to go with --transcription or --passages".into()));
    }
    if view != Some("--transcription") && a.flag("--numbers") {
        return Err(Failure::Usage("--numbers goes with --transcription".into()));
    }
    let instant = |option: &str| -> Result<Option<f64>, Failure> {
        a.value(option).map(|t| text::instant(t).ok_or_else(|| Failure::Usage(format!("invalid instant: \"{t}\" (h:mm:ss, m:ss or seconds)")))).transpose()
    };
    let window = Window { from: instant("--from")?, to: instant("--to")? };
    let Some(query) = a.positionals.first() else {
        return Err(Failure::Usage("missing the meeting".into()));
    };
    a.at_most(1)?;
    let reading = Reading::open(cx)?;
    let e = reading.find(query)?;
    let json = a.flag("--json");
    match view {
        None => default(&reading, &e, json),
        Some("--summary") => summary(&e, json),
        Some("--sheet") => sheet(&reading, &e, json),
        Some("--transcription") => transcription(&e, window, a.flag("--numbers"), json),
        Some("--speakers") => speakers(&e, json),
        Some("--actions") => actions(&e, json),
        _ => passages(&e, window, json),
    }
}

/// The part of the meeting `--from` and `--to` keep: what overlaps it.
#[derive(Clone, Copy)]
struct Window {
    from: Option<f64>,
    to: Option<f64>,
}

impl Window {
    fn keeps(self, start: f64, end: f64) -> bool {
        self.from.is_none_or(|f| end > f) && self.to.is_none_or(|t| start < t)
    }
}

/// The summary without its front matter, or `None` if there is none yet.
fn summary_text(m: &Meeting) -> Option<String> {
    if !m.files.contains(&Role::Summary) {
        return None;
    }
    let mut s = without_front_matter(&m.summary).trim_start_matches(['\n', '\r']).to_string();
    if !s.ends_with('\n') {
        s.push('\n');
    }
    Some(s)
}

fn no_summary(e: &Entry) -> Failure {
    Failure::Failed(format!("no summary yet for {} {}", e.identifier, text::completed_stage(&e.meeting)))
}

fn no_transcription(e: &Entry) -> Failure {
    Failure::Failed(format!("no transcription yet for {}", e.identifier))
}

#[derive(Serialize)]
struct ShowJson {
    #[serde(flatten)]
    meeting: MeetingJson,
    speakers: Vec<SpeakerJson>,
    /// The summary's outline; each `null` until there is one.
    brief: Option<String>,
    topics: Option<Vec<MomentJson>>,
    decisions: Option<Vec<MomentJson>>,
    /// The number of actions: the summary's checkboxes, as `--actions` numbers them.
    actions: Option<usize>,
}

/// A topic (`title`) or a decision (`text`) of the summary, at its moment.
#[derive(Serialize)]
struct MomentJson {
    /// In seconds.
    start: f64,
    timestamp: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
}

/// What the default view shows of the summary: the brief, the topics, the decisions and how
/// many actions; the rest is `--summary`'s, so that a long meeting stays a few lines.
struct Outline {
    brief: String,
    topics: Vec<Topic>,
    decisions: Vec<Decision>,
    /// The summary's checkboxes, as `--actions` and `check-action` number them.
    actions: usize,
}

impl Outline {
    fn of(m: &Meeting) -> Option<Outline> {
        let md = summary_text(m)?;
        Some(Outline { brief: summary::brief(&md), topics: summary::topics(&md), decisions: summary::decisions(&md), actions: summary::checkbox_count(&m.summary) })
    }

    fn text(&self) -> String {
        let mut s = String::new();
        if !self.brief.is_empty() {
            s += &format!("Brief: {}\n", self.brief);
        }
        if !self.topics.is_empty() {
            s += "Topics:\n";
            s += &self.topics.iter().map(|t| format!("  [{}] {}\n", playback_timestamp(t.start), t.title)).collect::<String>();
        }
        match self.decisions.as_slice() {
            [] => s += "Decisions: none\n",
            d => {
                s += "Decisions:\n";
                s += &d.iter().map(|d| format!("  [{}] {}\n", playback_timestamp(d.instant), d.text)).collect::<String>();
            }
        }
        s += &format!("Actions: {}\n", self.actions);
        s + "Next: --summary for the full summary, --transcription --from h:mm:ss --to h:mm:ss for what was said.\n"
    }
}

/// The header (identifier · title; date and time, duration, status; failure; speakers; folder),
/// then the summary's outline or where the meeting stands.
fn default(reading: &Reading, e: &Entry, json: bool) -> Result<String, Failure> {
    let m = &e.meeting;
    let outline = Outline::of(m);
    if json {
        let moment = |t: f64, title: Option<String>, text: Option<String>| MomentJson { start: t, timestamp: playback_timestamp(t), title, text };
        return Ok(crate::json(&ShowJson {
            meeting: MeetingJson::new(e, &reading.folder),
            speakers: speaker_list(m),
            brief: outline.as_ref().map(|o| o.brief.clone()),
            topics: outline.as_ref().map(|o| o.topics.iter().map(|t| moment(t.start, Some(t.title.clone()), None)).collect()),
            decisions: outline.as_ref().map(|o| o.decisions.iter().map(|d| moment(d.instant, None, Some(d.text.clone()))).collect()),
            actions: outline.as_ref().map(|o| o.actions),
        }));
    }
    let mut s = format!("{} · {}\n{} · {} · {}\n", e.identifier, m.title, text::start(m.start), playback_timestamp(m.duration), text::status(m, e.live.as_ref()));
    if let greffier_core::Status::Failed { message, .. } = &m.status {
        s += &format!("Error: {message}\n");
    }
    let shown: Vec<(String, &str)> = m.active_speakers().map(|i| (m.speakers[i].display_name(), m.speakers[i].label.as_str())).collect();
    // A name two voices share comes with their labels.
    let names: Vec<String> = shown.iter().map(|(n, l)| if shown.iter().filter(|(o, _)| o == n).count() > 1 { format!("{n} ({l})") } else { n.clone() }).collect();
    if !names.is_empty() {
        s += &format!("Speakers: {}\n", names.join(", "));
    }
    s += &format!("Folder: {}\n", reading.folder.folder(&m.folder_name).display());
    match outline {
        Some(outline) => s += &outline.text(),
        None => {
            s += &format!("No summary yet {}.\n", text::completed_stage(m));
            if !m.utterances.is_empty() {
                s += "Next: --transcription [--from h:mm:ss --to h:mm:ss] for what was said.\n";
            }
        }
    }
    Ok(s)
}

fn summary(e: &Entry, json: bool) -> Result<String, Failure> {
    let s = summary_text(&e.meeting).ok_or_else(|| no_summary(e))?;
    if json {
        return Ok(crate::json(&serde_json::json!({ "identifier": e.identifier, "summary": s })));
    }
    Ok(s)
}

fn sheet(reading: &Reading, e: &Entry, json: bool) -> Result<String, Failure> {
    let missing = || Failure::Failed(format!("no sheet for {}", e.identifier));
    if !e.meeting.files.contains(&Role::Sheet) {
        return Err(missing());
    }
    let s = std::fs::read_to_string(reading.folder.file(&e.meeting.folder_name, Role::Sheet)).map_err(|_| missing())?;
    if json {
        return Ok(crate::json(&serde_json::json!({ "identifier": e.identifier, "sheet": s })));
    }
    Ok(s)
}

#[derive(Serialize)]
struct UtteranceJson {
    number: usize,
    /// In seconds.
    start: f64,
    end: f64,
    /// `0:44:12`
    timestamp: String,
    /// The displayed name (confirmed, otherwise proposed "Khalid ?", otherwise the label).
    speaker: String,
    /// The displayed speaker's label (merges followed).
    label: String,
    text: String,
}

fn transcription(e: &Entry, window: Window, numbered: bool, json: bool) -> Result<String, Failure> {
    let m = &e.meeting;
    if m.utterances.is_empty() {
        return Err(no_transcription(e));
    }
    let kept: Vec<UtteranceJson> = m
        .utterances
        .iter()
        .enumerate()
        .filter(|(_, u)| window.keeps(u.start, u.end))
        .map(|(i, u)| {
            let speaker = &m.speakers[m.displayed_speaker(u.speaker)];
            UtteranceJson { number: i + 1, start: u.start, end: u.end, timestamp: playback_timestamp(u.start), speaker: speaker.display_name(), label: speaker.label.clone(), text: u.text.clone() }
        })
        .collect();
    if json {
        return Ok(crate::json(&serde_json::json!({ "identifier": e.identifier, "utterances": kept })));
    }
    if kept.is_empty() {
        return Ok("No utterances in this range.\n".into());
    }
    Ok(kept
        .iter()
        .map(|u| {
            let number = if numbered { format!("#{} ", u.number) } else { String::new() };
            format!("{number}[{}] {}: {}\n", u.timestamp, u.speaker, u.text)
        })
        .collect())
}

#[derive(Serialize)]
struct SpeakerJson {
    /// Its index, as in its label.
    index: usize,
    label: String,
    /// The name confirmed by the user (merges followed).
    name: Option<String>,
    /// The name the summary proposes, not confirmed.
    proposal: Option<String>,
    /// What the UI displays: "Alexandre", "Khalid ?", "SPEAKER_02".
    displayed: String,
    /// The label of the speaker it is merged into.
    merged_into: Option<String>,
    /// Seconds of speech (0 for a merged speaker: counted with the one replacing it).
    speaking_time: u64,
}

/// Seconds of speech of a displayed speaker, merged ones included.
fn speech(m: &Meeting, i: usize) -> f64 {
    m.utterances.iter().filter(|u| m.displayed_speaker(u.speaker) == i).map(|u| u.end - u.start).sum()
}

fn speaker_list(m: &Meeting) -> Vec<SpeakerJson> {
    (0..m.speakers.len())
        .map(|i| {
            let shown = &m.speakers[m.displayed_speaker(i)];
            let merged = m.speakers[i].merged_into.map(|j| m.speakers[j].label.clone());
            SpeakerJson {
                index: i,
                label: m.speakers[i].label.clone(),
                name: shown.name.clone(),
                proposal: shown.proposal.clone().filter(|_| shown.name.is_none()),
                displayed: shown.display_name(),
                speaking_time: if merged.is_some() { 0 } else { speech(m, i).round() as u64 },
                merged_into: merged,
            }
        })
        .collect()
}

fn speakers(e: &Entry, json: bool) -> Result<String, Failure> {
    let m = &e.meeting;
    if m.speakers.is_empty() {
        return Err(no_transcription(e));
    }
    let list = speaker_list(m);
    if json {
        return Ok(crate::json(&serde_json::json!({ "identifier": e.identifier, "speakers": list })));
    }
    let total: f64 = m.utterances.iter().map(|u| u.end - u.start).sum();
    Ok(list
        .iter()
        .map(|l| match &l.merged_into {
            Some(into) => format!("{} · {} · {} · merged into {into}\n", l.index, l.label, l.displayed),
            None => {
                let own = speech(m, l.index);
                let share = if total > 0.0 { own / total * 100.0 } else { 0.0 };
                format!("{} · {} · {} · {} · {share:.0} %\n", l.index, l.label, l.displayed, playback_timestamp(own))
            }
        })
        .collect())
}

fn actions(e: &Entry, json: bool) -> Result<String, Failure> {
    if !e.meeting.files.contains(&Role::Summary) {
        return Err(no_summary(e));
    }
    let boxes = checkboxes(&e.meeting.summary);
    if json {
        let list: Vec<_> = boxes.iter().enumerate().map(|(i, (done, t))| serde_json::json!({ "number": i + 1, "done": done, "text": t })).collect();
        return Ok(crate::json(&serde_json::json!({ "identifier": e.identifier, "actions": list })));
    }
    if boxes.is_empty() {
        return Ok("No actions.\n".into());
    }
    Ok(boxes.iter().enumerate().map(|(i, (done, t))| format!("#{} [{}] {t}\n", i + 1, if *done { "x" } else { " " })).collect())
}

#[derive(Serialize)]
struct PassageJson {
    number: usize,
    /// The number of its utterance (as in `--transcription --numbers`).
    utterance: usize,
    /// Its utterance's start, in seconds.
    start: f64,
    timestamp: String,
    excerpt: String,
    hypothesis: Option<String>,
    /// What the relisten heard.
    relisten: String,
    /// `to_check`, `hypothesis_accepted`, `original_kept` or `corrected`.
    status: &'static str,
    /// The user's correction, if `corrected`.
    correction: Option<String>,
}

fn passages(e: &Entry, window: Window, json: bool) -> Result<String, Failure> {
    let m = &e.meeting;
    if m.utterances.is_empty() {
        return Err(no_transcription(e));
    }
    let kept: Vec<PassageJson> = m
        .passages
        .iter()
        .enumerate()
        .filter_map(|(i, p)| {
            let u = m.utterances.get(p.utterance)?;
            window.keeps(u.start, u.end).then(|| {
                let (status, correction) = match &p.status {
                    PassageStatus::Pending => ("to_check", None),
                    PassageStatus::HypothesisAccepted => ("hypothesis_accepted", None),
                    PassageStatus::OriginalKept => ("original_kept", None),
                    PassageStatus::CorrectedManually(t) => ("corrected", Some(t.clone())),
                };
                PassageJson {
                    number: i + 1,
                    utterance: p.utterance + 1,
                    start: u.start,
                    timestamp: playback_timestamp(u.start),
                    excerpt: p.excerpt.clone(),
                    hypothesis: p.hypothesis.clone(),
                    relisten: p.relisten.clone(),
                    status,
                    correction,
                }
            })
        })
        .collect();
    if json {
        return Ok(crate::json(&serde_json::json!({ "identifier": e.identifier, "passages": kept })));
    }
    if kept.is_empty() {
        return Ok(if m.passages.is_empty() { "No passages to check.\n" } else { "No passages to check in this range.\n" }.into());
    }
    Ok(kept
        .iter()
        .map(|p| {
            let what = match (p.status, &p.hypothesis, &p.correction) {
                ("to_check", Some(h), _) => format!("to check: \"{}\" (hypothesis: \"{h}\")", p.excerpt),
                ("to_check", None, _) => format!("to check: \"{}\"", p.excerpt),
                ("hypothesis_accepted", h, _) => format!("hypothesis accepted: \"{}\" → \"{}\"", p.excerpt, h.as_deref().unwrap_or(&p.excerpt)),
                ("corrected", _, Some(c)) => format!("corrected: \"{}\" → \"{c}\"", p.excerpt),
                _ => format!("kept as is: \"{}\"", p.excerpt),
            };
            format!("#{} [{}] {what} · utterance #{}\n", p.number, p.timestamp, p.utterance)
        })
        .collect())
}
