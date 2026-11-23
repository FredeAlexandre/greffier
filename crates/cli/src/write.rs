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

/// What a command asks: the action, and whether to follow the processing it starts.
struct Asked {
    action: Action,
    wait: Option<Wait>,
}

/// `--wait [--timeout S]`.
#[derive(Clone, Copy)]
struct Wait {
    timeout: Option<f64>,
}

fn usage(m: impl Into<String>) -> Failure {
    Failure::Usage(m.into())
}

/// The positional argument `i`, or « il manque <what> ».
fn nth(a: &Args, i: usize, what: &str) -> Result<String, Failure> {
    a.positionals.get(i).cloned().ok_or_else(|| usage(format!("il manque {what}")))
}

/// The positional arguments from `i` on, joined by spaces (a title, a name, a text).
fn rest(a: &Args, i: usize, what: &str) -> Result<String, Failure> {
    let words = a.positionals.get(i..).unwrap_or_default();
    if words.is_empty() {
        return Err(usage(format!("il manque {what}")));
    }
    Ok(words.join(" "))
}

fn speakers(a: &Args) -> Result<Option<u8>, Failure> {
    a.value("--speakers").map(|n| n.parse::<u8>().ok().filter(|n| *n >= 1).ok_or_else(|| usage(format!("--speakers attend un nombre de locuteurs : « {n} »")))).transpose()
}

fn wait(a: &Args) -> Result<Option<Wait>, Failure> {
    let timeout = a.value("--timeout").map(|t| t.parse::<f64>().ok().filter(|t| t.is_finite() && *t >= 0.0).ok_or_else(|| usage(format!("--timeout attend des secondes : « {t} »")))).transpose()?;
    match (a.flag("--wait"), timeout) {
        (true, timeout) => Ok(Some(Wait { timeout })),
        (false, Some(_)) => Err(usage("--timeout va avec --wait")),
        (false, None) => Ok(None),
    }
}

/// Reads a command's arguments into what it asks.
fn parse(command: &str, args: &[String]) -> Result<Option<Asked>, Failure> {
    let (flags, valued): (&[&'static str], &[&'static str]) = match command {
        "import" => (&["--json", "--no-process", "--wait"], &["--titre", "--speakers", "--timeout"]),
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
    let meeting = || nth(&a, 0, "la réunion").map(sent);
    let action = match command {
        "import" => {
            if a.positionals.is_empty() {
                return Err(usage("il manque le fichier à importer"));
            }
            if a.flag("--no-process") && a.flag("--wait") {
                return Err(usage("--wait ne va pas avec --no-process"));
            }
            let files = a.positionals.iter().map(|f| absolute(f)).collect::<Result<_, _>>()?;
            Action::Import { files, title: a.value("--titre").map(str::to_string), speakers: speakers(&a)?, process: !a.flag("--no-process") }
        }
        "process" => {
            a.at_most(1)?;
            Action::Process { meeting: meeting()?, speakers: speakers(&a)? }
        }
        "redo" => {
            a.at_most(2)?;
            let stage = match nth(&a, 1, "l'étape (transcription, relecture ou bilan)")?.as_str() {
                "transcription" => Stage::Transcription,
                "relecture" => Stage::Review,
                "bilan" => Stage::Summary,
                other => return Err(usage(format!("étape inconnue : « {other} » (transcription, relecture ou bilan)"))),
            };
            Action::Redo { meeting: meeting()?, stage }
        }
        "stop" => {
            a.at_most(1)?;
            Action::Stop { meeting: meeting()? }
        }
        "rename" => Action::Rename { meeting: meeting()?, title: rest(&a, 1, "le titre")? },
        "speaker" => {
            let what = nth(&a, 0, "name, merge ou split")?;
            let meeting = sent(nth(&a, 1, "la réunion")?);
            let speaker = nth(&a, 2, "le locuteur")?;
            match what.as_str() {
                "name" => Action::NameSpeaker { meeting, speaker, name: rest(&a, 3, "le nom")? },
                "merge" => {
                    a.at_most(4)?;
                    Action::MergeSpeaker { meeting, speaker, into: nth(&a, 3, "le locuteur dans lequel fusionner")? }
                }
                "split" => {
                    a.at_most(3)?;
                    Action::SplitSpeaker { meeting, speaker }
                }
                other => return Err(usage(format!("greffier speaker {other} : name, merge ou split"))),
            }
        }
        "correct" => Action::Correct { meeting: meeting()?, utterance: nth(&a, 1, "la réplique")?, text: rest(&a, 2, "le texte")? },
        "resolve" => {
            let passage = nth(&a, 1, "le passage")?;
            let word = nth(&a, 2, "accepter, garder, rouvrir ou corriger")?;
            let resolution = match word.as_str() {
                "corriger" => Resolution::Correct(rest(&a, 3, "le texte de la correction")?),
                "accepter" | "garder" | "rouvrir" => {
                    a.at_most(3)?;
                    match word.as_str() {
                        "accepter" => Resolution::Accept,
                        "garder" => Resolution::Keep,
                        _ => Resolution::Reopen,
                    }
                }
                other => return Err(usage(format!("« {other} » : accepter, garder, rouvrir ou corriger <texte>"))),
            };
            Action::Resolve { meeting: meeting()?, passage, resolution }
        }
        "check-action" => {
            a.at_most(2)?;
            Action::CheckAction { meeting: meeting()?, action: nth(&a, 1, "le numéro de l'action")?, done: !a.flag("--undo") }
        }
        "delete" => {
            a.at_most(1)?;
            let meeting = meeting()?;
            if !a.flag("--yes") {
                return Err(usage(format!("greffier delete met la réunion à la corbeille : confirmez avec greffier delete {meeting} --yes")));
            }
            Action::Delete { meeting }
        }
        other => return Err(usage(format!("commande inconnue : {other}"))),
    };
    let wait = if matches!(command, "import" | "process" | "redo") { wait(&a)? } else { None };
    Ok(Some(Asked { action, wait }))
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
    Path::new(file).canonicalize().map_err(|_| Failure::Failed(format!("fichier introuvable : {file}")))
}

/// What a command that wrote prints in JSON.
#[derive(Serialize)]
struct DoneJson<'a> {
    id: Option<&'a str>,
    dossier: Option<String>,
    message: &'a str,
}

