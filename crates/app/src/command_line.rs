//! The app as the Greffier process the command line talks to (ADR 0006): one process per app
//! folder ([`server::lock_instance`]), its socket ([`server::listen`]), each request answered on
//! the UI thread as the user's own action would be, and the launch without a window
//! (`--background`) that quits on its own once idle.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use greffier_cli::client;
use greffier_cli::protocol::{Request, Response};
use greffier_cli::server::{self, Host, IdlePolicy, LockError};
use greffier_core::{Event, Greffier, MeetingId, Status};
use rune::Tx;

use crate::{App, Toast, settings};

/// Replaces the idle delay of a process started in the background, in seconds
/// ([`server::IDLE_QUIT`] by default): for trials.
pub const IDLE_QUIT_VARIABLE: &str = "GREFFIER_IDLE_QUIT";

/// How often a process waiting to quit looks at the time.
const IDLE_CHECK: Duration = Duration::from_secs(2);

/// How this process opens the app.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Launch {
    /// `--background`: started by the command line, without a window (nor Dock icon) until it is
    /// opened; quits on its own once idle.
    pub background: bool,
    /// Takes the app folder's lock and answers the command line: a live window only, never a
    /// headless run nor a scene.
    pub serve: bool,
}

impl Launch {
    /// The launch for these arguments (without the program's name); `headless`: Rune renders
    /// without a window (`--snap`, `--script`…).
    pub fn new(args: &[String], headless: bool) -> Launch {
        let has = |flag: &str| args.iter().any(|a| a == flag);
        Launch { background: has("--background"), serve: !headless && !has("--scene") && !has("--playground") }
    }
}

/// Opens the app (`greffier` without a command). If another Greffier process holds the app
/// folder, a plain launch asks it to show its window and a `--background` one does nothing:
/// both exit at once.
pub fn run(args: &[String]) {
    let mut launch = Launch::new(args, rune::is_headless());
    let mut _instance = None;
    if launch.serve {
        let app_folder = settings::app_folder();
        match server::lock_instance(&app_folder) {
            Ok(lock) => _instance = Some(lock),
            Err(LockError::Held) => {
                if !launch.background {
                    hand_over(&server::socket_path(&app_folder));
                }
                return;
            }
            Err(LockError::Failed(e)) => {
                eprintln!("greffier: {e} (the command line will not reach this window)");
                launch.serve = false;
            }
        }
    }
    crate::app(launch).run();
}

