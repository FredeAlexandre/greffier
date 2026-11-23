//! Greffier's command line (ADR 0006), for an agent acting for the user: `greffier <command>`.
//! The app's binary hands its arguments here when the first one is a command ([`is_command`]),
//! and opens the window otherwise.
//!
//! Reading (`search`, `list`, `show`, `status`) goes to the meetings folder (and the app's search
//! cache) through the core, never launches the app and never writes anything; `status` takes the
//! live data (queue rank, progress, time remaining) from a Greffier process already running,
//! through a [`LiveSource`].
//! Writing (`import`, `process`, `redo`, `stop`, `rename`, `speaker`, `correct`, `resolve`,
//! `check-action`, `delete`) is sent to the Greffier process over its socket ([`client`],
//! [`protocol`]), which is launched without a window if nobody answers; [`server`] is that
//! process's side, shared by the app and the tests' host (`greffier-test-server`). The crate
//! depends on the core alone (no Rune, no whisper.cpp), so its logic is tested without linking
//! the UI, and the app only dispatches to it.
//!
//! The output is for agents, in English with the glossary's terms (meeting content is printed as
//! is): compact, one line per item, no colour, no padding. Errors go to stderr, prefixed
//! `greffier: `. Exit codes: 0 success, 1 the command failed (unknown or
//! ambiguous meeting, missing file, refused by Greffier…), 2 usage error (unknown option,
//! missing argument), 3 Greffier out of reach (does not answer, or works on another meetings
//! folder), 4 `--timeout` elapsed while still processing. `--json` gives the same data in JSON;
//! the shapes are in `README.md`.

#![forbid(unsafe_code)]

mod args;
pub mod client;
mod list;
pub mod protocol;
mod reading;
mod search;
pub mod server;
mod show;
mod status;
mod text;
mod write;

use std::io::Write;
use std::path::PathBuf;

use greffier_core::Status;

/// What the running Greffier process knows of a meeting that its files do not.
#[derive(Clone, Debug, PartialEq)]
pub struct Live {
    /// The status, with the progress of a running stage.
    pub status: Status,
    /// Its place in the processing queue, from 1.
    pub rank: Option<usize>,
    /// The time remaining until it is processed, in seconds (the meetings ahead included).
    pub time_remaining: Option<f64>,
}

/// Where live data comes from: the Greffier process, once the command line talks to it.
pub trait LiveSource {
    /// What the process knows of the meeting in this folder; `None`: nothing beyond its files.
    fn live(&self, folder_name: &str) -> Option<Live>;
}

/// Where the command line works.
pub struct Context {
    /// `None` until a meetings folder is chosen.
    pub meetings_folder: Option<PathBuf>,
    /// The app folder, whose search cache `search` reads (never writes); `None`: no cache, the
    /// index is built in memory.
    pub app_folder: Option<PathBuf>,
    /// Live data given as is (tests); otherwise `status` asks the process, if it runs.
    pub live: Option<Box<dyn LiveSource>>,
    /// The Greffier process, for the commands that write and for `status`'s live data; `None`:
    /// out of reach (writing fails with exit code 3, reading uses the files alone).
    pub process: Option<client::Process>,
}

impl Context {
    /// The meetings folder and the app folder of the settings (or `GREFFIER_FOLDER` and
    /// `GREFFIER_APP_FOLDER`), and the Greffier process of that app folder.
    pub fn from_env() -> Context {
        Context { meetings_folder: greffier_core::settings::meetings_folder(), app_folder: Some(greffier_core::settings::app_folder()), live: None, process: Some(client::Process::from_env()) }
    }
}

/// Whether these arguments (without the program's name) are a command for the command line,
/// rather than the window's (Rune's `--snap`, `--scene`, `--list-scenes`…): the first one is a
/// word, known or not (a mistyped command must fail, not open the window), or `--help`/`-h`.
pub fn is_command(args: &[String]) -> bool {
    args.first().is_some_and(|a| !a.starts_with('-') || a == "--help" || a == "-h")
}

/// Runs the command in `args` (without the program's name) with the meetings folder of the
/// settings; returns the exit code.
pub fn run(args: &[String], out: &mut impl Write, err: &mut impl Write) -> i32 {
    run_with(&Context::from_env(), args, out, err)
}

/// Runs the command in `args` in this context; returns the exit code.
pub fn run_with(cx: &Context, args: &[String], out: &mut impl Write, err: &mut impl Write) -> i32 {
    let (code, text) = match dispatch(cx, args, out, err) {
        Ok(text) => {
            let _ = out.write_all(text.as_bytes());
            return 0;
        }
        Err(Failure::Failed(m)) => (1, m),
        Err(Failure::Usage(m)) => (2, format!("{m} (see greffier help)")),
        Err(Failure::Unreachable(m)) => (3, m),
        Err(Failure::StillRunning(m)) => (4, m),
    };
    let _ = writeln!(err, "greffier: {text}");
    code
}

