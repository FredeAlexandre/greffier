//! The Greffier process's side of the command line (ADR 0006): it answers the requests of
//! [`crate::protocol`] on its live core. One translation for the app and the test host: each
//! [`Action`] becomes one or more core [`Command`]s, run on whatever [`Host`] owns the core.
//!
//! The process also owns the socket ([`listen`]), a lock that keeps one process per app folder
//! ([`lock_instance`]), and quits on its own when started in the background with nothing to do
//! ([`IdlePolicy`]).

mod idle;
mod listener;
mod resolve;

use std::path::PathBuf;

use greffier_core::identifier::Identifiers;
use greffier_core::{Command, Event, Greffier, MeetingId, Status};

pub use idle::{IDLE_QUIT, IdlePolicy};
pub use listener::{InstanceLock, LockError, Listener, MAX_SOCKET_PATH, Reply, listen, lock_instance, lock_path, socket_path};

use crate::protocol::{Action, Done, Hello, LiveMeeting, Request, Resolution, Response, Snapshot, VERSION};
use crate::text;
use resolve::{meeting_named, number, speaker, utterance};

/// What owns the core and lives around it: the app (its window, its toasts) or the test host.
pub trait Host {
    fn core(&mut self) -> &mut Greffier;
    /// Started without a window (`--background`).
    fn background(&self) -> bool;
    /// The meetings folder the process works on, canonical; `None` until one is chosen. By
    /// default, the core's.
    fn meetings_folder(&mut self) -> Option<PathBuf> {
        let root = self.core().folder().root().to_path_buf();
        Some(root.canonicalize().unwrap_or(root))
    }
    /// The core's events from one request, once it is carried out: the meetings folder read
    /// again, then the action's (the app handles them as its own commands').
    fn executed(&mut self, _events: Vec<Event>) {}
    /// The meeting was moved to the trash (the app forgets its page, its video…).
    fn deleted(&mut self, _meeting: MeetingId) {}
    /// [`Request::OpenWindow`]: show the window.
    fn open_window(&mut self) {}
}

/// The answer to one request. A status or an action first reads the meetings folder again
/// ([`Greffier::rescan_folder`]): what the user typed is resolved against the folder as it is
/// now, as `greffier list` reads it, not as the folder watcher last reported it.
pub fn answer<H: Host + ?Sized>(host: &mut H, request: Request) -> Response {
    match request {
        Request::Hello => Response::Hello(Hello { version: VERSION, pid: std::process::id(), meetings_folder: host.meetings_folder(), background: host.background() }),
        Request::Status { meeting } => {
            let events = host.core().rescan_folder();
            if !events.is_empty() {
                host.executed(events);
            }
            match snapshot(host.core(), meeting.as_deref()) {
                Ok(s) => Response::Status(s),
                Err(message) => Response::Error { message },
            }
        }
        Request::OpenWindow => {
            host.open_window();
            Response::Ok(Done { message: "window opened".into(), ..Done::default() })
        }
        Request::Execute(action) => {
            let mut events = host.core().rescan_folder();
            let result = execute(host, action, &mut events);
            if !events.is_empty() {
                host.executed(events);
            }
            match result {
                Ok(done) => Response::Ok(done),
                Err(message) => Response::Error { message },
            }
        }
    }
}

/// The identifiers of the core's meetings (the recording's folder included).
fn identifiers(core: &Greffier) -> Identifiers {
    let recording = core.recording().map(|r| r.folder_name.as_str());
    Identifiers::new(core.meetings().iter().map(|m| m.folder_name.as_str()).chain(recording))
}