/// Where a waited meeting ended, in JSON.
#[derive(Serialize)]
struct EndJson<'a> {
    id: &'a str,
    statut: &'static str,
    titre: &'a str,
}

pub(crate) fn run(cx: &Context, command: &str, args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> Result<String, Failure> {
    let json = args.iter().any(|a| a == "--json");
    let Some(asked) = parse(command, args)? else { return Ok(HELP.into()) };
    let Some(folder) = &cx.meetings_folder else {
        return Err(Failure::Failed("aucun dossier de réunions : ouvrez Greffier une fois pour le choisir, ou définissez GREFFIER_FOLDER.".into()));
    };
    if !folder.is_dir() {
        return Err(Failure::Failed(format!("dossier de réunions introuvable : {}", folder.display())));
    }
    let Some(process) = &cx.process else { return Err(Failure::Unreachable("Greffier n'est pas joignable d'ici.".into())) };
    let mut c = process.reach(folder)?;
    let import = matches!(asked.action, Action::Import { .. });
    let done = match c.ask(&Request::Execute(asked.action))? {
        Response::Ok(done) => done,
        Response::Error { message } => return Err(Failure::Failed(message)),
        other => return Err(Failure::Unreachable(format!("réponse inattendue de Greffier : {other:?}"))),
    };
    let printed = if json {
        let dossier = done.folder_name.as_ref().map(|n| folder.join(n).to_string_lossy().into_owned());
        crate::json(&DoneJson { id: done.meeting.as_deref(), dossier, message: &done.message })
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
    id: String,
    title: String,
}

impl End {
    fn render(&self, json: bool) -> String {
        if json {
            return crate::json(&EndJson { id: &self.id, statut: "terminee", titre: &self.title });
        }
        format!("{} · terminée · {}\n", self.id, self.title)
    }
}

/// « en attente », « Transcription »: where a queued meeting stands, one line per change.
fn phase(s: &LiveStatus) -> String {
    match s {
        LiveStatus::Pending { .. } => "en attente".into(),
        LiveStatus::Running { stage, .. } => text::stage((*stage).into()).into(),
        _ => String::new(),
    }
}

/// « Transcription 40 % », « 2ᵉ dans la file ».
fn standing(m: &LiveMeeting) -> String {
    match &m.status {
        LiveStatus::Running { stage, progress } => format!("{} {:.0} %", text::stage((*stage).into()), progress * 100.0),
        LiveStatus::Pending { .. } => m.rank.map_or_else(|| "en attente".into(), |r| format!("{r}ᵉ dans la file")),
        _ => phase(&m.status),
    }
}

/// Asks where the meeting stands every `poll` until it leaves the queue: done, failed (exit
/// 1), stopped (exit 1); or the timeout elapses (exit 4). Each change of stage goes to `err`.
fn follow(c: &mut Connection, done: &Done, w: Wait, poll: Duration, err: &mut dyn Write) -> Result<End, Failure> {
    let id = done.meeting.clone().or_else(|| done.folder_name.clone()).ok_or_else(|| Failure::Failed("réunion sans identifiant : impossible de suivre son traitement".into()))?;
    let start = Instant::now();
    let mut last = String::new();
    loop {
        let snapshot = match c.ask(&Request::Status { meeting: Some(id.clone()) })? {
            Response::Status(s) => s,
            Response::Error { message } => return Err(Failure::Failed(message)),
            other => return Err(Failure::Unreachable(format!("réponse inattendue de Greffier : {other:?}"))),
        };
        let Some(m) = snapshot.meetings.iter().find(|m| m.id.as_deref() == Some(id.as_str()) || m.folder_name == id) else {
            return Err(Failure::Failed(format!("{id} n'est plus dans le dossier de réunions")));
        };
        match &m.status {
            LiveStatus::Done => return Ok(End { id, title: m.title.clone() }),
            LiveStatus::Failed { stage, message } => return Err(Failure::Failed(format!("{id} · échec : {} : {message}", text::stage((*stage).into())))),
            LiveStatus::ToProcess | LiveStatus::RecordingInterrupted | LiveStatus::Recording => return Err(Failure::Failed(format!("{id} · traitement arrêté"))),
            LiveStatus::Pending { .. } | LiveStatus::Running { .. } => {}
        }
        let now = phase(&m.status);
        if now != last {
            let _ = writeln!(err, "{id} · {now}");
            last = now;
        }
        let elapsed = start.elapsed().as_secs_f64();
        if let Some(t) = w.timeout
            && elapsed >= t
        {
            return Err(Failure::StillRunning(format!("{id} toujours en cours après {t} s : {}", standing(m))));
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
