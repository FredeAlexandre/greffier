//! Des mots de Whisper aux répliques : bruits retirés, attribution de chaque mot à un locuteur,
//! découpage en répliques. Les règles sont celles du vault :
//!
//! 1. **Locuteur de chaque mot** : le mot est décalé de +0,2 s (Whisper horodate un peu tôt) ;
//!    il va au locuteur qui parle le plus pendant ce temps, sinon au tour le plus proche à moins
//!    d'une seconde, sinon au locuteur du mot précédent. Un mot étiré sur plus de 2 s (whisper.cpp
//!    finit parfois un mot à la reprise de la parole) ne compte que pour ses premiers instants.
//! 2. **Interruptions** : 2 mots ou moins (la ponctuation seule ne compte pas) au milieu des mots
//!    d'un autre locuteur, sans fin de phrase, lui sont rendus : c'est un flottement de la
//!    séparation, pas une réponse.
//! 3. **Recalage** : un changement de locuteur à 3 mots ou moins d'une fin de phrase y est
//!    déplacé, en arrière d'abord (la séparation est le plus souvent en retard).
//! 4. **Étiquettes** : SPEAKER_00, SPEAKER_01… dans l'ordre du premier mot de chacun ; les
//!    locuteurs détectés sans aucun mot suivent, dans l'ordre de leur premier tour. Les tours de
//!    parole prennent ces étiquettes.
//! 5. **Répliques** : une nouvelle à chaque changement de locuteur (une ponctuation seule reste
//!    avec la précédente), après une pause de 2 s, ou à une fin de phrase passé 220 caractères.

use crate::meeting::{Speaker, Word, Utterance, HUES, SpeakerTurn};

/// Le décalage ajouté aux horodatages de Whisper avant de chercher le locuteur, en secondes (le
/// meilleur recouvrement avec pyannote sur le banc du vault).
const OFFSET: f64 = 0.2;
/// Au-delà, un mot est étiré sur un silence.
const STRETCHED_WORD: f64 = 2.0;
/// Distance maximale au tour le plus proche, quand aucun ne chevauche le mot, en secondes.
const NEAR: f64 = 1.0;
/// Une interruption rendue compte au plus ce nombre de mots.
const INTERRUPTION_WORDS: usize = 2;
/// Un changement de locuteur est recalé sur une fin de phrase à ce nombre de mots près.
const REALIGNMENT_WORDS: usize = 3;

/// Une pause qui commence une nouvelle réplique, en secondes.
const PAUSE: f64 = 2.0;
/// Passé cette longueur (en unités UTF-16, comme le vault), une réplique s'arrête à la première
/// fin de phrase.
const UTTERANCE_LENGTH: usize = 220;
/// Au-delà de ce nombre de mots, une parenthèse ou un crochet qui ne se ferme pas n'est pas un
/// bruit.
const NOISE_WORDS: usize = 6;

/// Les mots sans les bruits notés entre crochets ou parenthèses (« [Musique] », « (rires) »,
/// « [bruit de fond] ») ni les mots vides.
pub fn without_noises(words: Vec<Word>) -> Vec<Word> {
    let opens = |t: &str| t.starts_with('[') || t.starts_with('(');
    let closes = |t: &str| t.ends_with(']') || t.ends_with(')');
    let mut kept = Vec::with_capacity(words.len());
    let mut noise: Option<Vec<Word>> = None;
    for m in words.into_iter().filter(|m| !m.text.trim().is_empty()) {
        let t = m.text.trim().to_string();
        if let Some(b) = &mut noise {
            b.push(m);
            if closes(&t) {
                noise = None;
            } else if b.len() > NOISE_WORDS {
                kept.append(b);
                noise = None;
            }
            continue;
        }
        if opens(&t) {
            if !closes(&t) {
                noise = Some(vec![m]);
            }
            continue;
        }
        kept.push(m);
    }
    kept.extend(noise.unwrap_or_default());
    kept
}

/// Un texte (la réécoute d'un extrait) sans ses bruits, mêmes règles que `sans_les_bruits`, ses
/// espaces ramenés à un seul.
pub fn text_without_noises(text: &str) -> String {
    let words = text.split_whitespace().map(|t| Word { start: 0.0, end: 0.0, text: t.to_string(), confidence: 1.0, speaker: None }).collect();
    without_noises(words).into_iter().map(|m| m.text).collect::<Vec<_>>().join(" ")
}

