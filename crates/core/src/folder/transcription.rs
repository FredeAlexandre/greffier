//! The structured transcription (`… - transcription.json`) and the readable transcription
//! (`… - transcription.md`).
//!
//! The JSON keeps everything processing produced: the words (timestamps, confidence, speaker),
//! the speaker turns, the utterances, the raw transcription from before review, the review log,
//! the passages to check and the manual corrections. Speakers are always referred to by their
//! label ("SPEAKER_00"), even when named or merged; `names` and `merges` give the mapping, the
//! same as in the sheet (which is authoritative when it exists).
//!
//! ```json
//! {
//!   "version": 1,
//!   "speakers": ["SPEAKER_00", "SPEAKER_01", "SPEAKER_02"],
//!   "names": { "SPEAKER_00": "Alexandre", "SPEAKER_01": "Khalid", "SPEAKER_02": "Alexandre" },
//!   "merges": { "SPEAKER_02": "SPEAKER_00" },
//!   "words": [{ "start": 0.0, "end": 0.25, "text": "Bonjour", "confidence": 0.9, "speaker": "SPEAKER_00" }],
//!   "turns": [{ "start": 0.0, "end": 2.0, "speaker": "SPEAKER_00" }],
//!   "utterances": [{ "speaker": "SPEAKER_00", "start": 0.0, "end": 2.0, "text": "Bonjour Khalid." }],
//!   "raw": [ …the utterances from before the review, absent if it has not been done… ],
//!   "review": { "corrections": [{ "utterance": 1, "before": "recète", "after": "recette" }],
//!               "speaker_changes": [{ "utterance": 3, "speaker": "SPEAKER_01" }],
//!               "corrected_passages": [{ "utterance": 4, "excerpt": "la prod", "after": "la preprod",
//!                                        "relisten": "on bascule la preprod" }],
//!               "lexicon_additions": [{ "heard": "recète", "correct": "recette" }] },
//!   "passages": [{ "utterance": 2, "excerpt": "le support", "hypothesis": "l'équipe support",
//!                  "relisten": "…", "status": "pending" }],
//!   "manual_corrections": [{ "utterance": 0, "before": "…", "after": "…" }]
//! }
//! ```
//!
//! A passage's `status`: `pending`, `hypothesis_accepted`, `original_kept` or
//! `corrected_manually` (with `correction`). `review` absent: the review has not been done
//! (an imported transcription is treated as not reviewed).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::adapters::{LexiconAddition, SpeakerChange, ConfidentCorrection, CorrectedPassage};
use crate::meeting::{ManualCorrection, ReviewLog, Speaker, Word, PassageToCheck, Utterance, Meeting, PassageStatus, HUES, SpeakerTurn};
use crate::time::playback_timestamp;

