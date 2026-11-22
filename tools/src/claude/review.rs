//! La relecture par la CLI `claude`, en deux passes dans une même session (ADR 0002) :
//!
//! 1. l'agent reçoit les répliques numérotées (`#N [h:mm:ss → h:mm:ss] SPEAKER_xx : texte`, sur
//!    l'entrée standard), le lexique et le contexte ; il rend les corrections sûres, les
//!    changements de locuteur prouvés, les passages douteux (30 au plus) et les ajouts au lexique ;
//! 2. **réécoute** : chaque passage douteux est extrait de l'enregistrement (±1,5 s autour de sa
//!    réplique, 28 s au plus) et retranscrit avec le vocabulaire ;
//! 3. dans la même session, l'agent tranche chaque passage : corrigé, ou à vérifier.
//!
//! L'agent ne touche à aucun fichier (outils en lecture seule) : ce module traduit ses réponses en
//! [`ResultatDeRelecture`], que le cœur applique lui-même. Le prompt est livré avec l'app
//! (`ressources/prompt-review.md`) ; sa partie « Deuxième passe » est envoyée après la réécoute.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::Deserialize;
use serde_json::{Value, json};

use super::{ClaudeCall, Claude, Session, new_session};
use greffier_core::adapters::{LexiconAddition, SpeakerChange, ConfidentCorrection, ReviewRequest, CorrectedPassage, FlaggedPassage, AgentUtterance, ReviewResult};
use greffier_core::playback_timestamp;

/// Le prompt de la relecture livré avec l'app.
pub const PROMPT: &str = include_str!("../../resources/prompt-review.md");

/// Le titre qui ouvre la deuxième partie du prompt.
const SECOND_PASS: &str = "## Deuxième passe";

/// Au plus autant de passages douteux réécoutés.
pub const MAX_PASSAGES: usize = 30;

/// Les paliers d'avancement : première passe, réécoute, deuxième passe.
pub const MILESTONES: [f64; 3] = [0.0, 0.6, 0.75];

const ALLOWED_TOOLS: [&str; 3] = ["Read", "Glob", "Grep"];
const FORBIDDEN_TOOLS: [&str; 6] = ["Write", "Edit", "Bash", "WebFetch", "WebSearch", "Task"];

/// Retranscrit des extraits de l'enregistrement (début, fin en secondes) avec les réglages de la
/// transcription ; un texte par extrait, vide s'il n'y a rien d'exploitable.
pub trait Relisten {
    fn relisten(&self, recording: &Path, excerpts: &[(f64, f64)], vocabulary: &str, stop: &AtomicBool) -> Result<Vec<String>, String>;
}

// --- Les réponses de l'agent -----------------------------------------------------------------

#[derive(Deserialize)]
struct Correction {
    /// Le numéro de la réplique (1 = la première envoyée), « ligne » dans la réponse.
    #[serde(rename = "line")]
    utterance: usize,
    before: String,
    after: String,
}

#[derive(Deserialize)]
struct Speaker {
    /// Le numéro de la réplique (1 = la première envoyée), « ligne » dans la réponse.
    #[serde(rename = "line")]
    utterance: usize,
    speaker: String,
}

#[derive(Clone, Deserialize)]
struct PassageToRelisten {
    /// Le numéro de la réplique (1 = la première envoyée), « ligne » dans la réponse.
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
    /// Le numéro de la réplique (1 = la première envoyée), « ligne » dans la réponse.
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
        "required": ["corrections", "locuteurs", "doutes", "lexique"],
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
        "required": ["decisions", "lexique"],
        "additionalProperties": false,
    })
    .to_string()
}

fn read<T: for<'a> Deserialize<'a>>(v: Value) -> Result<T, String> {
    serde_json::from_value(v).map_err(|e| format!("Claude a rendu une réponse illisible : {e}"))
}

// --- Le prompt -----------------------------------------------------------------------------

/// Le lexique tel qu'on le donne à l'agent : sans frontmatter ni titre principal.
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
    if text.trim().is_empty() { "(vide)".into() } else { text.trim().to_string() }
}