/// `.`, `?`, `!` ou `…`, suivi ou non d'un guillemet fermant.
fn ends_sentence(text: &str) -> bool {
    let t = text.trim();
    let t = t.strip_suffix('»').or_else(|| t.strip_suffix('"')).unwrap_or(t);
    t.ends_with(['.', '?', '!', '…'])
}

/// Un vrai mot (une lettre ou un chiffre), pas une ponctuation seule.
fn has_letter(text: &str) -> bool {
    text.chars().any(|c| c.is_alphabetic() || c.is_ascii_digit())
}

/// Le résultat de l'attribution : la transcription d'une réunion avant relecture.
#[derive(Clone, Debug, PartialEq)]
pub struct Attribution {
    pub speakers: Vec<Speaker>,
    /// Les mots, chacun avec son locuteur.
    pub words: Vec<Word>,
    /// Les tours de parole, avec les étiquettes des locuteurs.
    pub turns: Vec<SpeakerTurn>,
    pub utterances: Vec<Utterance>,
}

/// Attribue chaque mot à un locuteur et découpe les répliques. Sans tours de parole, tout va à
/// un seul locuteur, SPEAKER_00.
pub fn attribute(words: Vec<Word>, turns: Vec<SpeakerTurn>) -> Attribution {
    let mut turns = turns;
    turns.sort_by(|a, b| a.start.total_cmp(&b.start));
    // Les locuteurs du séparateur, dans l'ordre de leur premier tour.
    let mut from_diarizer: Vec<String> = Vec::new();
    for t in &turns {
        if !from_diarizer.contains(&t.speaker) {
            from_diarizer.push(t.speaker.clone());
        }
    }
    let numbers: Vec<usize> = turns.iter().map(|t| from_diarizer.iter().position(|l| *l == t.speaker).unwrap_or(0)).collect();

    let mut ids = word_speakers(&words, &turns, &numbers);
    return_interruptions(&words, &mut ids);
    snap_to_sentence_ends(&words, &mut ids);

    // Étiquettes : dans l'ordre du premier mot, puis du premier tour.
    let mut order: Vec<usize> = Vec::new();
    for &i in ids.iter().chain(&numbers) {
        if !order.contains(&i) {
            order.push(i);
        }
    }
    let rank = |i: usize| order.iter().position(|&x| x == i).unwrap_or(0);
    let speakers: Vec<Speaker> = (0..order.len()).map(|i| Speaker::new(Speaker::numbered_label(i), HUES[i % HUES.len()])).collect();
    let turns = turns.into_iter().zip(&numbers).map(|(t, &n)| SpeakerTurn { speaker: speakers[rank(n)].label.clone(), ..t }).collect();
    let words: Vec<Word> = words.into_iter().zip(&ids).map(|(m, &i)| Word { speaker: Some(rank(i)), ..m }).collect();
    let utterances = utterances(&words);
    Attribution { speakers, words, turns, utterances }
}

/// Règle 1 : le locuteur (numéro du séparateur) de chaque mot.
fn word_speakers(words: &[Word], turns: &[SpeakerTurn], numbers: &[usize]) -> Vec<usize> {
    let mut ids = Vec::with_capacity(words.len());
    let mut previous: Option<usize> = None;
    let mut first = 0;
    for m in words {
        let duration = m.end - m.start;
        let start = m.start + OFFSET;
        let end = OFFSET + if duration > STRETCHED_WORD { m.start + duration.min(0.3 + 0.08 * m.text.trim().encode_utf16().count() as f64) } else { m.end };
        while first < turns.len() && turns[first].end < start - 30.0 {
            first += 1;
        }
        // Temps de parole de chaque locuteur pendant le mot, dans l'ordre des tours rencontrés.
        let mut speech: Vec<(usize, f64)> = Vec::new();
        let (mut closest, mut distance) = (None, NEAR);
        for (t, &n) in turns[first..].iter().zip(&numbers[first..]) {
            if t.start > end + distance {
                break;
            }
            let overlap = end.min(t.end) - start.max(t.start);
            if overlap > 0.0 {
                match speech.iter_mut().find(|(l, _)| *l == n) {
                    Some((_, s)) => *s += overlap,
                    None => speech.push((n, overlap)),
                }
            } else if -overlap < distance {
                (distance, closest) = (-overlap, Some(n));
            }
        }
        let mut most: Option<(usize, f64)> = None;
        for &(l, s) in &speech {
            if most.is_none_or(|(_, m)| s > m) {
                most = Some((l, s));
            }
        }
        let id = most.map(|(l, _)| l).or(closest).or(previous).unwrap_or(0);
        ids.push(id);
        previous = Some(id);
    }
    ids
}

