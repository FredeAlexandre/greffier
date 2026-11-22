//! Ce que montre l'icône de la barre des menus, selon l'état de l'app.

use greffier_menubar::{Choice, State, Icon, Line, menu};

fn choice(text: &str, choice: Choice) -> Line {
    Line::Choice { text: text.into(), choice, active: true }
}

#[test]
fn when_idle_the_menu_offers_to_start_and_open_greffier() {
    let m = menu(State::Ready { possible: true });
    assert_eq!(m.icon, Icon::Idle);
    assert_eq!(
        m.lines,
        vec![
            choice("Démarrer l'enregistrement", Choice::Start),
            Line::Separator,
            choice("Ouvrir Greffier", Choice::Open),
            Line::Separator,
            choice("Quitter Greffier", Choice::Quit),
        ]
    );
}

#[test]
fn while_recording_the_icon_changes_and_the_menu_shows_the_duration() {
    let m = menu(State::Recording { duration: 754.6 });
    assert_eq!(m.icon, Icon::Recording);
    assert_eq!(m.lines[0], Line::Info("Enregistrement en cours · 0:12:34".into()));
    assert_eq!(m.lines[1], choice("Arrêter l'enregistrement", Choice::Stop));
    assert!(m.lines.contains(&choice("Ouvrir Greffier", Choice::Open)));
    assert!(!m.lines.iter().any(|l| matches!(l, Line::Choice { choice: Choice::Start, .. })));
}

#[test]
fn duration_switches_to_hours() {
    let m = menu(State::Recording { duration: 2.0 * 3600.0 + 5.0 * 60.0 + 9.99 });
    assert_eq!(m.lines[0], Line::Info("Enregistrement en cours · 2:05:09".into()));
}

#[test]
fn without_meetings_folder_start_is_greyed_out() {
    let m = menu(State::Ready { possible: false });
    assert_eq!(m.lines[0], Line::Choice { text: "Démarrer l'enregistrement".into(), choice: Choice::Start, active: false });
    assert!(m.lines.contains(&choice("Ouvrir Greffier", Choice::Open)));
}

#[test]
fn cannot_quit_from_the_menu_during_a_recording() {
    let m = menu(State::Recording { duration: 3.0 });
    assert!(m.lines.contains(&Line::Choice { text: "Quitter Greffier".into(), choice: Choice::Quit, active: false }));
}
