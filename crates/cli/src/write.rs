//! The commands that write (`import`, `process`, `redo`, `stop`, `rename`, `speaker`,
//! `correct`, `resolve`, `check-action`, `delete`): each becomes one [`Action`] sent to the
//! Greffier process, which is launched without a window if it does not run (ADR 0006). The
//! command line resolves nothing itself: the process reads what the user typed against its live
//! state. `--wait` then follows the processing until the meeting leaves the queue.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::args::{self, Args};
use crate::client::Connection;
use crate::protocol::{Action, Done, LiveMeeting, LiveStatus, Request, Resolution, Response, Stage};
use crate::{Context, Failure, HELP, text};

/// The commands of this module.
pub(crate) const COMMANDS: [&str; 10] = ["import", "process", "redo", "stop", "rename", "speaker", "correct", "resolve", "check-action", "delete"];

/// What a command asks: the action, whether to follow the processing it starts, and in JSON.
struct Asked {
    action: Action,
    wait: Option<Wait>,
    json: bool,
}

/// `--wait [--timeout S]`.
#[derive(Clone, Copy)]
struct Wait {
    timeout: Option<f64>,
}

fn usage(m: impl Into<String>) -> Failure {
    Failure::Usage(m.into())
}

/// The positional argument `i`, or "missing <what>".
fn nth(a: &Args, i: usize, what: &str) -> Result<String, Failure> {
    a.positionals.get(i).cloned().ok_or_else(|| usage(format!("missing {what}")))
}

/// The positional arguments from `i` on, joined by spaces (a title, a name, a text).
fn rest(a: &Args, i: usize, what: &str) -> Result<String, Failure> {
    let words = a.positionals.get(i..).unwrap_or_default();
    if words.is_empty() {
        return Err(usage(format!("missing {what}")));
    }
    Ok(words.join(" "))
}

fn speakers(a: &Args) -> Result<Option<u8>, Failure> {
    a.value("--speakers").map(|n| n.parse::<u8>().ok().filter(|n| *n >= 1).ok_or_else(|| usage(format!("--speakers needs a number of speakers: \"{n}\"")))).transpose()
}

fn wait(a: &Args) -> Result<Option<Wait>, Failure> {
    let timeout = a.value("--timeout").map(|t| t.parse::<f64>().ok().filter(|t| t.is_finite() && *t >= 0.0).ok_or_else(|| usage(format!("--timeout needs seconds: \"{t}\"")))).transpose()?;
    match (a.flag("--wait"), timeout) {
        (true, timeout) => Ok(Some(Wait { timeout })),
        (false, Some(_)) => Err(usage("--timeout goes with --wait")),
        (false, None) => Ok(None),
    }
}

