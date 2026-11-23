//! From Whisper's words to utterances: noises removed, each word attributed to a speaker, split
//! into utterances. The rules are the vault's:
//!
//! 1. **Speaker of each word**: the word is shifted by +0.2 s (Whisper timestamps a bit early);
//!    it goes to the speaker who talks the most during that time, otherwise to the nearest turn
//!    within one second, otherwise to the previous word's speaker. A word stretched over more
//!    than 2 s (whisper.cpp sometimes ends a word when speech resumes) only counts for its first
//!    moments.
//! 2. **Interruptions**: 2 words or fewer (punctuation alone does not count) in the middle of
//!    another speaker's words, with no sentence end, are given back to that speaker: it is a
//!    wobble of the diarization, not an answer.
//! 3. **Realignment**: a speaker change 3 words or fewer from a sentence end is moved there,
//!    backward first (the diarization is most often late).
//! 4. **Labels**: SPEAKER_00, SPEAKER_01… in the order of each one's first word; speakers
//!    detected without any word follow, in the order of their first turn. The speaker turns take
//!    these labels.
//! 5. **Utterances**: a new one at each speaker change (punctuation alone stays with the
//!    previous one), after a 2 s pause, or at a sentence end past 220 characters.

use crate::meeting::{Speaker, Word, Utterance, HUES, SpeakerTurn};

/// The offset added to Whisper's timestamps before looking for the speaker, in seconds (the
/// best overlap with pyannote on the vault's benchmark).
const OFFSET: f64 = 0.2;
/// Beyond this, a word is stretched over a silence.
const STRETCHED_WORD: f64 = 2.0;
/// Maximum distance to the nearest turn, when none overlaps the word, in seconds.
const NEAR: f64 = 1.0;
/// A given-back interruption has at most this many words.
const INTERRUPTION_WORDS: usize = 2;
/// A speaker change is realigned on a sentence end within this many words.
const REALIGNMENT_WORDS: usize = 3;

/// A pause that starts a new utterance, in seconds.
const PAUSE: f64 = 2.0;
/// Past this length (in UTF-16 units, like the vault), an utterance stops at the first sentence
/// end.
const UTTERANCE_LENGTH: usize = 220;
/// Beyond this many words, a parenthesis or bracket that does not close is not a noise.
const NOISE_WORDS: usize = 6;

/// The words without the noises noted between brackets or parentheses (`[Musique]`, `(rires)`,
/// `[bruit de fond]`) nor the empty words.
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

/// A text (the relisten of an excerpt) without its noises, same rules as `without_noises`, its
/// spaces collapsed to one.
pub fn text_without_noises(text: &str) -> String {
    let words = text.split_whitespace().map(|t| Word { start: 0.0, end: 0.0, text: t.to_string(), confidence: 1.0, speaker: None }).collect();
    without_noises(words).into_iter().map(|m| m.text).collect::<Vec<_>>().join(" ")
}

/// `.`, `?`, `!` or `…`, optionally followed by a closing quote.
fn ends_sentence(text: &str) -> bool {
    let t = text.trim();
    let t = t.strip_suffix('»').or_else(|| t.strip_suffix('"')).unwrap_or(t);
    t.ends_with(['.', '?', '!', '…'])
}

/// A real word (a letter or a digit), not punctuation alone.
fn has_letter(text: &str) -> bool {
    text.chars().any(|c| c.is_alphabetic() || c.is_ascii_digit())
}

/// The result of the attribution: a meeting's transcription before review.
#[derive(Clone, Debug, PartialEq)]
pub struct Attribution {
    pub speakers: Vec<Speaker>,
    /// The words, each with its speaker.
    pub words: Vec<Word>,
    /// The speaker turns, with the speakers' labels.
    pub turns: Vec<SpeakerTurn>,
    pub utterances: Vec<Utterance>,
}

/// Attributes each word to a speaker and splits the utterances. Without speaker turns,
/// everything goes to a single speaker, SPEAKER_00.
pub fn attribute(words: Vec<Word>, turns: Vec<SpeakerTurn>) -> Attribution {
    let mut turns = turns;
    turns.sort_by(|a, b| a.start.total_cmp(&b.start));
    // The diarizer's speakers, in the order of their first turn.
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

    // Labels: in the order of the first word, then of the first turn.
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

/// Rule 1: the speaker (diarizer number) of each word.
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
        // Each speaker's speaking time during the word, in the order of the turns met.
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

/// Rule 2: interruptions of 2 words or fewer, with no sentence end, given back to the current
/// speaker. A single pass, left to right.
///
/// "In the middle of someone else's sentence" reads as in the vault (`assign()`, carried over
/// as is, spec #1): the same speaker on both sides and no sentence end in the interruption.
/// The vault does not check whether the word before ends a sentence; neither do we, to keep the
/// same utterances as it does.
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

/// Rule 3: each speaker change that does not follow a sentence end is moved to the nearest
/// sentence end within 3 words, backward first. Changes already made count for the following
/// ones.
///
/// "The nearest" follows the vault (`assign()`, carried over as is, spec #1): the nearest one
/// backward (the diarization's usual lag) if it exists, otherwise the nearest one forward, even
/// when the forward one is closer.
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

/// Rule 5: the utterances, from the already attributed words.
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
            let r = utterances.last_mut().expect("utterance in progress");
            r.text.push(' ');
            r.text.push_str(text);
            r.end = r.end.max(word.end);
        } else {
            utterances.push(Utterance { speaker, start: word.start, end: word.end, text: text.to_string(), passages: vec![] });
        }
    }
    utterances
}
