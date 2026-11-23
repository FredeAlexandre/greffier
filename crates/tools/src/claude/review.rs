//! The review by the `claude` CLI, in two passes within one session (ADR 0002):
//!
//! 1. the agent receives the numbered utterances (`#N [h:mm:ss → h:mm:ss] SPEAKER_xx : text`, on
//!    standard input), the lexicon and the context; it returns the confident corrections, the
//!    proven speaker changes, the doubtful passages (30 at most) and the lexicon additions;
//! 2. **relisten**: each doubtful passage is cut from the recording (±1.5 s around its
//!    utterance, 28 s at most) and transcribed again with the vocabulary;
//! 3. in the same session, the agent decides each passage: corrected, or to check.
//!
//! The agent touches no file (read-only tools): this module turns its answers into a
//! [`ReviewResult`], which the core applies itself. The prompt ships with the app
//! (`resources/prompt-review.md`); its "Second pass" part is sent after the relisten.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::Deserialize;
use serde_json::{Value, json};

use super::{ClaudeCall, Claude, Session, new_session};
use greffier_core::adapters::{LexiconAddition, SpeakerChange, ConfidentCorrection, ReviewRequest, CorrectedPassage, FlaggedPassage, AgentUtterance, ReviewResult};
use greffier_core::playback_timestamp;

/// The review prompt shipped with the app.
pub const PROMPT: &str = include_str!("../../resources/prompt-review.md");

/// The heading that opens the second part of the prompt.
const SECOND_PASS: &str = "## Second pass";

/// At most this many doubtful passages are relistened.
pub const MAX_PASSAGES: usize = 30;

/// The progress milestones: first pass, relisten, second pass.
pub const MILESTONES: [f64; 3] = [0.0, 0.6, 0.75];

const ALLOWED_TOOLS: [&str; 3] = ["Read", "Glob", "Grep"];
const FORBIDDEN_TOOLS: [&str; 6] = ["Write", "Edit", "Bash", "WebFetch", "WebSearch", "Task"];

/// Transcribes excerpts of the recording (start, end in seconds) again with the transcription
/// settings; one text per excerpt, empty if there is nothing usable.
pub trait Relisten {
    fn relisten(&self, recording: &Path, excerpts: &[(f64, f64)], vocabulary: &str, stop: &AtomicBool) -> Result<Vec<String>, String>;
}

// --- The agent's answers --------------------------------------------------------------------

#[derive(Deserialize)]
struct Correction {
    /// The utterance number (1 = the first one sent), `line` in the answer.
    #[serde(rename = "line")]
    utterance: usize,
    before: String,
    after: String,
}

#[derive(Deserialize)]
struct Speaker {
    /// The utterance number (1 = the first one sent), `line` in the answer.
    #[serde(rename = "line")]
    utterance: usize,
    speaker: String,
}

#[derive(Clone, Deserialize)]
struct PassageToRelisten {
    /// The utterance number (1 = the first one sent), `line` in the answer.
    #[serde(rename = "line")]
    utterance: usize,
    excerpt: String,
    #[serde(default)]
    hypothesis: String,
}

#[derive(Deserialize)]
struct IndexEntry {
    heard: String,
    correct: String,
}

#[derive(Deserialize)]
struct FirstPass {
    #[serde(default)]
    corrections: Vec<Correction>,
    #[serde(default)]
    speakers: Vec<Speaker>,
    #[serde(default, rename = "doubts")]
    passages: Vec<PassageToRelisten>,
    #[serde(default)]
    lexicon: Vec<IndexEntry>,
}

#[derive(Deserialize)]
struct Decision {
    /// The utterance number (1 = the first one sent), `line` in the answer.
    #[serde(rename = "line")]
    utterance: usize,
    #[serde(default)]
    excerpt: String,
    status: String,
    #[serde(default)]
    after: String,
}

#[derive(Deserialize)]
struct SecondPass {
    #[serde(default)]
    decisions: Vec<Decision>,
    #[serde(default)]
    lexicon: Vec<IndexEntry>,
}

fn list(properties: Value) -> Value {
    let required: Vec<&String> = properties.as_object().map(|o| o.keys().collect()).unwrap_or_default();
    json!({ "type": "array", "items": { "type": "object", "properties": properties, "required": required, "additionalProperties": false } })
}

fn first_pass_schema() -> String {
    let (text, integer) = (json!({ "type": "string" }), json!({ "type": "integer" }));
    json!({
        "type": "object",
        "properties": {
            "corrections": list(json!({ "line": integer, "before": text, "after": text, "reason": text })),
            "speakers": list(json!({ "line": integer, "speaker": text, "reason": text })),
            "doubts": list(json!({ "line": integer, "excerpt": text, "hypothesis": text, "reason": text })),
            "lexicon": list(json!({ "heard": text, "correct": text })),
        },
        "required": ["corrections", "speakers", "doubts", "lexicon"],
        "additionalProperties": false,
    })
    .to_string()
}

