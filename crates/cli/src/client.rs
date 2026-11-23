//! The command line's side of the socket (ADR 0006): reaching the Greffier process that works
//! on the expected meetings folder, launching it without a window if nobody answers.

use std::ffi::OsString;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::time::{Duration, Instant};

use crate::Failure;
use crate::protocol::{Action, Request, Response, Snapshot, VERSION, encode};
use crate::server::MAX_SOCKET_PATH;

/// The installed app's bundle identifier.
pub const BUNDLE_ID: &str = "io.github.fredealexandre.greffier";

/// Names the program the command line launches when nobody answers (the tests' host).
pub const SERVER_PROGRAM_VARIABLE: &str = "GREFFIER_SERVER_PROGRAM";

/// How to start a Greffier process without a window.
#[derive(Clone, Debug, PartialEq)]
pub enum Launch {
    /// This program with `--background`, detached, with these variables added.
    Program { program: PathBuf, env: Vec<(String, OsString)> },
    /// The installed app: `open -g -j -b <id> --args --background`.
    Bundle(String),
    /// Never launch.
    Nothing,
}

impl Launch {
    /// The launch for this command line: `GREFFIER_SERVER_PROGRAM` if set; the running binary
    /// if a folder is replaced (`GREFFIER_FOLDER`, `GREFFIER_APP_FOLDER`) or if it is not in an
    /// app bundle (development); otherwise the installed app.
    pub fn from_env() -> Launch {
        let set = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty());
        let replaced = set(greffier_core::settings::FOLDER_VARIABLE).is_some() || set(greffier_core::settings::APP_FOLDER_VARIABLE).is_some();
        // The binary may be a symbolic link in the PATH: the bundle is where it points to.
        let executable = std::env::current_exe().ok().map(|e| e.canonicalize().unwrap_or(e));
        Launch::rule(set(SERVER_PROGRAM_VARIABLE).map(PathBuf::from), replaced, executable)
    }

    /// [`Launch::from_env`]'s rule, on its inputs.
    pub fn rule(server_program: Option<PathBuf>, folders_replaced: bool, executable: Option<PathBuf>) -> Launch {
        if let Some(program) = server_program {
            return Launch::Program { program, env: vec![] };
        }
        match executable {
            Some(e) if folders_replaced || !in_bundle(&e) => Launch::Program { program: e, env: vec![] },
            _ => Launch::Bundle(BUNDLE_ID.into()),
        }
    }

    /// Starts the process; it outlives the command line. The program launched directly comes
    /// back as a child, to see whether it exits; the app launched by `open` does not.
    pub fn launch(&self) -> Result<Option<Child>, String> {
        match self {
            Launch::Nothing => Ok(None),
            Launch::Program { program, env } => std::process::Command::new(program)
                .arg("--background")
                .envs(env.iter().map(|(k, v)| (k, v)))
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                // Out of the terminal's process group: a ⌃C on the command line does not stop it.
                .process_group(0)
                .spawn()
                .map(Some)
                .map_err(|e| format!("lancement de {} : {e}", program.display())),
            Launch::Bundle(id) => {
                let status = std::process::Command::new("/usr/bin/open")
                    .args(["-g", "-j", "-b", id, "--args", "--background"])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .map_err(|e| format!("open : {e}"))?;
                if status.success() { Ok(None) } else { Err(format!("open -b {id} a échoué ({status}) : Greffier est-il installé ? (cargo xtask install-app)")) }
            }
        }
    }
}

/// Whether this executable is inside an app bundle (`…/Greffier.app/Contents/MacOS/greffier`).
fn in_bundle(executable: &Path) -> bool {
    executable.components().any(|c| c.as_os_str().to_string_lossy().ends_with(".app"))
}

/// At most this many launches while waiting for the process to answer.
const LAUNCHES: u32 = 3;

/// A process launched that exits at once (it found the app folder's lock still held by a
/// process quitting) is launched again no sooner than this after the previous launch, the time
/// for the one quitting to be gone.
const RELAUNCH_AFTER: Duration = Duration::from_secs(1);

/// The app launched by `open` cannot be watched: it is launched again if, this long after the
/// previous launch, the socket does not answer and no live process holds the app folder's lock
/// (`open` only woke up an instance quitting, or the instance launched found the lock held). Long
/// enough for an instance starting to have taken the lock: `open` sent to an instance running
/// would show its window.
const REOPEN_AFTER: Duration = Duration::from_secs(3);

