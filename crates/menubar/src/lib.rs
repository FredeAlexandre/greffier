//! Greffier's icon in the menu bar. Rune doesn't provide it on macOS yet, so it is written in
//! `objc2` (NSStatusItem), from the main thread (ADR 0004).
//!
//! What it shows is computed here, without AppKit ([`menu`]); the macOS adapter
//! ([`MenubarIcon`]) only displays it and returns the user's [`Choice`]s. Neither the core nor
//! the Rune UI sees `objc2`.

/// What the app tells the menu bar.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum State {
    /// No recording. `possible` is false as long as a recording can't start (meetings folder not
    /// chosen yet).
    Ready { possible: bool },
    /// A recording has been running for `duration` seconds.
    Recording { duration: f64 },
}

/// What the user picks in the menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    Start,
    Stop,
    Open,
    Quit,
}

/// The icon's look.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Idle,
    Recording,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Line {
    /// A line that can be picked (greyed out if `active` is false).
    Choice { text: String, choice: Choice, active: bool },
    /// An information line, never picked.
    Info(String),
    Separator,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Menu {
    pub icon: Icon,
    pub lines: Vec<Line>,
}

/// The icon and its menu for a state of the app.
pub fn menu(state: State) -> Menu {
    let line = |text: &str, choice, active| Line::Choice { text: text.into(), choice, active };
    let (icon, mut lines) = match state {
        State::Ready { possible } => (Icon::Idle, vec![line("Démarrer l'enregistrement", Choice::Start, possible)]),
        State::Recording { duration } => (Icon::Recording, vec![Line::Info(duration_text(duration)), line("Arrêter l'enregistrement", Choice::Stop, true)]),
    };
    // Quitting during a recording would cut it off: stop it first.
    let can_quit = matches!(state, State::Ready { .. });
    lines.extend([Line::Separator, line("Ouvrir Greffier", Choice::Open, true), Line::Separator, line("Quitter Greffier", Choice::Quit, can_quit)]);
    Menu { icon, lines }
}

/// The recording's information line: "Enregistrement en cours · 0:12:34".
pub fn duration_text(duration: f64) -> String {
    let t = duration.max(0.0) as u64;
    format!("Enregistrement en cours · {}:{:02}:{:02}", t / 3600, (t / 60) % 60, t % 60)
}

#[cfg(target_os = "macos")]
mod mac;
#[cfg(target_os = "macos")]
pub use mac::MenubarIcon;