/// Why a command did not give its output.
pub(crate) enum Failure {
    /// The command failed: exit code 1.
    Failed(String),
    /// The command was misused: exit code 2.
    Usage(String),
    /// The Greffier process did not answer, or works on another meetings folder: exit code 3.
    Unreachable(String),
    /// `--timeout` elapsed while the meeting was still processing: exit code 4.
    StillRunning(String),
}

fn dispatch(cx: &Context, args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> Result<String, Failure> {
    let (command, rest) = args.split_first().map_or(("help", &[][..]), |(c, r)| (c.as_str(), r));
    match command {
        "list" => list::run(cx, rest),
        "show" => show::run(cx, rest),
        "status" => status::run(cx, rest),
        "search" => search::run(cx, rest),
        "help" | "--help" | "-h" => Ok(HELP.into()),
        c if write::COMMANDS.contains(&c) => write::run(cx, c, rest, out, err),
        other => Err(Failure::Usage(format!("unknown command: {other}"))),
    }
}

pub(crate) const HELP: &str = "\
greffier: the meetings of the meetings folder, on the command line. Without a command, opens Greffier.

greffier search <query> [--since D] [--until D] [--date D|M] [--speaker NAME] [--in WHERE] [--context N] [--limit N]
  What was said: the lines that have all the words and \"exact phrases\" (case and accents
  ignored; a word finds the words that contain it: bookmark finds bookmarks, type the stem),
  by meeting, most recent first: summary then utterances, [h:mm:ss] Name: excerpt.
  --in summary,transcription,title. --context N: N utterances around. --limit: 30 in all.
greffier list [--date D|M] [--since D] [--until D] [--limit N]
  The meetings, most recent first: identifier · duration · status · title.
greffier show <meeting> [--summary | --transcription | --sheet | --speakers | --actions | --passages]
  Without a view: header, brief, topics, decisions. --summary: the whole summary. --sheet: the sheet.
  --transcription [--from T] [--to T] [--numbers]: the utterances, [h:mm:ss] Name: text.
  --speakers: index · label · name · speaking time. --actions: the summary's checkboxes, numbered.
  --passages [--from T] [--to T]: the passages to check, numbered.
greffier status [<meeting>]
  Without a meeting: the meetings not done. With one: status, completed stage, error.
greffier help

These commands go through Greffier, started without a window if it is not running:
greffier import <files…> [--title T] [--speakers N] [--no-process] [--wait [--timeout S]]
  Imports a media file (with its transcription, its summary) and queues it; prints the identifier.
greffier process <meeting> [--speakers N] [--wait [--timeout S]]
greffier redo <meeting> transcription|review|summary [--wait [--timeout S]]
greffier stop <meeting>
greffier rename <meeting> <title…>
greffier speaker name <meeting> <speaker> <name…> | merge <meeting> <speaker> <into> | split <meeting> <speaker>
greffier correct <meeting> <utterance> <text…>
greffier resolve <meeting> <passage> accept|keep|reopen|correct <text…>
greffier check-action <meeting> <n> [--undo]
greffier delete <meeting> --yes
  --wait waits for the meeting to leave the queue (done or failed), --timeout S for S seconds at most.
  After --, everything is text: greffier correct 2026-11-20-0939 3 -- -5 degrees.

<meeting>: identifier (2026-11-20-0939), folder name or path (inside the meetings folder),
  date (2026-11-20) or start of an identifier.
<speaker>: index (0), label (SPEAKER_00) or displayed name. <utterance>: number (#3) or instant (h:mm:ss).
<passage>, <n>: number, as show gives it (#1).
D: YYYY-MM-DD. M: YYYY-MM. T: h:mm:ss, m:ss or seconds.
--json on every command. Exit: 0 success, 1 failure, 2 usage error, 3 Greffier out of reach,
4 --timeout elapsed before the processing ended.
GREFFIER_FOLDER replaces the meetings folder, GREFFIER_APP_FOLDER the app folder.

Examples:
  greffier search bookmark label --since 2026-11-16
  greffier search '\"fiscal year\"' --speaker Khalid --context 2
  greffier show 2026-11-20-0939 --transcription --from 1:25:00 --to 1:30:00
";

/// `value` in JSON, on one line.
pub(crate) fn json(value: &impl serde::Serialize) -> String {
    let mut s = serde_json::to_string(value).unwrap_or_else(|e| format!("{{\"error\":\"{e}\"}}"));
    s.push('\n');
    s
}
