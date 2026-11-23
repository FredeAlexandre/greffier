//! The command line's wording, the same as the UI's (`crates/app/src/ui.rs`): stages, statuses,
//! durations; and reading the dates and instants a user or an agent types.

use greffier_core::{Meeting, Stage, Status};

use crate::Live;

/// A stage's name, as in the UI.
pub(crate) fn stage(e: Stage) -> &'static str {
    match e {
        Stage::Transcription => "Transcription",
        Stage::Review => "Relecture IA",
        Stage::Summary => "Bilan IA",
    }
}

/// « (étape faite : Transcription) », « (aucune étape faite) ».
pub(crate) fn completed_stage(m: &Meeting) -> String {
    match m.completed_stage {
        Some(e) => format!("(étape faite : {})", stage(e)),
        None => "(aucune étape faite)".into(),
    }
}

/// A meeting's status in a line, as in the UI's sidebar: « à traiter », « échec : Relecture
/// IA », « terminée, 2 à vérifier »; with the running process's data, where it stands in the
/// queue: « 2ᵉ dans la file · 15 min », « Transcription 40 % · 7 min ».
pub(crate) fn status(m: &Meeting, live: Option<&Live>) -> String {
    let eta = live.and_then(|l| l.time_remaining).map(|t| format!(" · {}", duration(t.max(60.0)))).unwrap_or_default();
    match (&m.status, live) {
        (Status::Recording, _) => "enregistrement en cours".into(),
        (Status::RecordingInterrupted, _) => "enregistrement interrompu".into(),
        (Status::ToProcess, _) => "à traiter".into(),
        (Status::Pending(_), Some(Live { rank: Some(r), .. })) => format!("{r}ᵉ dans la file{eta}"),
        (Status::Pending(_), _) => format!("en attente{eta}"),
        (Status::Running { stage: e, progress }, Some(_)) => format!("{} {:.0} %{eta}", stage(*e), progress * 100.0),
        (Status::Running { stage: e, .. }, None) => format!("{} en cours", stage(*e)),
        (Status::Failed { stage: e, .. }, _) => format!("échec : {}", stage(*e)),
        (Status::Done, _) if m.pending_passages() > 0 => format!("terminée, {} à vérifier", m.pending_passages()),
        (Status::Done, _) => "terminée".into(),
    }
}

/// 2 h 34, 50 min, 42 s (as the UI's).
pub(crate) fn duration(t: f64) -> String {
    let t = t.max(0.0) as u64;
    match (t / 3600, (t / 60) % 60) {
        (0, 0) => format!("{t} s"),
        (0, m) => format!("{m} min"),
        (h, m) => format!("{h} h {m:02}"),
    }
}

/// A calendar day, comparable: (year, month, day).
pub(crate) type Day = (i32, u8, u8);

/// `AAAA-MM-JJ` gives that day, `AAAA-MM` the whole month: the first and last days.
pub(crate) fn days(s: &str) -> Option<(Day, Day)> {
    let number = |x: &str, n: usize| (x.len() == n && x.bytes().all(|c| c.is_ascii_digit())).then(|| x.parse::<u32>().ok()).flatten();
    let parts: Vec<&str> = s.split('-').collect();
    let year = number(parts.first()?, 4)? as i32;
    let month = number(parts.get(1)?, 2).filter(|m| (1..=12).contains(m))? as u8;
    match parts.get(2) {
        None => Some(((year, month, 1), (year, month, 31))),
        Some(d) if parts.len() == 3 => {
            let day = number(d, 2).filter(|d| (1..=31).contains(d))? as u8;
            Some(((year, month, day), (year, month, day)))
        }
        _ => None,
    }
}

/// An instant in the meeting, in seconds: `h:mm:ss`, `m:ss` or seconds (`90`, `2.5`).
pub(crate) fn instant(s: &str) -> Option<f64> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() == 1 {
        return s.parse::<f64>().ok().filter(|t| t.is_finite() && *t >= 0.0);
    }
    let number = |x: &str| (!x.is_empty() && x.bytes().all(|c| c.is_ascii_digit())).then(|| x.parse::<f64>().ok()).flatten();
    let sixty = |x: &str| number(x).filter(|n| x.len() == 2 && *n < 60.0);
    match parts.as_slice() {
        [m, s] => Some(number(m)? * 60.0 + sixty(s)?),
        [h, m, s] => Some(number(h)? * 3600.0 + sixty(m)? * 60.0 + sixty(s)?),
        _ => None,
    }
}
