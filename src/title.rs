//! Changing a meeting's title: a click on the title (or « Changer le titre… » in the sidebar
//! menu) opens a window; the core renames the folder and its files.

use rune::{TextEdit, col, row, text};
use rune_kit::{Button, ButtonVariant, Dialog, Kit, TextInput};

use crate::ui::{self, muted};
use crate::{App, Command, El, MeetingId};

/// The title window, open.
pub struct Renaming {
    pub meeting: MeetingId,
    pub edit: TextEdit,
    pub error: Option<String>,
}

impl App {
    pub fn start_rename(&mut self, id: MeetingId) {
        let Some(m) = self.core.meeting(id) else { return };
        let mut edit = TextEdit::new(m.title.clone());
        edit.focused = true;
        self.rename = Some(Renaming { meeting: id, edit, error: None });
        self.menu = None;
    }

    pub fn confirm_rename(&mut self) {
        let Some(r) = &self.rename else { return };
        let c = Command::ChangeTitle { meeting: r.meeting, title: r.edit.text.clone() };
        match self.try_command(c) {
            Ok(()) => self.rename = None,
            Err(e) => {
                if let Some(r) = &mut self.rename {
                    r.error = Some(e.to_string());
                }
            }
        }
    }
}

pub fn window(s: &App, k: &Kit, layer: &mut Vec<El>) {
    let Some(r) = &s.rename else { return };
    let Some(m) = s.core.meeting(r.meeting) else { return };
    let input = TextInput::new(&r.edit).width(None).on_edit(|s: &mut App| &mut s.rename.as_mut().expect("title window open").edit).on_submit(|s: &mut App, _| s.confirm_rename()).build(k).key("rename-input").label("Titre de la réunion").w_full();
    let mut body = col()
        .w_full()
        .px(16.0)
        .pb(16.0)
        .gap(12.0)
        .child(input)
        .child(muted(k, format!("Le dossier « {} » et ses fichiers prendront ce titre ; les caractères interdits deviennent des tirets.", m.folder_name)));
    if let Some(e) = &r.error {
        body = body.child(text(e.clone()).wrap().font(k.ty.small).color(ui::red()));
    }
    body = body.child(
        row()
            .w_full()
            .gap(8.0)
            .justify_end()
            .child(Button::new("Annuler").variant(ButtonVariant::Ghost).on_press(|s: &mut App| s.rename = None).build(k).key("rename-cancel"))
            .child(Button::new("Renommer").variant(ButtonVariant::Submit).on_press(|s: &mut App| s.confirm_rename()).build(k).key("rename-ok")),
    );
    Dialog::new("rename", "Changer le titre").open(true).width(480.0).fit().on_close(|s: &mut App| s.rename = None).child(body).build(k, layer);
}
