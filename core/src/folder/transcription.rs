//! La transcription structurée (`… - transcription.json`) et la transcription lisible
//! (`… - transcription.md`).
//!
//! Le JSON garde tout ce que le traitement a produit : les mots (horodatages, confiance,
//! locuteur), les tours de parole, les répliques, la transcription brute d'avant relecture, le
//! journal de la relecture, les passages à vérifier et les corrections faites à la main. Les
//! locuteurs y sont toujours désignés par leur étiquette (« SPEAKER_00 »), même nommés ou
//! fusionnés ; `noms` et `fusions` donnent la correspondance, la même que dans la fiche (qui fait
//! foi quand elle existe).
//!
//! ```json
//! {
//!   "version": 1,
//!   "locuteurs": ["SPEAKER_00", "SPEAKER_01", "SPEAKER_02"],
//!   "noms": { "SPEAKER_00": "Alexandre", "SPEAKER_01": "Khalid", "SPEAKER_02": "Alexandre" },
//!   "fusions": { "SPEAKER_02": "SPEAKER_00" },
//!   "mots": [{ "debut": 0.0, "fin": 0.25, "texte": "Bonjour", "confiance": 0.9, "locuteur": "SPEAKER_00" }],
//!   "tours": [{ "debut": 0.0, "fin": 2.0, "locuteur": "SPEAKER_00" }],
//!   "repliques": [{ "locuteur": "SPEAKER_00", "debut": 0.0, "fin": 2.0, "texte": "Bonjour Khalid." }],
//!   "brute": [ …les répliques d'avant la relecture, absentes si elle n'est pas faite… ],
//!   "relecture": { "corrections": [{ "replique": 1, "avant": "recète", "apres": "recette" }],
//!                  "changements_de_locuteur": [{ "replique": 3, "locuteur": "SPEAKER_01" }],
//!                  "passages_corriges": [{ "replique": 4, "extrait": "la prod", "apres": "la preprod",
//!                                          "reecoute": "on bascule la preprod" }],
//!                  "ajouts_au_lexique": [{ "entendu": "recète", "correct": "recette" }] },
//!   "passages": [{ "replique": 2, "extrait": "le support", "hypothese": "l'équipe support",
//!                  "reecoute": "…", "statut": "pending" }],
//!   "corrections_a_la_main": [{ "replique": 0, "avant": "…", "apres": "…" }]
//! }
//! ```
//!
//! `statut` d'un passage : `en_attente`, `hypothese_acceptee`, `original_garde` ou
//! `corrige_a_la_main` (avec `correction`). `relecture` absente : la relecture n'est pas faite
//! (une transcription importée est réputée non relue).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::adapters::{LexiconAddition, SpeakerChange, ConfidentCorrection, CorrectedPassage};
use crate::meeting::{ManualCorrection, ReviewLog, Speaker, Word, PassageToCheck, Utterance, Meeting, PassageStatus, HUES, SpeakerTurn};
use crate::time::playback_timestamp;

pub const VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StructuredTranscription {
    pub version: u32,
    /// Les étiquettes des locuteurs, dans l'ordre (leurs couleurs en dépendent).
    #[serde(default)]
    pub speakers: Vec<String>,
    /// SPEAKER_xx → nom donné par l'utilisateur, fusions comprises.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub names: BTreeMap<String, String>,
    /// SPEAKER_xx → SPEAKER_yy : le premier est la même voix que le second, qui le remplace.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub merges: BTreeMap<String, String>,
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
    /// Les passages douteux corrigés après la réécoute.
    #[serde(default)]
    pub corrected_passages: Vec<StoredCorrectedPassage>,
    /// Les erreurs écrites dans le lexique.
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
        // Le locuteur d'un mot : celui que l'attribution lui a donné, sinon (mots relus d'une
        // transcription qui n'en avait pas) celui de la réplique où il commence.
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

    /// Remplace la transcription de la réunion (locuteurs, mots, répliques, relecture,
    /// passages). Sans locuteur nulle part, tout va à un seul locuteur, SPEAKER_00.
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
        r.forget_merged_names();
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

/// La transcription lisible : une ligne par réplique, `[h:mm:ss] Nom : texte`, avec le nom du
/// locuteur quand il est connu (fusions suivies), et les passages à vérifier en citation dessous.
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