/// Règle 2 : les interruptions de 2 mots ou moins, sans fin de phrase, rendues au locuteur en
/// cours. Une seule passe, de gauche à droite.
///
/// « Au milieu de la phrase de quelqu'un d'autre » se lit comme dans le vault (`assign()`,
/// repris à l'identique, spec #1) : le même locuteur de part et d'autre et aucune fin de phrase
/// dans l'interruption. Le vault ne regarde pas si le mot d'avant finit une phrase ; on ne le
/// regarde pas non plus, pour garder les mêmes répliques que lui.
fn return_interruptions(words: &[Word], ids: &mut [usize]) {
    let mut i = 0;
    while i < ids.len() {
        let mut j = i;
        while j + 1 < ids.len() && ids[j + 1] == ids[i] {
            j += 1;
        }
        if i > 0 && j + 1 < ids.len() {
            let (before, after) = (ids[i - 1], ids[j + 1]);
            let sequence = &words[i..=j];
            let real_words = sequence.iter().filter(|m| has_letter(&m.text)).count();
            if before == after && before != ids[i] && real_words <= INTERRUPTION_WORDS && !sequence.iter().any(|m| ends_sentence(&m.text)) {
                ids[i..=j].fill(before);
            }
        }
        i = j + 1;
    }
}

/// Règle 3 : chaque changement de locuteur qui ne suit pas une fin de phrase est déplacé sur la
/// fin de phrase la plus proche à 3 mots près, en arrière d'abord. Les changements déjà faits
/// comptent pour les suivants.
///
/// « La plus proche » suit le vault (`assign()`, repris à l'identique, spec #1) : la plus proche
/// en arrière (le retard habituel de la séparation) si elle existe, sinon la plus proche en
/// avant, même quand celle d'avant est plus près.
fn snap_to_sentence_ends(words: &[Word], ids: &mut [usize]) {
    let ends = |k: usize| ends_sentence(&words[k].text);
    for i in 1..ids.len() {
        if ids[i] == ids[i - 1] || ends(i - 1) {
            continue;
        }
        let (old, new) = (ids[i - 1], ids[i]);
        let backward = (1..=REALIGNMENT_WORDS).find(|&k| i > k && ids[i - k..i].iter().all(|&x| x == old) && ends(i - k - 1));
        if let Some(k) = backward {
            ids[i - k..i].fill(new);
            continue;
        }
        let before = (1..=REALIGNMENT_WORDS).find(|&k| i + k <= ids.len() && ids[i..i + k].iter().all(|&x| x == new) && ends(i + k - 1));
        if let Some(k) = before {
            ids[i..i + k].fill(old);
        }
    }
}

/// Règle 5 : les répliques, des mots déjà attribués.
fn utterances(words: &[Word]) -> Vec<Utterance> {
    let mut utterances: Vec<Utterance> = Vec::new();
    for word in words {
        let text = word.text.trim();
        let speaker = word.speaker.unwrap_or(0);
        let sequence = match utterances.last() {
            None => false,
            Some(r) => {
                let is_long = r.text.encode_utf16().count() >= UTTERANCE_LENGTH && ends_sentence(&r.text);
                (r.speaker == speaker || !has_letter(text)) && word.start - r.end < PAUSE && !is_long
            }
        };
        if sequence {
            let r = utterances.last_mut().expect("réplique en cours");
            r.text.push(' ');
            r.text.push_str(text);
            r.end = r.end.max(word.end);
        } else {
            utterances.push(Utterance { speaker, start: word.start, end: word.end, text: text.to_string(), passages: vec![] });
        }
    }
    utterances
}
