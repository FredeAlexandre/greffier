//! When a process started in the background quits on its own.

/// How long a process started in the background waits with nothing to do before quitting, in
/// seconds.
pub const IDLE_QUIT: f64 = 300.0;

/// A process started in the background (`--background`) quits after `timeout` seconds during
/// which nothing was busy ([`greffier_core::Greffier::busy`]) and no request came, unless its
/// window was shown once: the user then quits it. Time is given by the caller, in seconds from
/// any origin.
#[derive(Clone, Debug)]
pub struct IdlePolicy {
    background: bool,
    timeout: f64,
    /// Since when nothing happened.
    since: f64,
    window_shown: bool,
}

impl IdlePolicy {
    pub fn new(background: bool, timeout: f64, now: f64) -> IdlePolicy {
        IdlePolicy { background, timeout, since: now, window_shown: false }
    }

    /// A request came: the wait starts again.
    pub fn request(&mut self, now: f64) {
        self.since = now;
    }

    /// The window was shown: the process no longer quits on its own.
    pub fn window_shown(&mut self) {
        self.window_shown = true;
    }

    /// Whether to quit now; `busy`: what the core is doing.
    pub fn should_quit(&mut self, now: f64, busy: bool) -> bool {
        if busy {
            self.since = now;
        }
        self.background && !self.window_shown && now - self.since >= self.timeout
    }
}
