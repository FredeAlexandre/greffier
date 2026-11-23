//! Greffier's command line (ADR 0006), for an agent acting for the user: `greffier <command>`.
//! The app's binary hands its arguments here when the first one is a command ([`is_command`]),
//! and opens the window otherwise.
//!
//! This crate is the reading side: `search`, `list`, `show`, `status` read the meetings folder
//! (and the app's search cache) through the core, never launch the app and never write anything. It depends on the core alone (no
//! Rune, no whisper.cpp), so its logic is tested without linking the UI, and the app only
//! dispatches to it. The process's live data (queue rank, progress, time remaining) will come
//! through a [`LiveSource`]; for now everything is read from disk.
//!
//! The output is for agents: compact, one line per item, no colour, no padding. Errors go to
//! stderr, prefixed `greffier : `. Exit codes: 0 success, 1 the command failed (unknown or
//! ambiguous meeting, missing file…), 2 usage error (unknown option, missing argument).
//! `--json` gives the same data in JSON; the shapes are in `README.md`.

#![forbid(unsafe_code)]

mod args;
mod library;
mod list;
pub mod protocol;
mod search;
pub mod server;
mod show;
mod status;
mod text;

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
    pub live: Option<Box<dyn LiveSource>>,
}

impl Context {
    /// The meetings folder and the app folder of the settings (or `GREFFIER_FOLDER` and
    /// `GREFFIER_APP_FOLDER`), without live data.
    pub fn from_env() -> Context {
        Context { meetings_folder: greffier_core::settings::meetings_folder(), app_folder: Some(greffier_core::settings::app_folder()), live: None }
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
    let (code, text) = match dispatch(cx, args) {
        Ok(text) => {
            let _ = out.write_all(text.as_bytes());
            return 0;
        }
        Err(Failure::Failed(m)) => (1, m),
        Err(Failure::Usage(m)) => (2, format!("{m} (voir greffier help)")),
    };
    let _ = writeln!(err, "greffier : {text}");
    code
}

/// Why a command did not give its output.
pub(crate) enum Failure {
    /// The command failed: exit code 1.
    Failed(String),
    /// The command was misused: exit code 2.
    Usage(String),
}

fn dispatch(cx: &Context, args: &[String]) -> Result<String, Failure> {
    let (command, rest) = args.split_first().map_or(("help", &[][..]), |(c, r)| (c.as_str(), r));
    match command {
        "list" => list::run(cx, rest),
        "show" => show::run(cx, rest),
        "status" => status::run(cx, rest),
        "search" => search::run(cx, rest),
        "help" | "--help" | "-h" => Ok(HELP.into()),
        other => Err(Failure::Usage(format!("commande inconnue : {other}"))),
    }
}

pub(crate) const HELP: &str = "\
greffier : les réunions du dossier de réunions, en ligne de commande. Sans commande, ouvre Greffier.

greffier search <requête> [--since J] [--until J] [--date J|M] [--speaker NOM] [--in OÙ] [--context N] [--limit N]
  Ce qui a été dit : les lignes qui ont tous les mots et \"expressions exactes\" (casse et accents
  ignorés ; un mot trouve ceux qui le contiennent : bookmark trouve bookmarks, tapez le radical),
  par réunion, de la plus récente : bilan puis répliques, [h:mm:ss] Nom : extrait.
  --in bilan,transcription,titre. --context N : N répliques autour. --limit : 30 en tout.
greffier list [--date J|M] [--since J] [--until J] [--limit N]
  Les réunions, de la plus récente à la plus ancienne : identifiant · durée · statut · titre.
greffier show <réunion> [--bilan | --transcription | --fiche | --locuteurs | --actions | --passages]
  Sans vue : en-tête, en bref, sujets, décisions. --bilan : le bilan complet. --fiche : la fiche.
  --transcription [--from T] [--to T] [--numeros] : les répliques, [h:mm:ss] Nom : texte.
  --locuteurs : index · label · nom · temps de parole. --actions : les cases du bilan, numérotées.
  --passages [--from T] [--to T] : les passages à vérifier, numérotés.
greffier status [<réunion>]
  Sans réunion : les réunions pas terminées. Avec : statut, étape faite, erreur.
greffier help

<réunion> : identifiant (2026-11-20-0939), nom ou chemin du dossier, date (2026-11-20) ou début d'identifiant.
J : AAAA-MM-JJ. M : AAAA-MM. T : h:mm:ss, m:ss ou secondes.
--json sur chaque commande. Sortie : 0 succès, 1 échec, 2 erreur d'usage.
GREFFIER_FOLDER remplace le dossier de réunions, GREFFIER_APP_FOLDER le dossier de l'app.

Exemples :
  greffier search bookmark label --since 2026-11-16
  greffier search '\"fiscal year\"' --speaker Khalid --context 2
  greffier show 2026-11-20-0939 --transcription --from 1:25:00 --to 1:30:00
";

/// `value` in JSON, on one line.
pub(crate) fn json(value: &impl serde::Serialize) -> String {
    let mut s = serde_json::to_string(value).unwrap_or_else(|e| format!("{{\"erreur\":\"{e}\"}}"));
    s.push('\n');
    s
}
