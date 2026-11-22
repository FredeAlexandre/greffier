//! L'icône de Greffier dans la barre des menus. Rune ne la fournit pas encore sur macOS : on
//! l'écrit en `objc2` (NSStatusItem), depuis le thread principal (ADR 0004).
//!
//! Ce qu'elle montre se calcule ici, sans AppKit ([`menu`]) ; l'adaptateur macOS
//! ([`IconeDeLaBarre`]) ne fait que l'afficher et rendre les [`Choix`] de l'utilisateur. Ni
//! le cœur ni l'interface Rune ne voient `objc2`.

/// Ce que l'app dit à la barre des menus.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum State {
    /// Aucun enregistrement. `possible` est faux tant qu'un enregistrement ne peut pas partir
    /// (dossier de réunions pas encore choisi).
    Ready { possible: bool },
    /// Un enregistrement tourne depuis `duree` secondes.
    Recording { duration: f64 },
}

/// Ce que l'utilisateur choisit dans le menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    Start,
    Stop,
    Open,
    Quit,
}

/// L'aspect de l'icône.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Idle,
    Recording,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Line {
    /// Une ligne qu'on peut choisir (grisée si `active` est faux).
    Choice { text: String, choice: Choice, active: bool },
    /// Une ligne d'information, jamais choisie.
    Info(String),
    Separator,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Menu {
    pub icon: Icon,
    pub lines: Vec<Line>,
}

/// L'icône et son menu pour un état de l'app.
pub fn menu(state: State) -> Menu {
    let line = |text: &str, choice, active| Line::Choice { text: text.into(), choice, active };
    let (icon, mut lines) = match state {
        State::Ready { possible } => (Icon::Idle, vec![line("Démarrer l'enregistrement", Choice::Start, possible)]),
        State::Recording { duration } => (Icon::Recording, vec![Line::Info(duration_text(duration)), line("Arrêter l'enregistrement", Choice::Stop, true)]),
    };
    // Quitter pendant un enregistrement le couperait : on l'arrête d'abord.
    let can_quit = matches!(state, State::Ready { .. });
    lines.extend([Line::Separator, line("Ouvrir Greffier", Choice::Open, true), Line::Separator, line("Quitter Greffier", Choice::Quit, can_quit)]);
    Menu { icon, lines }
}

/// La ligne d'information de l'enregistrement : « Enregistrement en cours · 0:12:34 ».
pub fn duration_text(duration: f64) -> String {
    let t = duration.max(0.0) as u64;
    format!("Enregistrement en cours · {}:{:02}:{:02}", t / 3600, (t / 60) % 60, t % 60)
}

#[cfg(target_os = "macos")]
mod mac;
#[cfg(target_os = "macos")]
pub use mac::MenubarIcon;