/// Reads a command's arguments into what it asks.
fn parse(command: &str, args: &[String]) -> Result<Option<Asked>, Failure> {
    let (flags, valued): (&[&'static str], &[&'static str]) = match command {
        "import" => (&["--json", "--no-process", "--wait"], &["--title", "--speakers", "--timeout"]),
        "process" => (&["--json", "--wait"], &["--speakers", "--timeout"]),
        "redo" => (&["--json", "--wait"], &["--timeout"]),
        "check-action" => (&["--json", "--undo"], &[]),
        "delete" => (&["--json", "--yes"], &[]),
        _ => (&["--json"], &[]),
    };
    let a = args::parse(args, flags, valued)?;
    if a.help() {
        return Ok(None);
    }
    let meeting = || nth(&a, 0, "the meeting").map(sent);
    let action = match command {
        "import" => {
            if a.positionals.is_empty() {
                return Err(usage("missing the file to import"));
            }
            if a.flag("--no-process") && a.flag("--wait") {
                return Err(usage("--wait does not go with --no-process"));
            }
            let files = a.positionals.iter().map(|f| absolute(f)).collect::<Result<_, _>>()?;
            Action::Import { files, title: a.value("--title").map(str::to_string), speakers: speakers(&a)?, process: !a.flag("--no-process") }
        }
        "process" => {
            a.at_most(1)?;
            Action::Process { meeting: meeting()?, speakers: speakers(&a)? }
        }
        "redo" => {
            a.at_most(2)?;
            let stage = match nth(&a, 1, "the stage (transcription, review or summary)")?.as_str() {
                "transcription" => Stage::Transcription,
                "review" => Stage::Review,
                "summary" => Stage::Summary,
                other => return Err(usage(format!("unknown stage: \"{other}\" (transcription, review or summary)"))),
            };
            Action::Redo { meeting: meeting()?, stage }
        }
        "stop" => {
            a.at_most(1)?;
            Action::Stop { meeting: meeting()? }
        }
        "rename" => Action::Rename { meeting: meeting()?, title: rest(&a, 1, "the title")? },
        "speaker" => {
            let what = nth(&a, 0, "name, merge or split")?;
            let meeting = sent(nth(&a, 1, "the meeting")?);
            let speaker = nth(&a, 2, "the speaker")?;
            match what.as_str() {
                "name" => Action::NameSpeaker { meeting, speaker, name: rest(&a, 3, "the name")? },
                "merge" => {
                    a.at_most(4)?;
                    Action::MergeSpeaker { meeting, speaker, into: nth(&a, 3, "the speaker to merge into")? }
                }
                "split" => {
                    a.at_most(3)?;
                    Action::SplitSpeaker { meeting, speaker }
                }
                other => return Err(usage(format!("greffier speaker {other}: name, merge or split"))),
            }
        }
        "correct" => Action::Correct { meeting: meeting()?, utterance: nth(&a, 1, "the utterance")?, text: rest(&a, 2, "the text")? },
        "resolve" => {
            let passage = nth(&a, 1, "the passage")?;
            let word = nth(&a, 2, "accept, keep, reopen or correct")?;
            let resolution = match word.as_str() {
                "correct" => Resolution::Correct(rest(&a, 3, "the correction's text")?),
                "accept" | "keep" | "reopen" => {
                    a.at_most(3)?;
                    match word.as_str() {
                        "accept" => Resolution::Accept,
                        "keep" => Resolution::Keep,
                        _ => Resolution::Reopen,
                    }
                }
                other => return Err(usage(format!("\"{other}\": accept, keep, reopen or correct <text>"))),
            };
            Action::Resolve { meeting: meeting()?, passage, resolution }
        }
        "check-action" => {
            a.at_most(2)?;
            Action::CheckAction { meeting: meeting()?, action: nth(&a, 1, "the action's number")?, done: !a.flag("--undo") }
        }
        "delete" => {
            a.at_most(1)?;
            let meeting = meeting()?;
            if !a.flag("--yes") {
                return Err(usage(format!("greffier delete moves the meeting to the trash: confirm with greffier delete {meeting} --yes")));
            }
            Action::Delete { meeting }
        }
        other => return Err(usage(format!("unknown command: {other}"))),
    };
    let wait = if matches!(command, "import" | "process" | "redo") { wait(&a)? } else { None };
    Ok(Some(Asked { action, wait, json: a.flag("--json") }))
}

/// A meeting as typed, for the process; a path made absolute, as the process does not share
/// this folder (it then checks that the path leads into its meetings folder).
fn sent(meeting: String) -> String {
    if !meeting.contains('/') {
        return meeting;
    }
    std::path::absolute(meeting.trim()).map_or(meeting, |p| p.to_string_lossy().into_owned())
}

/// A file named on the command line, absolute: the process does not share its folder.
fn absolute(file: &str) -> Result<PathBuf, Failure> {
    Path::new(file).canonicalize().map_err(|_| Failure::Failed(format!("file not found: {file}")))
}

/// What a command that wrote prints in JSON.
#[derive(Serialize)]
struct DoneJson<'a> {
    identifier: Option<&'a str>,
    /// The meeting's folder, absolute.
    folder: Option<String>,
    message: &'a str,
}

/// Where a waited meeting ended, in JSON.
#[derive(Serialize)]
struct EndJson<'a> {
    identifier: &'a str,
    status: &'static str,
    title: &'a str,
}