/// Les deux parties du prompt, `{{…}}` remplacés.
fn prompts(d: &ReviewRequest) -> (String, String) {
    let body = match PROMPT.strip_prefix("---\n").and_then(|r| r.find("\n---\n").map(|i| &r[i + 5..])) {
        Some(c) => c,
        None => PROMPT,
    };
    let (first, second) = body.split_at(body.find(SECOND_PASS).unwrap_or(body.len()));
    let mut people = d.speaker_count.map(|n| format!("{n} personnes dans la réunion.")).unwrap_or_default();
    let mut names: Vec<String> = d.utterances.iter().filter_map(|r| r.name.as_ref().map(|n| format!("{} = {n}", r.speaker))).collect();
    names.sort();
    names.dedup();
    if !names.is_empty() {
        people = format!("{people} Noms déjà donnés : {}.", names.join(", ")).trim().to_string();
    }
    let date = format!("{:04}-{:02}-{:02}", d.start.year, d.start.month, d.start.day);
    let fill = |t: &str| {
        t.replace("{{DATE}}", &date)
            .replace("{{TITRE}}", &d.title)
            .replace("{{DOSSIER}}", &d.folder_name)
            .replace("{{PERSONNES}}", &people)
            .replace("{{LEXIQUE}}", &lexicon_for_agent(&d.lexicon))
            .trim()
            .to_string()
    };
    (fill(first), fill(second))
}

/// `#N [h:mm:ss → h:mm:ss] SPEAKER_xx : texte`, numérotées à partir de 1.
fn numbered_utterances(utterances: &[AgentUtterance]) -> String {
    let lines: Vec<String> = utterances.iter().enumerate().map(|(i, r)| format!("#{} [{} → {}] {} : {}", i + 1, playback_timestamp(r.start), playback_timestamp(r.end), r.speaker, r.text)).collect();
    lines.join("\n")
}

/// Une hypothèse qui n'en est pas une (vide, « ? », « ? (peut-être…) ») ne compte pas.
fn hypothesis(h: &str) -> Option<String> {
    let h = h.trim();
    (!h.is_empty() && !h.starts_with('?')).then(|| h.to_string())
}

// --- La relecture --------------------------------------------------------------------------

/// Les deux passes et la réécoute, sur le fil appelant. `progres` reçoit le palier atteint
/// ([`PALIERS`]). Un échec de la réécoute n'arrête pas la relecture : l'agent tranche sans elle.
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

    // La réplique N de l'agent (sa « ligne ») est la N-ième réplique envoyée.
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
        let r = sent(x.utterance).expect("réplique vérifiée");
        greffier_core::excerpt_to_relisten(r.start, r.end, &r.text, &x.excerpt)
    }).collect();
    let (relistened, for_agent): (Vec<String>, Vec<String>) = match relisten.relisten(&d.recording, &excerpts, &d.vocabulary, stop) {
        Ok(texts) => {
            let texts: Vec<String> = (0..passages.len()).map(|i| texts.get(i).map(|t| greffier_core::attribution::text_without_noises(t)).unwrap_or_default()).collect();
            let readable_texts = texts.iter().map(|t| if t.is_empty() { "(rien d'exploitable)".to_string() } else { t.clone() }).collect();
            (texts, readable_texts)
        }
        Err(e) => (vec![String::new(); passages.len()], vec![format!("(réécoute impossible : {e})"); passages.len()]),
    };
    stopped()?;

    progress(MILESTONES[2]);
    let listing: Vec<String> = passages
        .iter()
        .zip(&for_agent)
        .map(|(x, heard)| {
            let h = if x.hypothesis.trim().is_empty() { "?" } else { x.hypothesis.trim() };
            let start = sent(x.utterance).map_or(0.0, |r| r.start);
            format!("- ligne #{} [{}] · extrait : « {} » · ton hypothèse : « {h} »\n  réécoute : « {heard} »", x.utterance, playback_timestamp(start), x.excerpt)
        })
        .collect();
    let prompt = format!("{second}\n\n{}", listing.join("\n"));
    let p2: SecondPass = read(claude.ask(&call(prompt, String::new(), second_pass_schema(), Session::Resume(session)), stop)?)?;

    for (x, relisten) in passages.into_iter().zip(relistened) {
        let number = sent(x.utterance).expect("réplique vérifiée").number;
        let decision = p2.decisions.iter().find(|y| y.utterance == x.utterance && y.excerpt == x.excerpt).or_else(|| p2.decisions.iter().find(|y| y.utterance == x.utterance));
        match decision {
            Some(y) if y.status == "corrected" && hypothesis(&y.after).is_some() => {
                // Corrigé en lui-même : l'extrait était juste, rien à changer.
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
