//! Refaire une étape (et les suivantes) : « Refaire le bilan » dans l'onglet Bilan, « Refaire
//! la relecture » et « Refaire la transcription… » dans l'onglet Transcription et dans le menu de
//! la barre latérale. Refaire la transcription ouvre une fenêtre pour changer d'abord le nombre
//! de locuteurs.

use greffier_core::{Command, Stage, MeetingId, Meeting, Status};
use rune::{El, col, row, text};
use rune_kit::{Button, ButtonVariant, ControlSize, Dialog, Kit, Menu, MenuItem};

use crate::App;
use crate::ui::{self, muted};

/// La fenêtre « Refaire la transcription », ouverte.
pub struct RedoTranscription {
    pub meeting: MeetingId,
    /// Le nombre de locuteurs choisi (`None` : détecté).
    pub number: Option<u8>,
}

/// L'étape est faite et peut être refaite maintenant (pas dans la file).
pub fn can_redo(m: &Meeting, stage: Stage) -> bool {
    m.can_redo(stage) && m.completed_stage >= Some(stage)
}

impl App {
    /// Remet la réunion dans la file à partir de cette étape.
    pub fn redo(&mut self, id: MeetingId, stage: Stage) {
        self.menu = None;
        self.redo_menu = false;
        self.command(Command::Redo { meeting: id, stage });
    }

    pub fn start_redo_transcription(&mut self, id: MeetingId) {
        self.menu = None;
        self.redo_menu = false;
        let Some(m) = self.core.meeting(id) else { return };
        self.redo_transcription = Some(RedoTranscription { meeting: id, number: m.speaker_count });
    }

    /// Le nombre de locuteurs n'est changé que si la transcription peut être refaite.
    pub fn confirm_redo_transcription(&mut self) {
        let Some(r) = self.redo_transcription.take() else { return };
        let redo = Command::Redo { meeting: r.meeting, stage: Stage::Transcription };
        match self.try_command(redo) {
            Ok(()) => self.command(Command::SpeakerCount { meeting: r.meeting, number: r.number }),
            Err(e) => eprintln!("greffier : {e}"),
        }
    }
}

/// Le bouton « Refaire… » de l'onglet Transcription et son menu (relecture, transcription).
pub fn transcript_menu(s: &App, m: &Meeting, k: &Kit, layer: &mut Vec<El>) -> Option<El> {
    let (review, transcription) = (can_redo(m, Stage::Review), can_redo(m, Stage::Transcription));
    if !review && !transcription {
        return None;
    }
    let id = m.id;
    let trigger = Button::new("Refaire…")
        .icon(&ui::SPARK)
        .variant(ButtonVariant::Outline)
        .control_size(ControlSize::Small)
        .on_press(|s: &mut App| s.redo_menu = !s.redo_menu)
        .build(k)
        .key("redo-menu-button");
    let mut menu = Menu::new("redo-menu").trigger(trigger).open(s.redo_menu).min_width(300.0).on_close(|s: &mut App| s.redo_menu = false);
    if review {
        menu = menu.item(
            MenuItem::new("Refaire la relecture")
                .description("Avec le lexique actuel, puis le bilan")
                .on_select(move |s: &mut App| s.redo(id, Stage::Review)),
        );
    }
    if transcription {
        menu = menu.item(
            MenuItem::new("Refaire la transcription…")
                .description("Nombre de locuteurs au choix, puis relecture et bilan")
                .on_select(move |s: &mut App| s.start_redo_transcription(id)),
        );
    }
    Some(menu.build(k, layer))
}

pub fn window(s: &App, k: &Kit, layer: &mut Vec<El>) {
    let Some(r) = &s.redo_transcription else { return };
    let Some(m) = s.core.meeting(r.meeting) else { return };
    let people = r.number;
    let set = |number: Option<u8>| {
        move |s: &mut App| {
            if let Some(r) = &mut s.redo_transcription {
                r.number = number;
            }
        }
    };
    let stepper = row()
        .gap(6.0)
        .center_y()
        .child(Button::new("−").variant(ButtonVariant::Outline).control_size(ControlSize::Small).on_press(set(match people { Some(n) if n > 2 => Some(n - 1), _ => None })).build(k).key("redo-ppl-minus"))
        .child(text(people.map_or("Détecté".to_string(), |n| format!("{n} locuteurs"))).font(k.ty.ui_medium).color(k.c.text_base).w(100.0).align(rune::Align::Center))
        .child(Button::new("+").variant(ButtonVariant::Outline).control_size(ControlSize::Small).on_press(set(Some(people.map_or(2, |n| (n + 1).min(12))))).build(k).key("redo-ppl-plus"));
    let mut body = col()
        .w_full()
        .px(16.0)
        .pb(16.0)
        .gap(14.0)
        .child(row().gap(12.0).center_y().child(text("Nombre de locuteurs").font(k.ty.small_medium).color(k.c.text_muted).w(130.0)).child(stepper))
        .child(muted(k, "La transcription, la relecture et le bilan sont refaits. Les locuteurs sont séparés à nouveau : les noms qui leur ont été donnés et les passages tranchés sont oubliés."));
    if m.status == Status::Done {
        body = body.child(muted(k, "Le bilan actuel reste affiché jusqu'au nouveau."));
    }
    body = body.child(
        row()
            .w_full()
            .gap(8.0)
            .justify_end()
            .child(Button::new("Annuler").variant(ButtonVariant::Ghost).on_press(|s: &mut App| s.redo_transcription = None).build(k).key("redo-cancel"))
            .child(Button::new("Refaire la transcription").variant(ButtonVariant::Submit).on_press(|s: &mut App| s.confirm_redo_transcription()).build(k).key("redo-ok")),
    );
    Dialog::new("redo-transcription", "Refaire la transcription").open(true).width(480.0).fit().on_close(|s: &mut App| s.redo_transcription = None).child(body).build(k, layer);
}