fn second_pass_schema() -> String {
    let (text, integer) = (json!({ "type": "string" }), json!({ "type": "integer" }));
    json!({
        "type": "object",
        "properties": {
            "decisions": list(json!({ "line": integer, "excerpt": text, "status": { "type": "string", "enum": ["corrected", "to_check"] }, "after": text, "reason": text })),
            "lexicon": list(json!({ "heard": text, "correct": text })),
        },
        "required": ["decisions", "lexicon"],
        "additionalProperties": false,
    })
    .to_string()
}

fn read<T: for<'a> Deserialize<'a>>(v: Value) -> Result<T, String> {
    serde_json::from_value(v).map_err(|e| format!("Claude a rendu une réponse illisible : {e}"))
}

// --- The prompt ----------------------------------------------------------------------------

/// The lexicon as given to the agent: without front matter or main heading.
fn lexicon_for_agent(md: &str) -> String {
    let mut body = md;
    if let Some(rest) = body.strip_prefix("---\n")
        && let Some(end) = rest.find("\n---\n")
    {
        body = &rest[end + 5..];
    }
    let body: Vec<&str> = body.lines().collect();
    let without_title = match body.iter().position(|l| l.starts_with("# ")) {
        Some(i) => [&body[..i], &body[i + 1..]].concat(),
        None => body,
    };
    let text = without_title.join("\n");
    if text.trim().is_empty() { "(empty)".into() } else { text.trim().to_string() }
}

/// The two parts of the prompt, with the `{{…}}` replaced.
fn prompts(d: &ReviewRequest) -> (String, String) {
    let body = match PROMPT.strip_prefix("---\n").and_then(|r| r.find("\n---\n").map(|i| &r[i + 5..])) {
        Some(c) => c,
        None => PROMPT,
    };
    let (first, second) = body.split_at(body.find(SECOND_PASS).unwrap_or(body.len()));
    let mut people = d.speaker_count.map(|n| format!("{n} people in the meeting.")).unwrap_or_default();
    let mut names: Vec<String> = d.utterances.iter().filter_map(|r| r.name.as_ref().map(|n| format!("{} = {n}", r.speaker))).collect();
    names.sort();
    names.dedup();
    if !names.is_empty() {
        people = format!("{people} Names already given: {}.", names.join(", ")).trim().to_string();
    }
    let date = format!("{:04}-{:02}-{:02}", d.start.year, d.start.month, d.start.day);
    let fill = |t: &str| {
        t.replace("{{DATE}}", &date)
            .replace("{{TITLE}}", &d.title)
            .replace("{{FOLDER}}", &d.folder_name)
            .replace("{{PEOPLE}}", &people)
            .replace("{{LEXICON}}", &lexicon_for_agent(&d.lexicon))
            .trim()
            .to_string()
    };
    (fill(first), fill(second))
}

/// `#N [h:mm:ss → h:mm:ss] SPEAKER_xx : text`, numbered from 1.
fn numbered_utterances(utterances: &[AgentUtterance]) -> String {
    let lines: Vec<String> = utterances.iter().enumerate().map(|(i, r)| format!("#{} [{} → {}] {} : {}", i + 1, playback_timestamp(r.start), playback_timestamp(r.end), r.speaker, r.text)).collect();
    lines.join("\n")
}

/// A hypothesis that is not one (empty, "?", "? (maybe…)") does not count.
fn hypothesis(h: &str) -> Option<String> {
    let h = h.trim();
    (!h.is_empty() && !h.starts_with('?')).then(|| h.to_string())
}

// --- The review ----------------------------------------------------------------------------