pub const VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StructuredTranscription {
    pub version: u32,
    /// The speakers' labels, in order (their colors depend on it).
    #[serde(default)]
    pub speakers: Vec<String>,
    /// SPEAKER_xx → name given by the user, merges included.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub names: BTreeMap<String, String>,
    /// SPEAKER_xx → SPEAKER_yy: the first is the same voice as the second, which replaces it.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub merges: BTreeMap<String, String>,
    /// SPEAKER_xx → its own name, which `names` hides while it is merged; given back at split.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub names_before_merge: BTreeMap<String, String>,
    /// SPEAKER_xx → SPEAKER_yy: the first bears the name the second lent it at merge, as it had
    /// none; the name goes back at split.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub lent_names: BTreeMap<String, String>,
    #[serde(default)]
    pub words: Vec<StoredWord>,
    #[serde(default)]
    pub turns: Vec<StoredTurn>,
    #[serde(default)]
    pub utterances: Vec<StoredUtterance>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub raw: Vec<StoredUtterance>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review: Option<StoredReview>,
    #[serde(default)]
    pub passages: Vec<StoredPassage>,
    #[serde(default)]
    pub manual_corrections: Vec<StoredCorrection>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StoredWord {
    pub start: f64,
    pub end: f64,
    pub text: String,
    #[serde(default = "full_confidence")]
    pub confidence: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speaker: Option<String>,
}

fn full_confidence() -> f32 {
    1.0
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StoredTurn {
    pub start: f64,
    pub end: f64,
    pub speaker: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StoredUtterance {
    pub speaker: String,
    pub start: f64,
    pub end: f64,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StoredCorrection {
    pub utterance: usize,
    pub before: String,
    pub after: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StoredSpeakerChange {
    pub utterance: usize,
    pub speaker: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StoredReview {
    #[serde(default)]
    pub corrections: Vec<StoredCorrection>,
    #[serde(default)]
    pub speaker_changes: Vec<StoredSpeakerChange>,
    /// The doubtful passages corrected after the relisten.
    #[serde(default)]
    pub corrected_passages: Vec<StoredCorrectedPassage>,
    /// The errors written into the lexicon.
    #[serde(default)]
    pub lexicon_additions: Vec<StoredLexiconAddition>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StoredCorrectedPassage {
    pub utterance: usize,
    pub excerpt: String,
    pub after: String,
    #[serde(default)]
    pub relisten: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StoredLexiconAddition {
    pub heard: String,
    pub correct: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StoredPassage {
    pub utterance: usize,
    pub excerpt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hypothesis: Option<String>,
    #[serde(default)]
    pub relisten: String,
    #[serde(default = "pending")]
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correction: Option<String>,
}

fn pending() -> String {
    "pending".into()
}

impl StructuredTranscription {
    pub fn read(json: &str) -> Result<StructuredTranscription, String> {
        serde_json::from_str(json).map_err(|e| format!("transcription structurée illisible : {e}"))
    }

    pub fn json(&self) -> String {
        let mut s = serde_json::to_string_pretty(self).unwrap_or_default();
        s.push('\n');
        s
    }

    pub fn from_meeting(r: &Meeting) -> StructuredTranscription {
        let label = |i: usize| r.speakers.get(i).map(|l| l.label.clone()).unwrap_or_else(|| "SPEAKER_00".into());
        let write = |x: &Utterance| StoredUtterance { speaker: label(x.speaker), start: x.start, end: x.end, text: x.text.clone() };
        // A word's speaker: the one attribution gave it, otherwise (words read back from a
        // transcription that had none) that of the utterance where it starts.
        let word_speaker = |m: &Word| match m.speaker {
            Some(i) => Some(label(i)),
            None => r.utterances.iter().find(|x| m.start >= x.start - 1e-6 && m.start <= x.end + 1e-6).map(|x| label(x.speaker)),
        };
        let correction = |c: &ConfidentCorrection| StoredCorrection { utterance: c.utterance, before: c.before.clone(), after: c.after.clone() };
        StructuredTranscription {
            version: VERSION,
            speakers: r.speakers.iter().map(|l| l.label.clone()).collect(),
            names: (0..r.speakers.len()).filter_map(|i| Some((label(i), r.speaker_name(i)?.to_string()))).collect(),
            merges: r.speakers.iter().filter_map(|l| Some((l.label.clone(), label(l.merged_into?)))).collect(),
            names_before_merge: r.merged_names().collect(),
            lent_names: r.lent_names().collect(),
            words: r.words.iter().map(|m| StoredWord { start: m.start, end: m.end, text: m.text.clone(), confidence: m.confidence, speaker: word_speaker(m) }).collect(),
            turns: r.turns.iter().map(|t| StoredTurn { start: t.start, end: t.end, speaker: t.speaker.clone() }).collect(),
            utterances: r.utterances.iter().map(write).collect(),
            raw: r.raw.iter().map(write).collect(),
            review: r.review.as_ref().map(|j| StoredReview {
                corrections: j.corrections.iter().map(correction).collect(),
                speaker_changes: j.speaker_changes.iter().map(|c| StoredSpeakerChange { utterance: c.utterance, speaker: c.speaker.clone() }).collect(),
                corrected_passages: j.corrected_passages.iter().map(|p| StoredCorrectedPassage { utterance: p.utterance, excerpt: p.excerpt.clone(), after: p.after.clone(), relisten: p.relisten.clone() }).collect(),
                lexicon_additions: j.lexicon_additions.iter().map(|a| StoredLexiconAddition { heard: a.heard.clone(), correct: a.correct.clone() }).collect(),
            }),
            passages: r
                .passages
                .iter()
                .map(|p| {
                    let (status, correction) = match &p.status {
                        PassageStatus::Pending => ("pending", None),
                        PassageStatus::HypothesisAccepted => ("hypothesis_accepted", None),
                        PassageStatus::OriginalKept => ("original_kept", None),
                        PassageStatus::CorrectedManually(t) => ("corrected_manually", Some(t.clone())),
                    };
                    StoredPassage { utterance: p.utterance, excerpt: p.excerpt.clone(), hypothesis: p.hypothesis.clone(), relisten: p.relisten.clone(), status: status.into(), correction }
                })
                .collect(),
            manual_corrections: r.manual_corrections.iter().map(|c| StoredCorrection { utterance: c.utterance, before: c.before.clone(), after: c.after.clone() }).collect(),
        }
    }

    /// Replaces the meeting's transcription (speakers, words, utterances, review, passages).
    /// With no speaker anywhere, everything goes to a single speaker, SPEAKER_00.
    pub fn apply(self, r: &mut Meeting) {
        let mut labels = self.speakers;
        let others = self.utterances.iter().chain(&self.raw).map(|x| &x.speaker).chain(self.turns.iter().map(|t| &t.speaker)).chain(self.words.iter().filter_map(|m| m.speaker.as_ref()));
        let mut added: Vec<String> = others.filter(|e| !labels.contains(e)).cloned().collect();
        added.sort();
        added.dedup();
        labels.extend(added);
        if labels.is_empty() && !self.utterances.is_empty() {
            labels.push("SPEAKER_00".into());
        }
        let index = |e: &str| labels.iter().position(|x| x == e).unwrap_or(0);
        let read = |x: &StoredUtterance| Utterance { speaker: index(&x.speaker), start: x.start, end: x.end, text: x.text.clone(), passages: vec![] };
        r.speakers = labels.iter().enumerate().map(|(i, e)| Speaker::new(e.clone(), HUES[i % HUES.len()])).collect();
        for (e, name) in self.names {
            if let Some(l) = r.speakers.iter_mut().find(|l| l.label == e) {
                l.name = Some(name).filter(|n| !n.trim().is_empty());
            }
        }
        for (e, into) in &self.merges {
            if let (Some(i), Some(j)) = (labels.iter().position(|x| x == e), labels.iter().position(|x| x == into)) {
                r.restore_merge(i, j);
            }
        }
        r.restore_merged_names(&self.names_before_merge, &self.lent_names);
        r.words = self.words.into_iter().map(|m| Word { speaker: m.speaker.as_deref().map(index), start: m.start, end: m.end, text: m.text, confidence: m.confidence }).collect();
        r.turns = self.turns.into_iter().map(|t| SpeakerTurn { start: t.start, end: t.end, speaker: t.speaker }).collect();
        r.utterances = self.utterances.iter().map(read).collect();
        r.raw = self.raw.iter().map(read).collect();
        r.review = self.review.map(|j| ReviewLog {
            corrections: j.corrections.into_iter().map(|c| ConfidentCorrection { utterance: c.utterance, before: c.before, after: c.after }).collect(),
            speaker_changes: j.speaker_changes.into_iter().map(|c| SpeakerChange { utterance: c.utterance, speaker: c.speaker }).collect(),
            corrected_passages: j.corrected_passages.into_iter().map(|p| CorrectedPassage { utterance: p.utterance, excerpt: p.excerpt, after: p.after, relisten: p.relisten }).collect(),
            lexicon_additions: j.lexicon_additions.into_iter().map(|a| LexiconAddition { heard: a.heard, correct: a.correct }).collect(),
        });
        r.corrections = r.review.as_ref().map_or(0, |j| (j.corrections.len() + j.corrected_passages.len()) as u32);
        r.passages.clear();
        for p in self.passages {
            let Some(x) = r.utterances.get_mut(p.utterance) else { continue };
            if x.passages.iter().any(|&i| r.passages[i].excerpt == p.excerpt) {
                continue;
            }
            x.passages.push(r.passages.len());
            let status = match (p.status.as_str(), p.correction) {
                ("hypothesis_accepted", _) if p.hypothesis.is_some() => PassageStatus::HypothesisAccepted,
                ("original_kept", _) => PassageStatus::OriginalKept,
                ("corrected_manually", Some(t)) => PassageStatus::CorrectedManually(t),
                _ => PassageStatus::Pending,
            };
            r.passages.push(PassageToCheck { utterance: p.utterance, excerpt: p.excerpt, hypothesis: p.hypothesis, relisten: p.relisten, status });
        }
        r.manual_corrections = self.manual_corrections.into_iter().map(|c| ManualCorrection { utterance: c.utterance, before: c.before, after: c.after }).collect();
    }
}

/// The readable transcription: one line per utterance, `[h:mm:ss] Name : text`, with the
/// speaker's name when known (merges followed), and the passages to check quoted below.
pub fn readable_transcription(r: &Meeting) -> String {
    let mut s = format!("# {} — transcription\n\nRéunion du {:04}-{:02}-{:02} à {}, {}.\n", r.title, r.start.year, r.start.month, r.start.day, r.start.short_time(), playback_timestamp(r.duration.round()));
    for x in &r.utterances {
        let name = r.speaker_name(x.speaker).unwrap_or(&r.speakers[r.displayed_speaker(x.speaker)].label);
        s.push_str(&format!("\n[{}] {name} : {}\n", playback_timestamp(x.start), x.text));
        for p in x.passages.iter().filter_map(|&i| r.passages.get(i)).filter(|p| p.status == PassageStatus::Pending) {
            match &p.hypothesis {
                Some(h) => s.push_str(&format!("> À vérifier : « {} » (hypothèse : « {h} »)\n", p.excerpt)),
                None => s.push_str(&format!("> À vérifier : « {} »\n", p.excerpt)),
            }
        }
    }
    s
}