/// The live state: the recording, then the queue in its order, then `meeting` if asked for.
pub fn snapshot(core: &Greffier, meeting: Option<&str>) -> Result<Snapshot, String> {
    let identifiers = identifiers(core);
    let mut meetings = Vec::new();
    if let Some(r) = core.recording() {
        let identifier = identifiers.of(&r.folder_name).map(str::to_string);
        // Its title is the one its folder carries (the files' default until the summary names it).
        let title = greffier_core::folder::read_folder_name(&r.folder_name).map(|(_, t)| t).unwrap_or_default();
        meetings.push(LiveMeeting { identifier, folder_name: r.folder_name.clone(), title, status: (&Status::Recording).into(), rank: None, time_remaining: None });
    }
    let live = |m: &greffier_core::Meeting| LiveMeeting {
        identifier: identifiers.of(&m.folder_name).map(str::to_string),
        folder_name: m.folder_name.clone(),
        title: m.title.clone(),
        status: (&m.status).into(),
        rank: core.rank(m.id).map(|r| r + 1),
        time_remaining: core.time_remaining(m.id),
    };
    let mut queue: Vec<&greffier_core::Meeting> = core.meetings().iter().filter(|m| m.status.in_queue()).collect();
    queue.sort_by_key(|m| core.rank(m.id).unwrap_or(usize::MAX));
    meetings.extend(queue.into_iter().map(live));
    if let Some(query) = meeting {
        let id = meeting_named(core, query)?;
        if let Some(m) = core.meeting(id).filter(|m| !m.status.in_queue()) {
            meetings.push(live(m));
        }
    }
    Ok(Snapshot { busy: core.busy(), meetings })
}

/// Runs the core command; its events join the action's. A refusal comes back in English.
fn run<H: Host + ?Sized>(host: &mut H, command: Command, events: &mut Vec<Event>) -> Result<(), String> {
    events.extend(host.core().execute(command).map_err(|r| r.english())?);
    Ok(())
}

/// "2026-11-20-1000 · <what>", with the meeting's identifier and folder after the action.
fn done(core: &Greffier, meeting: MeetingId, what: impl FnOnce(&str) -> String) -> Done {
    let folder_name = core.meeting(meeting).map(|m| m.folder_name.clone());
    let identifier = folder_name.as_deref().and_then(|n| identifiers(core).of(n).map(str::to_string));
    let shown = identifier.clone().or_else(|| folder_name.clone()).unwrap_or_default();
    Done { message: what(&shown), meeting: identifier, folder_name }
}

/// "queued: Transcription", the stage the meeting waits for.
fn queued(core: &Greffier, meeting: MeetingId) -> String {
    match core.meeting(meeting).map(|m| &m.status) {
        Some(Status::Pending(stage) | Status::Running { stage, .. }) => format!("queued: {}", text::stage(*stage)),
        _ => "queued".into(),
    }
}