pub(crate) fn run(cx: &Context, command: &str, args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> Result<String, Failure> {
    let Some(asked) = parse(command, args)? else { return Ok(HELP.into()) };
    let json = asked.json;
    let Some(folder) = &cx.meetings_folder else {
        return Err(Failure::Failed(crate::reading::NO_MEETINGS_FOLDER.into()));
    };
    if !folder.is_dir() {
        return Err(Failure::Failed(format!("meetings folder not found: {}", folder.display())));
    }
    let Some(process) = &cx.process else { return Err(Failure::Unreachable("Greffier cannot be reached from here.".into())) };
    let mut c = process.reach(folder)?;
    let import = matches!(asked.action, Action::Import { .. });
    let done = match c.ask(&Request::Execute(asked.action))? {
        Response::Ok(done) => done,
        Response::Error { message } => return Err(Failure::Failed(message)),
        other => return Err(Failure::Unreachable(format!("unexpected answer from Greffier: {other:?}"))),
    };
    let printed = if json {
        let folder = done.folder_name.as_ref().map(|n| folder.join(n).to_string_lossy().into_owned());
        crate::json(&DoneJson { identifier: done.meeting.as_deref(), folder, message: &done.message })
    } else if import {
        // The new meeting's identifier alone, for a script to take.
        format!("{}\n", done.meeting.as_deref().or(done.folder_name.as_deref()).unwrap_or_default())
    } else {
        format!("{}\n", done.message)
    };
    let Some(w) = asked.wait else { return Ok(printed) };
    if import {
        // Said at once, before the wait.
        let _ = out.write_all(printed.as_bytes());
        let _ = out.flush();
    }
    Ok(follow(&mut c, &done, w, process.poll, err)?.render(json))
}

/// A waited meeting, once processed.
struct End {
    identifier: String,
    title: String,
}

impl End {
    fn render(&self, json: bool) -> String {
        if json {
            return crate::json(&EndJson { identifier: &self.identifier, status: "done", title: &self.title });
        }
        format!("{} · done · {}\n", self.identifier, self.title)
    }
}

/// "queued", "Transcription": where a queued meeting stands, one line per change.
fn phase(s: &LiveStatus) -> String {
    match s {
        LiveStatus::Pending { .. } => "queued".into(),
        LiveStatus::Running { stage, .. } => text::stage((*stage).into()).into(),
        _ => String::new(),
    }
}

/// "Transcription 40 %", "2nd in queue".
fn standing(m: &LiveMeeting) -> String {
    match &m.status {
        LiveStatus::Running { stage, progress } => format!("{} {:.0} %", text::stage((*stage).into()), progress * 100.0),
        LiveStatus::Pending { .. } => m.rank.map_or_else(|| "queued".into(), |r| format!("{} in queue", text::ordinal(r))),
        _ => phase(&m.status),
    }
}

/// Asks where the meeting stands every `poll` until it leaves the queue: done, failed (exit
/// 1), stopped (exit 1); or the timeout elapses (exit 4). Each change of stage goes to `err`.
fn follow(c: &mut Connection, done: &Done, w: Wait, poll: Duration, err: &mut dyn Write) -> Result<End, Failure> {
    let identifier = done.meeting.clone().or_else(|| done.folder_name.clone()).ok_or_else(|| Failure::Failed("meeting without an identifier: its processing cannot be followed".into()))?;
    let start = Instant::now();
    let mut last = String::new();
    loop {
        let snapshot = match c.ask(&Request::Status { meeting: Some(identifier.clone()) })? {
            Response::Status(s) => s,
            Response::Error { message } => return Err(Failure::Failed(message)),
            other => return Err(Failure::Unreachable(format!("unexpected answer from Greffier: {other:?}"))),
        };
        let Some(m) = snapshot.meetings.iter().find(|m| m.identifier.as_deref() == Some(identifier.as_str()) || m.folder_name == identifier) else {
            return Err(Failure::Failed(format!("{identifier} is no longer in the meetings folder")));
        };
        match &m.status {
            LiveStatus::Done => return Ok(End { identifier, title: m.title.clone() }),
            LiveStatus::Failed { stage, message } => return Err(Failure::Failed(format!("{identifier} · failed: {}: {message}", text::stage((*stage).into())))),
            LiveStatus::ToProcess | LiveStatus::RecordingInterrupted | LiveStatus::Recording => return Err(Failure::Failed(format!("{identifier} · processing stopped"))),
            LiveStatus::Pending { .. } | LiveStatus::Running { .. } => {}
        }
        let now = phase(&m.status);
        if now != last {
            let _ = writeln!(err, "{identifier} · {now}");
            last = now;
        }
        let elapsed = start.elapsed().as_secs_f64();
        if let Some(t) = w.timeout
            && elapsed >= t
        {
            return Err(Failure::StillRunning(format!("{identifier} still processing after {t} s: {}", standing(m))));
        }
        let left = w.timeout.map_or(poll, |t| poll.min(Duration::from_secs_f64((t - elapsed).max(0.0))));
        std::thread::sleep(left);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn action(args: &[&str]) -> Action {
        let rest: Vec<String> = args[1..].iter().map(|a| a.to_string()).collect();
        match parse(args[0], &rest) {
            Ok(Some(asked)) => asked.action,
            _ => panic!("{args:?}"),
        }
    }

    #[test]
    fn a_relative_path_to_a_meeting_is_sent_absolute() {
        // The process does not run in this folder.
        let here = std::env::current_dir().unwrap();
        let rename = |meeting: &str| Action::Rename { meeting: meeting.into(), title: "Revue".into() };
        assert_eq!(action(&["rename", "Réunions/2026-11-20 10h00 - Revue", "Revue"]), rename(&here.join("Réunions/2026-11-20 10h00 - Revue").to_string_lossy()));
        assert_eq!(action(&["rename", "/Réunions/2026-11-20 10h00 - Revue", "Revue"]), rename("/Réunions/2026-11-20 10h00 - Revue"));
        assert_eq!(action(&["rename", "2026-11-20", "Revue"]), rename("2026-11-20"));
        let Action::NameSpeaker { meeting, .. } = action(&["speaker", "name", "./2026-11-20 10h00 - Revue", "0", "Alex"]) else { panic!() };
        assert_eq!(Path::new(&meeting), here.join("2026-11-20 10h00 - Revue"));
    }
}
