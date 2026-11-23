//! `greffier status`: where processing stands. Without a meeting, the meetings that are not
//! done; with one, its status, last completed stage and failure. Read from the files; the
//! process's live data (rank in the queue, progress, time remaining), when a [`crate::LiveSource`]
//! gives it, takes over the same output.

use greffier_core::Status;
use serde::Serialize;

use crate::library::{Entry, Library, MeetingJson};
use crate::{Context, Failure, HELP, args, text};

/// A meeting's status in JSON: the meeting, plus what only the running process knows.
#[derive(Serialize)]
struct StatusJson {
    #[serde(flatten)]
    meeting: MeetingJson,
    /// Place in the queue, from 1.
    rang: Option<usize>,
    /// The running stage's progress, from 0 to 1.
    progression: Option<f64>,
    /// Seconds until processed.
    temps_restant: Option<u64>,
}

impl StatusJson {
    fn new(e: &Entry, library: &Library) -> StatusJson {
        let live = e.live.as_ref();
        StatusJson {
            meeting: MeetingJson::new(e, &library.folder),
            rang: live.and_then(|l| l.rank),
            progression: live.and_then(|l| match l.status {
                Status::Running { progress, .. } => Some(progress),
                _ => None,
            }),
            temps_restant: live.and_then(|l| l.time_remaining).map(|t| t.max(0.0).round() as u64),
        }
    }
}

/// The order of the meetings not done: what is moving first, then what needs the user.
fn order(s: &Status) -> u8 {
    match s {
        Status::Recording => 0,
        Status::Running { .. } => 1,
        Status::Pending(_) => 2,
        Status::Failed { .. } => 3,
        Status::RecordingInterrupted => 4,
        Status::ToProcess => 5,
        Status::Done => 6,
    }
}

pub(crate) fn run(cx: &Context, args: &[String]) -> Result<String, Failure> {
    let a = args::parse(args, &["--json"], &[])?;
    if a.help() {
        return Ok(HELP.into());
    }
    a.at_most(1)?;
    // What the process already running knows (it is never launched for this).
    let live: Context;
    let cx = match (&cx.live, &cx.process, &cx.meetings_folder) {
        (None, Some(process), Some(folder)) if folder.is_dir() => match process.snapshot(folder) {
            Some(snapshot) => {
                live = Context { meetings_folder: Some(folder.clone()), app_folder: cx.app_folder.clone(), live: Some(Box::new(snapshot)), process: None };
                &live
            }
            None => cx,
        },
        _ => cx,
    };
    let library = Library::open(cx)?;
    let json = a.flag("--json");
    if let Some(query) = a.positionals.first() {
        let e = library.find(query)?;
        if json {
            return Ok(crate::json(&StatusJson::new(&e, &library)));
        }
        let m = &e.meeting;
        let mut s = format!("{} · {}\nStatut : {}\n", e.id, m.title, text::status(m, e.live.as_ref()));
        s += &format!("Étape faite : {}\n", m.completed_stage.map_or("aucune", text::stage));
        if let Status::Failed { message, .. } = &m.status {
            s += &format!("Erreur : {message}\n");
        }
        return Ok(s);
    }
    let mut entries: Vec<Entry> = library.all().into_iter().filter(|e| e.meeting.status != Status::Done).collect();
    entries.sort_by_key(|e| order(&e.meeting.status));
    if json {
        return Ok(crate::json(&entries.iter().map(|e| StatusJson::new(e, &library)).collect::<Vec<_>>()));
    }
    if entries.is_empty() {
        return Ok("Rien en cours.\n".into());
    }
    Ok(entries
        .iter()
        .map(|e| {
            let m = &e.meeting;
            let error = match &m.status {
                Status::Failed { message, .. } => format!(" · {message}"),
                _ => String::new(),
            };
            format!("{} · {} · {}{error}\n", e.id, text::status(m, e.live.as_ref()), m.title)
        })
        .collect())
}