/// How the command line reaches the Greffier process.
#[derive(Clone, Debug)]
pub struct Process {
    /// `<app folder>/greffier.sock`.
    pub socket: PathBuf,
    pub launch: Launch,
    /// How long a launched process has to answer.
    pub patience: Duration,
    /// How often `--wait` asks where the processing stands.
    pub poll: Duration,
    /// How long the process has to answer a request.
    pub timeouts: Timeouts,
}

/// How long the process has to answer a request, by kind: a process that hangs must not hang
/// the command line, and one at work must have the time to finish.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Timeouts {
    /// `hello`, `status`, `open_window`: answered from what the process knows.
    pub quick: Duration,
    /// An action: a few files written, a folder renamed or trashed.
    pub action: Duration,
    /// An import: the media is copied, and converted if need be, before the answer.
    pub import: Duration,
}

impl Default for Timeouts {
    fn default() -> Timeouts {
        Timeouts { quick: Duration::from_secs(30), action: Duration::from_secs(120), import: Duration::from_secs(600) }
    }
}

impl Timeouts {
    fn of(&self, request: &Request) -> Duration {
        match request {
            Request::Execute(Action::Import { .. }) => self.import,
            Request::Execute(_) => self.action,
            Request::Hello | Request::Status { .. } | Request::OpenWindow => self.quick,
        }
    }
}

impl Process {
    /// The process of this app folder, launched this way.
    pub fn new(app_folder: &Path, launch: Launch) -> Process {
        Process { socket: crate::server::socket_path(app_folder), launch, patience: Duration::from_secs(15), poll: Duration::from_secs(1), timeouts: Timeouts::default() }
    }

    /// The process of the app folder (`GREFFIER_APP_FOLDER` or the default), launched as
    /// [`Launch::from_env`] says.
    pub fn from_env() -> Process {
        Process::new(&greffier_core::settings::app_folder(), Launch::from_env())
    }

    /// A connection to the process working on `meetings_folder`; it is launched if nobody
    /// answers. Fails ([`Failure::Unreachable`]) if it does not answer in time, or works on
    /// another folder.
    pub(crate) fn reach(&self, meetings_folder: &Path) -> Result<Connection, Failure> {
        // No process can listen there: launching one would only wait for nothing.
        let length = self.socket.as_os_str().len();
        if length > MAX_SOCKET_PATH {
            return Err(Failure::Unreachable(format!(
                "le chemin du socket de Greffier est trop long ({length} octets, {MAX_SOCKET_PATH} au plus) : {}. Choisissez un dossier de l'app plus court (GREFFIER_APP_FOLDER).",
                self.socket.display()
            )));
        }
        if let Ok(c) = Connection::open(&self.socket, self.timeouts) {
            return c.greet(meetings_folder);
        }
        let deadline = Instant::now() + self.patience;
        let mut child = self.launch.launch().map_err(Failure::Unreachable)?;
        let (mut launches, mut launched) = (1, Instant::now());
        loop {
            std::thread::sleep(Duration::from_millis(50));
            if let Ok(c) = Connection::open(&self.socket, self.timeouts) {
                return c.greet(meetings_folder);
            }
            let now = Instant::now();
            if now >= deadline {
                return Err(Failure::Unreachable(not_answering(&self.socket)));
            }
            // The process launched is gone without answering: launched again, a few times.
            let gone = match &mut child {
                Some(c) => matches!(c.try_wait(), Ok(Some(_))),
                None => matches!(self.launch, Launch::Bundle(_)) && now - launched >= REOPEN_AFTER && !lock_held(&self.socket),
            };
            if !gone {
                continue;
            }
            if launches == LAUNCHES {
                if child.is_some() {
                    return Err(Failure::Unreachable(not_answering(&self.socket)));
                }
                continue;
            }
            if now - launched >= RELAUNCH_AFTER {
                child = self.launch.launch().map_err(Failure::Unreachable)?;
                (launches, launched) = (launches + 1, now);
            }
        }
    }

    /// What the process already running on `meetings_folder` is doing; `None` if none answers
    /// (it is never launched for this).
    pub(crate) fn snapshot(&self, meetings_folder: &Path) -> Option<Snapshot> {
        let mut c = Connection::open(&self.socket, self.timeouts).ok()?.greet(meetings_folder).ok()?;
        match c.ask(&Request::Status { meeting: None }) {
            Ok(Response::Status(s)) => Some(s),
            _ => None,
        }
    }
}

