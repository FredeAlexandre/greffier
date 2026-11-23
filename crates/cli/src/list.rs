//! `greffier list`: the meetings, most recent first, one line each:
//! `identifiant · durée · statut · titre`.

use greffier_core::playback_timestamp;

use crate::reading::{MeetingJson, Reading};
use crate::text::{self, Day};
use crate::{Context, Failure, HELP, args};

pub(crate) fn run(cx: &Context, args: &[String]) -> Result<String, Failure> {
    let a = args::parse(args, &["--json"], &["--date", "--since", "--until", "--limit"])?;
    if a.help() {
        return Ok(HELP.into());
    }
    a.at_most(0)?;
    let days = |option: &str| -> Result<Option<(Day, Day)>, Failure> {
        a.value(option).map(|d| text::days(d).ok_or_else(|| Failure::Usage(format!("date invalide : « {d} » (AAAA-MM-JJ ou AAAA-MM)")))).transpose()
    };
    let (date, since, until) = (days("--date")?, days("--since")?, days("--until")?);
    let limit = match a.value("--limit") {
        Some(n) => Some(n.parse::<usize>().ok().filter(|n| *n > 0).ok_or_else(|| Failure::Usage(format!("--limit attend un nombre : « {n} »")))?),
        None => None,
    };
    let reading = Reading::open(cx)?;
    let first = since.map(|d| d.0).into_iter().chain(date.map(|d| d.0)).max();
    let last = until.map(|d| d.1).into_iter().chain(date.map(|d| d.1)).min();
    let entries: Vec<_> = reading
        .all()
        .into_iter()
        .filter(|e| {
            let s = e.meeting.start;
            let day = (s.year, s.month, s.day);
            first.is_none_or(|f| day >= f) && last.is_none_or(|l| day <= l)
        })
        .take(limit.unwrap_or(usize::MAX))
        .collect();
    if a.flag("--json") {
        return Ok(crate::json(&entries.iter().map(|e| MeetingJson::new(e, &reading.folder)).collect::<Vec<_>>()));
    }
    if entries.is_empty() {
        return Ok("Aucune réunion.\n".into());
    }
    Ok(entries.iter().map(|e| format!("{} · {} · {} · {}\n", e.identifier, playback_timestamp(e.meeting.duration), text::status(&e.meeting, e.live.as_ref()), e.meeting.title)).collect())
}