/// Asks the Greffier process already running to show its window; it may still be starting, and
/// not listen yet.
fn hand_over(socket: &std::path::Path) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if client::ask_once(socket, &Request::OpenWindow).is_some() {
            return;
        }
        if Instant::now() >= deadline {
            eprintln!("greffier: Greffier is already running but does not answer on {}", socket.display());
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// The idle delay of a process started in the background: `GREFFIER_IDLE_QUIT` seconds, or
/// [`server::IDLE_QUIT`].
pub fn idle_quit_delay() -> f64 {
    std::env::var(IDLE_QUIT_VARIABLE).ok().and_then(|v| v.trim().parse::<f64>().ok()).filter(|d| d.is_finite() && *d >= 0.0).unwrap_or(server::IDLE_QUIT)
}

impl App {
    /// This app was started in the background: it quits after `idle_quit` seconds with nothing
    /// to do, no request, and its window never shown.
    pub fn in_background(mut self, idle_quit: f64) -> App {
        self.background = true;
        self.idle = IdlePolicy::new(true, idle_quit, self.clock);
        self
    }

    /// Answers a request of the command line, received at `now` (seconds on the app's clock).
    pub fn answer(&mut self, request: Request, now: f64) -> Response {
        self.idle.request(now.max(self.clock));
        let response = server::answer(self, request);
        // The question after an import, about a meeting the command line since queued.
        if let Some(id) = self.process_imported
            && self.core.meeting(id).is_none_or(|m| m.status != Status::ToProcess)
        {
            self.process_imported = None;
        }
        response
    }

    /// Shows the window (and the Dock icon of an app started in the background); from then on
    /// the app no longer quits on its own.
    pub fn show_window(&mut self) {
        self.window_shown();
        if let Some(h) = &self.handle {
            h.open();
        }
    }

    /// The window shows, however it came: the app no longer quits on its own, and one started
    /// in the background takes its Dock icon and menus.
    pub(crate) fn window_shown(&mut self) {
        if self.background && self.idle.may_quit() && let Some(h) = &self.handle {
            h.set_dock_icon(true);
        }
        self.idle.window_shown();
    }

    /// Listens on the app folder's socket; each request is answered on the UI thread.
    pub(crate) fn listen(&mut self, tx: &mut Tx) {
        let Some(handle) = self.handle.clone() else { return };
        let socket = server::socket_path(&settings::app_folder());
        let deliver = move |request: Request, reply: server::Reply| handle.update(move |s: &mut App, tx: &mut Tx| reply(s.answer(request, tx.clock())));
        self.serve(server::listen(&socket, deliver), tx);
    }

    /// Keeps the socket listened on. If it could not be (a socket path too long…), a window goes
    /// on without the command line, but a process started in the background quits at once:
    /// nobody can reach it, and it would idle for minutes for nothing.
    pub fn serve(&mut self, listening: Result<server::Listener, String>, tx: &mut Tx) {
        match listening {
            Ok(listener) => self.listener = Some(listener),
            Err(e) if self.background => {
                eprintln!("greffier: the command line cannot reach this process: {e}; quitting");
                self.quit(tx);
            }
            Err(e) => eprintln!("greffier: the command line will not reach this window: {e}"),
        }
    }

    /// Quits: the socket goes first, so that the command line launches a new process rather
    /// than talk to one quitting.
    pub(crate) fn quit(&mut self, tx: &mut Tx) {
        self.listener = None;
        tx.quit();
    }

    /// In `poll`: a process started in the background quits once idle long enough; until then
    /// it looks at the time every few seconds.
    pub(crate) fn quit_when_idle(&mut self, clock: f64, active: bool, tx: &mut Tx) {
        if !self.idle.may_quit() {
            return;
        }
        if self.handle.as_ref().is_some_and(rune::AppHandle::is_visible) {
            self.window_shown();
            return;
        }
        if self.idle.should_quit(clock, self.core.busy()) {
            eprintln!("greffier: nothing to do in the background: quitting");
            self.quit(tx);
        } else if !active {
            tx.wake_after(IDLE_CHECK);
        }
    }

    /// The core's events from a command line action, handled as those of the user's own
    /// commands, but for an import: the command line said whether to process it, so the meeting
    /// is neither opened nor asked about; a toast says it came in.
    fn handle_requested(&mut self, events: Vec<Event>) {
        let (imported, others): (Vec<Event>, Vec<Event>) = events.into_iter().partition(|e| matches!(e, Event::MeetingImported(_)));
        self.handle(others);
        for e in imported {
            if let Event::MeetingImported(id) = e
                && let Some(m) = self.core.meeting(id)
            {
                let text = format!("Réunion importée : {}", m.title);
                self.toast = Some(Toast { text, meeting: Some(id), until: self.clock + 6.0, error: false });
            }
        }
    }
}

/// The app answers the command line on its live core.
impl Host for App {
    fn core(&mut self) -> &mut Greffier {
        &mut self.core
    }
    fn background(&self) -> bool {
        self.background
    }
    /// None until the user chose one (first launch).
    fn meetings_folder(&mut self) -> Option<PathBuf> {
        if self.needs_folder {
            return None;
        }
        let root = self.core.folder().root().to_path_buf();
        Some(root.canonicalize().unwrap_or(root))
    }
    fn executed(&mut self, events: Vec<Event>) {
        self.follow_core();
        self.handle_requested(events);
    }
    fn deleted(&mut self, meeting: MeetingId) {
        self.forget(meeting);
    }
    fn open_window(&mut self) {
        self.show_window();
    }
}

/// macOS reopens the app (Dock, Finder, `open`): an app started in the background, without a
/// Dock icon, shows its window as one opened normally would.
pub(crate) fn on_reopen(s: &mut App, _visible: bool, _tx: &mut Tx) -> bool {
    if !s.background {
        s.window_shown();
        return false;
    }
    s.show_window();
    true
}