/// The two passes and the relisten, on the calling thread. `progress` receives the milestone
/// reached ([`MILESTONES`]). A failed relisten does not stop the review: the agent decides without it.
pub fn review(claude: &dyn Claude, relisten: &dyn Relisten, d: &ReviewRequest, stop: &AtomicBool, progress: &mut dyn FnMut(f64)) -> Result<ReviewResult, String> {
    let stopped = || if stop.load(Ordering::Relaxed) { Err("arrêté".to_string()) } else { Ok(()) };
    let session = new_session();
    let (first, second) = prompts(d);
    let call = |prompt: String, input: String, schema: String, session: Session| ClaudeCall {
        prompt,
        input,
        schema,
        session,
        directory: d.meetings_folder.clone(),
        allowed_tools: ALLOWED_TOOLS.map(String::from).to_vec(),
        forbidden_tools: FORBIDDEN_TOOLS.map(String::from).to_vec(),
        can_write: false,
        extra_folders: Vec::new(),
    };
    progress(MILESTONES[0]);
    let p1: FirstPass = read(claude.ask(&call(first, numbered_utterances(&d.utterances), first_pass_schema(), Session::New(session.clone())), stop)?)?;
    stopped()?;

    // The agent's utterance N (its `line`) is the N-th utterance sent.
    let sent = |number: usize| number.checked_sub(1).and_then(|i| d.utterances.get(i));
    let mut result = ReviewResult { session: Some(session.clone()), ..Default::default() };
    result.corrections = p1.corrections.into_iter().filter_map(|c| Some(ConfidentCorrection { utterance: sent(c.utterance)?.number, before: c.before, after: c.after })).collect();
    result.speaker_changes = p1.speakers.into_iter().filter_map(|c| Some(SpeakerChange { utterance: sent(c.utterance)?.number, speaker: c.speaker })).collect();
    result.lexicon_additions = p1.lexicon.into_iter().map(|e| LexiconAddition { heard: e.heard, correct: e.correct }).collect();

    let mut passages: Vec<PassageToRelisten> = Vec::new();
    for x in p1.passages {
        let seen = passages.iter().any(|y| y.utterance == x.utterance && y.excerpt == x.excerpt);
        if sent(x.utterance).is_some() && !x.excerpt.trim().is_empty() && !seen && passages.len() < MAX_PASSAGES {
            passages.push(x);
        }
    }
    if passages.is_empty() {
        progress(1.0);
        return Ok(result);
    }

    progress(MILESTONES[1]);
    let excerpts: Vec<(f64, f64)> = passages.iter().map(|x| {
        let r = sent(x.utterance).expect("utterance checked above");
        greffier_core::excerpt_to_relisten(r.start, r.end, &r.text, &x.excerpt)
    }).collect();
    let (relistened, for_agent): (Vec<String>, Vec<String>) = match relisten.relisten(&d.recording, &excerpts, &d.vocabulary, stop) {
        Ok(texts) => {
            let texts: Vec<String> = (0..passages.len()).map(|i| texts.get(i).map(|t| greffier_core::attribution::text_without_noises(t)).unwrap_or_default()).collect();
            let readable_texts = texts.iter().map(|t| if t.is_empty() { "(nothing usable)".to_string() } else { t.clone() }).collect();
            (texts, readable_texts)
        }
        Err(e) => (vec![String::new(); passages.len()], vec![format!("(relisten failed: {e})"); passages.len()]),
    };
    stopped()?;

    progress(MILESTONES[2]);
    let listing: Vec<String> = passages
        .iter()
        .zip(&for_agent)
        .map(|(x, heard)| {
            let h = if x.hypothesis.trim().is_empty() { "?" } else { x.hypothesis.trim() };
            let start = sent(x.utterance).map_or(0.0, |r| r.start);
            format!("- line #{} [{}] · excerpt: \"{}\" · your hypothesis: \"{h}\"\n  relisten: \"{heard}\"", x.utterance, playback_timestamp(start), x.excerpt)
        })
        .collect();
    let prompt = format!("{second}\n\n{}", listing.join("\n"));
    let p2: SecondPass = read(claude.ask(&call(prompt, String::new(), second_pass_schema(), Session::Resume(session)), stop)?)?;

    for (x, relisten) in passages.into_iter().zip(relistened) {
        let number = sent(x.utterance).expect("utterance checked above").number;
        let decision = p2.decisions.iter().find(|y| y.utterance == x.utterance && y.excerpt == x.excerpt).or_else(|| p2.decisions.iter().find(|y| y.utterance == x.utterance));
        match decision {
            Some(y) if y.status == "corrected" && hypothesis(&y.after).is_some() => {
                // Corrected to itself: the excerpt was right, nothing to change.
                if y.after.trim() != x.excerpt.trim() {
                    result.corrected_passages.push(CorrectedPassage { utterance: number, excerpt: x.excerpt, after: y.after.trim().to_string(), relisten });
                }
            }
            _ => {
                let h = decision.and_then(|y| hypothesis(&y.after)).or_else(|| hypothesis(&x.hypothesis));
                result.passages_to_check.push(FlaggedPassage { utterance: number, excerpt: x.excerpt, hypothesis: h, relisten });
            }
        }
    }
    result.lexicon_additions.extend(p2.lexicon.into_iter().map(|e| LexiconAddition { heard: e.heard, correct: e.correct }));
    progress(1.0);
    Ok(result)
}