fn execute<H: Host + ?Sized>(host: &mut H, action: Action, events: &mut Vec<Event>) -> Result<Done, String> {
    match action {
        Action::Process { meeting, speakers } => {
            let id = meeting_named(host.core(), &meeting)?;
            match host.core().meeting(id).map(|m| &m.status) {
                Some(Status::Done) => return Err("the meeting is already processed (greffier redo to redo a stage)".into()),
                Some(s) if s.in_queue() => return Err("the meeting is already in the queue".into()),
                _ => {}
            }
            run(host, Command::Process(id), events)?;
            if let Some(n) = speakers {
                run(host, Command::SpeakerCount { meeting: id, number: Some(n) }, events)?;
            }
            let core = host.core();
            Ok(done(core, id, |m| format!("{m} · {}", queued(core, id))))
        }
        Action::Redo { meeting, stage } => {
            let id = meeting_named(host.core(), &meeting)?;
            run(host, Command::Redo { meeting: id, stage: stage.into() }, events)?;
            let core = host.core();
            Ok(done(core, id, |m| format!("{m} · {}", queued(core, id))))
        }
        Action::Stop { meeting } => {
            let id = meeting_named(host.core(), &meeting)?;
            run(host, Command::StopProcessing(id), events)?;
            Ok(done(host.core(), id, |m| format!("{m} · processing stopped")))
        }
        Action::Rename { meeting, title } => {
            let id = meeting_named(host.core(), &meeting)?;
            run(host, Command::ChangeTitle { meeting: id, title }, events)?;
            let core = host.core();
            let title = core.meeting(id).map(|m| m.title.clone()).unwrap_or_default();
            Ok(done(core, id, |m| format!("{m} · renamed: {title}")))
        }
        Action::NameSpeaker { meeting, speaker: who, name } => {
            let id = meeting_named(host.core(), &meeting)?;
            let (i, label) = speaker(host.core(), id, &who)?;
            run(host, Command::NameSpeaker { meeting: id, speaker: i, name: name.clone() }, events)?;
            Ok(done(host.core(), id, |m| format!("{m} · {label}: {}", name.trim())))
        }
        Action::MergeSpeaker { meeting, speaker: who, into } => {
            let id = meeting_named(host.core(), &meeting)?;
            let (i, label) = speaker(host.core(), id, &who)?;
            let (j, into_label) = speaker(host.core(), id, &into)?;
            run(host, Command::MergeSpeakers { meeting: id, speaker: i, into: j }, events)?;
            Ok(done(host.core(), id, |m| format!("{m} · {label} merged into {into_label}")))
        }
        Action::SplitSpeaker { meeting, speaker: who } => {
            let id = meeting_named(host.core(), &meeting)?;
            let (i, label) = speaker(host.core(), id, &who)?;
            run(host, Command::SplitSpeaker { meeting: id, speaker: i }, events)?;
            Ok(done(host.core(), id, |m| format!("{m} · {label} split")))
        }
        Action::Correct { meeting, utterance: which, text } => {
            let id = meeting_named(host.core(), &meeting)?;
            let i = utterance(host.core(), id, &which)?;
            run(host, Command::CorrectUtterance { meeting: id, utterance: i, text }, events)?;
            Ok(done(host.core(), id, |m| format!("{m} · utterance #{} corrected", i + 1)))
        }
        Action::Resolve { meeting, passage, resolution } => {
            let id = meeting_named(host.core(), &meeting)?;
            let count = host.core().meeting(id).map_or(0, |m| m.passages.len());
            if count == 0 {
                let name = done(host.core(), id, |m| m.to_string()).message;
                return Err(format!("no passages to check in {name}"));
            }
            let i = number(&passage, count).ok_or_else(|| format!("passage not found: \"{passage}\" (number from 1 to {count})"))?;
            let what = match &resolution {
                Resolution::Accept => "hypothesis accepted",
                Resolution::Keep => "original kept",
                Resolution::Reopen => "reopened",
                Resolution::Correct(_) => "corrected",
            };
            run(host, Command::Resolve { meeting: id, passage: i, resolution: resolution.into() }, events)?;
            Ok(done(host.core(), id, |m| format!("{m} · passage #{}: {what}", i + 1)))
        }
        Action::CheckAction { meeting, action, done: checked } => {
            let id = meeting_named(host.core(), &meeting)?;
            let count = host.core().meeting(id).map_or(0, |m| greffier_core::summary::checkbox_count(&m.summary));
            if count == 0 {
                let name = done(host.core(), id, |m| m.to_string()).message;
                return Err(format!("no actions in the summary of {name}"));
            }
            let i = number(&action, count).ok_or_else(|| format!("action not found: \"{action}\" (number from 1 to {count})"))?;
            run(host, Command::CheckAction { meeting: id, action: i, done: checked }, events)?;
            Ok(done(host.core(), id, |m| format!("{m} · action #{} {}", i + 1, if checked { "checked" } else { "unchecked" })))
        }
        Action::Delete { meeting } => {
            let id = meeting_named(host.core(), &meeting)?;
            // Said before: the meeting is gone after.
            let before = done(host.core(), id, |m| format!("{m} · moved to the trash"));
            run(host, Command::Delete(id), events)?;
            host.deleted(id);
            Ok(Done { meeting: None, folder_name: None, ..before })
        }
        Action::Import { files, title, speakers, process } => {
            let mut import = host.core().propose_import(&files).map_err(|r| r.english())?;
            if let Some(t) = title {
                if t.trim().is_empty() {
                    return Err("the title is empty".into());
                }
                import.title = t.trim().to_string();
            }
            let before = events.len();
            run(host, Command::Import(import), events)?;
            let id = events[before..].iter().find_map(|e| if let Event::MeetingImported(id) = e { Some(*id) } else { None }).ok_or("the import created no meeting")?;
            if let Some(n) = speakers {
                run(host, Command::SpeakerCount { meeting: id, number: Some(n) }, events)?;
            }
            let to_process = host.core().meeting(id).is_some_and(|m| m.status == Status::ToProcess);
            if process && to_process {
                run(host, Command::Process(id), events)?;
            }
            let core = host.core();
            let queued = core.meeting(id).is_some_and(|m| m.status.in_queue()).then(|| queued(core, id));
            Ok(done(core, id, |m| match queued {
                Some(q) => format!("{m} · imported, {q}"),
                None => format!("{m} · imported"),
            }))
        }
    }
}
