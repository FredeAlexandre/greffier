//! What the user typed, read against the live core, with the numbering of `greffier show`:
//! meetings as [`Identifiers::resolve`] reads them (a path, if it leads into the meetings
//! folder), utterances, passages and actions from 1
//! (`#3` or `3`), an utterance also by an instant in it (`0:12:05`), speakers by index (`0`),
//! label (`SPEAKER_00`) or displayed name.

use greffier_core::{Greffier, MeetingId, playback_timestamp};

use super::identifiers;

/// The meeting `query` names (identifier, folder name or path, date, start of identifier). The
/// candidates of an ambiguous query come with their live titles.
pub(super) fn meeting_named(core: &Greffier, query: &str) -> Result<MeetingId, String> {
    let ids = identifiers(core);
    let query = crate::reading::in_meetings_folder(query, core.folder().root())?;
    let name = ids.resolve(query).map_err(|mut e| {
        for (id, title) in &mut e.candidates {
            let meeting = ids.iter().find(|(i, _)| i == id).and_then(|(_, n)| core.meetings().iter().find(|m| m.folder_name == n));
            if let Some(m) = meeting {
                *title = m.title.clone();
            }
        }
        e.to_string()
    })?;
    match core.meetings().iter().find(|m| m.folder_name == name) {
        Some(m) => Ok(m.id),
        None => Err(format!("la réunion {} est en cours d'enregistrement", ids.of(name).unwrap_or(name))),
    }
}

/// A number from 1 to `count` (`3` or `#3`), as an index from 0.
pub(super) fn number(s: &str, count: usize) -> Option<usize> {
    let n: usize = s.trim().strip_prefix('#').unwrap_or(s.trim()).parse().ok()?;
    (1..=count).contains(&n).then(|| n - 1)
}

/// The utterance `s` names: its number, or an instant (`h:mm:ss`, `m:ss`) inside it.
pub(super) fn utterance(core: &Greffier, meeting: MeetingId, s: &str) -> Result<usize, String> {
    let m = core.meeting(meeting).ok_or("réunion inconnue")?;
    let count = m.utterances.len();
    if count == 0 {
        return Err("pas encore de transcription".into());
    }
    let unknown = || format!("réplique introuvable : « {s} » (numéro de 1 à {count}, ou instant h:mm:ss)");
    if !s.contains(':') {
        return number(s, count).ok_or_else(unknown);
    }
    let t = crate::text::instant(s.trim()).ok_or_else(unknown)?;
    m.utterances.iter().position(|u| u.start <= t && t <= u.end).ok_or_else(|| format!("aucune réplique à {}", playback_timestamp(t)))
}

/// Lowercase, without accents, without the « ? » of a proposed name.
fn folded(s: &str) -> String {
    let s = s.trim();
    let s = s.strip_suffix('?').unwrap_or(s).trim_end();
    s.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => 'a',
            'ç' => 'c',
            'è' | 'é' | 'ê' | 'ë' => 'e',
            'ì' | 'í' | 'î' | 'ï' => 'i',
            'ñ' => 'n',
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' => 'o',
            'ù' | 'ú' | 'û' | 'ü' => 'u',
            'ý' | 'ÿ' => 'y',
            '’' | '‘' => '\'',
            c => c,
        })
        .collect()
}

/// The speaker `s` names, with its label: its index, its label, or the name a single voice
/// displays (a merged speaker displays the voice replacing it).
pub(super) fn speaker(core: &Greffier, meeting: MeetingId, s: &str) -> Result<(usize, String), String> {
    let m = core.meeting(meeting).ok_or("réunion inconnue")?;
    if m.speakers.is_empty() {
        return Err("pas encore de transcription".into());
    }
    let found = |i: usize| Ok((i, m.speakers[i].label.clone()));
    let q = s.trim();
    if let Ok(i) = q.parse::<usize>() {
        return if i < m.speakers.len() { found(i) } else { Err(unknown_speaker(s)) };
    }
    if let Some(i) = m.speakers.iter().position(|l| l.label.eq_ignore_ascii_case(q)) {
        return found(i);
    }
    let wanted = folded(q);
    // As `show --locuteurs` displays them: a merged speaker shows the voice replacing it.
    let mut voices: Vec<usize> = (0..m.speakers.len()).map(|i| m.displayed_speaker(i)).filter(|&i| folded(&m.speakers[i].display_name()) == wanted).collect();
    voices.sort_unstable();
    voices.dedup();
    match voices.as_slice() {
        [] => Err(unknown_speaker(s)),
        [i] => found(*i),
        _ => Err(format!("plusieurs locuteurs pour « {s} » : {}", voices.iter().map(|&i| m.speakers[i].label.as_str()).collect::<Vec<_>>().join(", "))),
    }
}

fn unknown_speaker(s: &str) -> String {
    format!("locuteur introuvable : « {s} » (index, label SPEAKER_00 ou nom affiché)")
}