/// Sends one request to the process listening on `socket` and returns its response; `None` if
/// nobody answers. For the app's second launch: [`Request::OpenWindow`] to the first one.
pub fn ask_once(socket: &Path, request: &Request) -> Option<Response> {
    Connection::open(socket, Timeouts::default()).ok()?.ask(request).ok()
}

/// Whether a live process holds the lock of the app folder this socket is in: the lock file
/// gives the pid of its last holder.
fn lock_held(socket: &Path) -> bool {
    let Some(app_folder) = socket.parent() else { return false };
    let Ok(pid) = std::fs::read_to_string(crate::server::lock_path(app_folder)) else { return false };
    let pid = pid.trim();
    !pid.is_empty() && std::process::Command::new("/bin/kill").args(["-0", pid]).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
}

/// « Greffier ne répond pas », and the likely cause: an older Greffier, open without a socket
/// the command line can talk to.
fn not_answering(socket: &Path) -> String {
    format!("Greffier ne répond pas sur {}. Si un Greffier plus ancien est ouvert, quittez-le ou mettez-le à jour (cargo xtask install-app), puis relancez la commande.", socket.display())
}

/// A connection to the process: requests, one at a time.
pub(crate) struct Connection {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
    timeouts: Timeouts,
}

impl Connection {
    fn open(socket: &Path, timeouts: Timeouts) -> std::io::Result<Connection> {
        let s = UnixStream::connect(socket)?;
        Ok(Connection { reader: BufReader::new(s.try_clone()?), writer: s, timeouts })
    }

    /// [`Request::Hello`]: the process must speak this protocol and work on `meetings_folder`.
    fn greet(mut self, meetings_folder: &Path) -> Result<Connection, Failure> {
        let hello = match self.ask(&Request::Hello)? {
            Response::Hello(h) => h,
            other => return Err(Failure::Unreachable(format!("réponse inattendue de Greffier : {other:?}"))),
        };
        if hello.version != VERSION {
            return Err(Failure::Unreachable(format!(
                "Greffier (pid {}) parle une autre version du protocole ({}, la commande attend {VERSION}) : mettez-le à jour (cargo xtask install-app) ou quittez-le.",
                hello.pid, hello.version
            )));
        }
        let ours = meetings_folder.canonicalize().unwrap_or_else(|_| meetings_folder.to_path_buf());
        match hello.meetings_folder {
            Some(theirs) if theirs == ours => Ok(self),
            Some(theirs) => Err(Failure::Unreachable(format!("Greffier tourne sur un autre dossier de réunions : {} (la commande attend {})", theirs.display(), ours.display()))),
            None => Err(Failure::Unreachable("Greffier n'a pas encore de dossier de réunions : ouvrez-le pour le choisir.".into())),
        }
    }

    pub fn ask(&mut self, request: &Request) -> Result<Response, Failure> {
        let timeout = self.timeouts.of(request);
        let lost = |e: std::io::Error| match e.kind() {
            // Alive, but still at work: an action may still complete.
            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut => Failure::Unreachable(match request {
                Request::Execute(_) => format!(
                    "Greffier n'a pas répondu en {} : il travaille peut-être encore, et l'action peut aboutir. Vérifiez avec greffier list ou greffier status avant de la relancer.",
                    span(timeout)
                ),
                _ => format!("Greffier n'a pas répondu en {} : il est peut-être occupé ; réessayez dans un moment.", span(timeout)),
            }),
            _ => Failure::Unreachable(format!("Greffier ne répond plus : {e}")),
        };
        self.reader.get_ref().set_read_timeout(Some(timeout)).map_err(lost)?;
        self.writer.write_all(encode(request).as_bytes()).map_err(lost)?;
        let mut line = String::new();
        if self.reader.read_line(&mut line).map_err(lost)? == 0 {
            return Err(Failure::Unreachable("Greffier ne répond plus : connexion fermée".into()));
        }
        serde_json::from_str(&line).map_err(|e| Failure::Unreachable(format!("réponse illisible de Greffier : {e}")))
    }
}

/// « 0.2 s », « 30 s », « 10 min ».
fn span(d: Duration) -> String {
    let seconds = d.as_secs_f64();
    if seconds >= 60.0 && d.as_secs() % 60 == 0 { format!("{} min", d.as_secs() / 60) } else { format!("{seconds} s") }
}
