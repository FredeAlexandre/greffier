//! The command line's wording, in English with the glossary's terms (ADR 0006): stages,
//! statuses, durations, and their names in JSON; and reading the dates and instants a user or
//! an agent types.

use greffier_core::{DateTime, Meeting, Stage, Status};

use crate::Live;

/// A stage's name: Transcription, Review, Summary.
pub(crate) fn stage(e: Stage) -> &'static str {
    match e {
        Stage::Transcription => "Transcription",
        Stage::Review => "Review",
        Stage::Summary => "Summary",
    }
}

/// A stage's name in JSON and on the command line (`redo`): `transcription`, `review`, `summary`.
pub(crate) fn stage_name(e: Stage) -> &'static str {
    match e {
        Stage::Transcription => "transcription",
        Stage::Review => "review",
        Stage::Summary => "summary",
    }
}

/// A status's name in JSON.
pub(crate) fn status_name(s: &Status) -> &'static str {
    match s {
        Status::Recording => "recording",
        Status::RecordingInterrupted => "recording_interrupted",
        Status::ToProcess => "to_process",
        Status::Pending(_) => "queued",
        Status::Running { .. } => "running",
        Status::Failed { .. } => "failed",
        Status::Done => "done",
    }
}

/// "(completed stage: Transcription)", "(no stage completed)".
pub(crate) fn completed_stage(m: &Meeting) -> String {
    match m.completed_stage {
        Some(e) => format!("(completed stage: {})", stage(e)),
        None => "(no stage completed)".into(),
    }
}

/// "1st", "2nd", "3rd", "4th", "11th", "21st"…
pub(crate) fn ordinal(n: usize) -> String {
    let suffix = match (n % 10, n % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

/// A meeting's status in a line: "to process", "failed: Review", "done, 2 to check"; with the
/// running process's data, where it stands in the queue: "2nd in queue · 15 min",
/// "Transcription 40 % · 7 min".
pub(crate) fn status(m: &Meeting, live: Option<&Live>) -> String {
    let eta = live.and_then(|l| l.time_remaining).map(|t| format!(" · {}", duration(t.max(60.0)))).unwrap_or_default();
    match (&m.status, live) {
        (Status::Recording, _) => "recording".into(),
        (Status::RecordingInterrupted, _) => "recording interrupted".into(),
        (Status::ToProcess, _) => "to process".into(),
        (Status::Pending(_), Some(Live { rank: Some(r), .. })) => format!("{} in queue{eta}", ordinal(*r)),
        (Status::Pending(_), _) => format!("queued{eta}"),
        (Status::Running { stage: e, progress }, Some(_)) => format!("{} {:.0} %{eta}", stage(*e), progress * 100.0),
        (Status::Running { stage: e, .. }, None) => format!("{} running", stage(*e)),
        (Status::Failed { stage: e, .. }, _) => format!("failed: {}", stage(*e)),
        (Status::Done, _) if m.pending_passages() > 0 => format!("done, {} to check", m.pending_passages()),
        (Status::Done, _) => "done".into(),
    }
}

/// 2 h 34, 50 min, 42 s.
pub(crate) fn duration(t: f64) -> String {
    let t = t.max(0.0) as u64;
    match (t / 3600, (t / 60) % 60) {
        (0, 0) => format!("{t} s"),
        (0, m) => format!("{m} min"),
        (h, m) => format!("{h} h {m:02}"),
    }
}

/// "2026-11-20 10:00", a meeting's start.
pub(crate) fn start(t: DateTime) -> String {
    format!("{:04}-{:02}-{:02} {:02}:{:02}", t.year, t.month, t.day, t.hour, t.minute)
}

/// A calendar day, comparable: (year, month, day).
pub(crate) type Day = (i32, u8, u8);

/// `YYYY-MM-DD` gives that day, `YYYY-MM` the whole month: the first and last days.
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
