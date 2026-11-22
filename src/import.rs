//! Importing a past meeting: files dropped on the window or chosen through Fichier › Importer…,
//! the import window (editable date and title), then the now-or-later question for the
//! processing.

use std::path::PathBuf;

use rune::{El, FileDialogOptions, Tx, col, row, text};
use rune_kit::{Button, ButtonVariant, ControlSize, Dialog, Kit, TextInput};

use greffier_core::folder::{MEDIA_EXTENSIONS, Import};
use greffier_core::{Command, DateTime, MeetingId, Status};

use crate::ui::{self, glyph, muted};
use crate::{App, Toast};

/// The open import window.
pub struct Draft {
    pub import: Import,
    pub date: rune::TextEdit,
    pub hour: rune::TextEdit,
    pub title: rune::TextEdit,
    /// The proposed title, to know whether the user changed theirs.
    pub proposed_title: String,
    pub error: Option<String>,
}

const TRANSCRIPTIONS: [&str; 3] = ["vtt", "srt", "json"];

impl App {
    /// Opens the import window for these files, or adds the files to it if it is open.
    pub fn propose_import(&mut self, files: &[PathBuf]) {
        if self.needs_folder || files.is_empty() {
            return;
        }
        let mut all: Vec<PathBuf> = files.to_vec();
        if let Some(d) = &self.import {
            let i = &d.import;
            let kinds = |p: &PathBuf| kind(p);
            let new_kinds: Vec<u8> = files.iter().map(kinds).collect();
            for old in [Some(i.recording.clone()), i.transcription.clone(), i.summary.clone()].into_iter().flatten() {
                if !new_kinds.contains(&kind(&old)) {
                    all.push(old);
                }
            }
        }
        let proposal = match self.core.propose_import(&all) {
            Ok(p) => p,
            Err(e) => {
                match &mut self.import {
                    Some(d) => d.error = Some(e.to_string()),
                    None => self.toast = Some(Toast { text: e.to_string(), meeting: None, until: self.clock + 6.0 }),
                }
                return;
            }
        };
        match &mut self.import {
            // The files change; a date and title already edited stay.
            Some(d) => {
                if d.title.text.trim() == d.proposed_title {
                    d.title = rune::TextEdit::new(proposal.title.clone());
                }
                d.proposed_title = proposal.title.clone();
                d.import = Import { start: d.import.start, title: d.import.title.clone(), ..proposal };
                d.error = None;
            }
            None => {
                self.import = Some(Draft {
                    date: rune::TextEdit::new(format!("{:04}-{:02}-{:02}", proposal.start.year, proposal.start.month, proposal.start.day)),
                    hour: rune::TextEdit::new(format!("{:02}:{:02}", proposal.start.hour, proposal.start.minute)),
                    title: rune::TextEdit::new(proposal.title.clone()),
                    proposed_title: proposal.title.clone(),
                    import: proposal,
                    error: None,
                });
                self.menu = None;
                self.search = None;
            }
        }
    }

    /// Fichier › Importer…: the macOS file picker, for the media.
    pub fn pick_import(&mut self, tx: &mut Tx) {
        if self.needs_folder {
            return;
        }
        let options = FileDialogOptions { title: "Importer une réunion".into(), ..Default::default() }.filter("Média", &MEDIA_EXTENSIONS);
        tx.open_file(options, |s: &mut App, answer, _| {
            if let Ok(Some(file)) = answer {
                s.propose_import(&[file]);
            }
        });
    }

    fn pick_import_part(&mut self, tx: &mut Tx, title: &str, name: &str, extensions: &[&str]) {
        let options = FileDialogOptions { title: title.into(), ..Default::default() }.filter(name, extensions);
        tx.open_file(options, |s: &mut App, answer, _| {
            if let Ok(Some(file)) = answer {
                s.propose_import(&[file]);
            }
        });
    }

    /// Confirms the import window: the meeting is created and opened, and we ask when to process it.
    pub fn confirm_import(&mut self) {
        let Some(d) = &mut self.import else { return };
        let Some(start) = parse_date(&d.date.text, &d.hour.text) else {
            d.error = Some("La date s'écrit AAAA-MM-JJ et l'heure HH:MM.".into());
            return;
        };
        let import = Import { start, title: d.title.text.trim().to_string(), ..d.import.clone() };
        match self.core.execute(Command::Import(import)) {
            Ok(events) => {
                self.import = None;
                self.handle(events);
            }
            Err(e) => {
                if let Some(d) = &mut self.import {
                    d.error = Some(e.to_string());
                }
            }
        }
    }

    /// The meeting was just imported: open it and, if stages remain, ask when to start the
    /// processing.
    pub fn imported(&mut self, id: MeetingId) {
        self.open(id);
        if self.core.meeting(id).is_some_and(|m| m.status == Status::ToProcess) {
            self.process_imported = Some(id);
        }
    }

    /// The « Lancer le traitement maintenant » button.
    pub fn process_imported_now(&mut self) {
        let Some(id) = self.process_imported.take() else { return };
        self.process(id);
    }
}

/// 0 media, 1 transcription, 2 summary, 3 other.
fn kind(p: &std::path::Path) -> u8 {
    let e = p.extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase();
    if MEDIA_EXTENSIONS.contains(&e.as_str()) {
        0
    } else if TRANSCRIPTIONS.contains(&e.as_str()) {
        1
    } else if e == "md" || e == "markdown" {
        2
    } else {
        3
    }
}

/// "2026-10-02" and "09:30" (or "9h30").
fn parse_date(date: &str, hour: &str) -> Option<DateTime> {
    let n = |s: &str| s.trim().parse::<u32>().ok();
    let mut d = date.trim().split('-');
    let (a, m, j) = (n(d.next()?)?, n(d.next()?)?, n(d.next()?)?);
    let (h, mi) = hour.trim().split_once([':', 'h'])?;
    let (h, mi) = (n(h)?, n(mi)?);
    let ok = d.next().is_none() && (1..=12).contains(&m) && (1..=31).contains(&j) && h <= 23 && mi <= 59 && (1900..=9999).contains(&a);
    ok.then(|| DateTime::new(a as i32, m as u8, j as u8, h as u8, mi as u8))
}

fn file_name(p: &std::path::Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// A file row: role, name (or « Aucun »), Ajouter / Retirer button.
fn file_row(k: &Kit, key: &str, label: &str, file: Option<&PathBuf>, action: Option<El>) -> El {
    let name = match file {
        Some(f) => text(file_name(f)).font(k.ty.ui).color(k.c.text_base),
        None => text("Aucun").font(k.ty.ui).color(k.c.text_faint),
    };
    let mut r = row()
        .key(key.to_owned())
        .w_full()
        .gap(10.0)
        .center_y()
        .child(text(label.to_owned()).font(k.ty.small_medium).color(k.c.text_muted).w(96.0))
        .child(row().grow(1.0).min_w(0.0).gap(6.0).center_y().child(glyph(&ui::FOLDER, 12.0, k.c.icon_muted)).child(name.truncate().min_w(0.0)));
    if let Some(a) = action {
        r = r.child(a);
    }
    r
}

fn field(k: &Kit, label: &str, input: El) -> El {
    row().w_full().gap(10.0).center_y().child(text(label.to_owned()).font(k.ty.small_medium).color(k.c.text_muted).w(96.0)).child(input)
}

pub fn window(s: &App, k: &Kit, layer: &mut Vec<El>) {
    let Some(d) = &s.import else { return };
    let i = &d.import;
    let add_or_remove = |present: bool, key: &str, remove: fn(&mut App), add: fn(&mut App, &mut Tx)| -> El {
        if present {
            Button::new("Retirer").variant(ButtonVariant::Ghost).control_size(ControlSize::Small).on_press(move |s: &mut App| remove(s)).build(k).key(format!("{key}-remove"))
        } else {
            Button::new("Ajouter…").variant(ButtonVariant::Ghost).control_size(ControlSize::Small).on_press_tx(move |s: &mut App, tx: &mut Tx| add(s, tx)).build(k).key(format!("{key}-add"))
        }
    };
    let transcription = add_or_remove(
        i.transcription.is_some(),
        "import-transcription",
        |s| {
            if let Some(d) = &mut s.import {
                d.import.transcription = None;
            }
        },
        |s, tx| s.pick_import_part(tx, "Transcription de la réunion", "Transcription", &TRANSCRIPTIONS),
    );
    let summary = add_or_remove(
        i.summary.is_some(),
        "import-summary",
        |s| {
            if let Some(d) = &mut s.import {
                d.import.summary = None;
            }
        },
        |s, tx| s.pick_import_part(tx, "Bilan de la réunion", "Bilan", &["md", "markdown"]),
    );
    let input = |edit: &rune::TextEdit, key: &str, width: Option<f32>, get: fn(&mut App) -> &mut rune::TextEdit| {
        TextInput::new(edit).width(width).on_edit(get).build(k).key(key.to_owned())
    };
    let what = match (&i.transcription, &i.summary) {
        (None, _) => "Le traitement passera par la transcription, la relecture et le bilan.",
        (Some(_), None) => "La transcription sera reprise telle quelle, non relue ; le traitement passera par la relecture et le bilan.",
        (Some(_), Some(_)) => "La réunion arrive complète : aucune étape à faire.",
    };
    let mut body = col()
        .w_full()
        .px(16.0)
        .pb(16.0)
        .gap(12.0)
        .child(file_row(k, "import-media", "Média", Some(&i.recording), None))
        .child(file_row(k, "import-transcription", "Transcription", i.transcription.as_ref(), Some(transcription)))
        .child(file_row(k, "import-summary", "Bilan", i.summary.as_ref(), Some(summary)))
        .child(field(k, "Date", row().gap(8.0).child(input(&d.date, "import-date", Some(120.0), |s| &mut s.import.as_mut().expect("import open").date)).child(input(&d.hour, "import-time", Some(72.0), |s| &mut s.import.as_mut().expect("import open").hour))))
        .child(field(k, "Titre", input(&d.title, "import-title", None, |s| &mut s.import.as_mut().expect("import open").title).grow(1.0)))
        .child(muted(k, format!("{what} Les fichiers sont copiés dans un nouveau dossier de réunion ; les originaux ne bougent pas.")));
    if let Some(e) = &d.error {
        body = body.child(text(e.clone()).wrap().font(k.ty.small).color(ui::red()));
    }
    body = body.child(
        row()
            .w_full()
            .gap(8.0)
            .justify_end()
            .child(Button::new("Annuler").variant(ButtonVariant::Ghost).on_press(|s: &mut App| s.import = None).build(k).key("import-cancel"))
            .child(Button::new("Importer").variant(ButtonVariant::Submit).on_press(|s: &mut App| s.confirm_import()).build(k).key("import-ok")),
    );
    // A file dropped on the import window is added to it (or replaces the one with the same role).
    let body = body.on_drop_files(|s: &mut App, files, _| s.propose_import(files));
    Dialog::new("import", "Importer une réunion")
        .open(true)
        .width(560.0)
        .fit()
        .on_close(|s: &mut App| s.import = None)
        .child(body)
        .build(k, layer);
}

pub fn process_window(s: &App, k: &Kit, layer: &mut Vec<El>) {
    let Some(id) = s.process_imported else { return };
    let Some(m) = s.core.meeting(id) else { return };
    let body = col()
        .w_full()
        .px(16.0)
        .pb(16.0)
        .gap(14.0)
        .child(row().gap(8.0).center_y().child(glyph(&ui::FOLDER, 13.0, k.c.icon_muted)).child(text(m.title.clone()).font(k.ty.ui_medium).color(k.c.text_base).truncate().min_w(0.0)))
        .child(muted(k, format!("Réunion du {} à {}, {}.", m.start.short_day(), m.start.short_time(), ui::dur(m.duration))))
        .child(
            row()
                .w_full()
                .gap(8.0)
                .justify_end()
                .child(Button::new("Plus tard").variant(ButtonVariant::Ghost).on_press(|s: &mut App| s.process_imported = None).build(k).key("import-later"))
                .child(Button::new("Lancer le traitement maintenant").variant(ButtonVariant::Submit).on_press(|s: &mut App| s.process_imported_now()).build(k).key("import-now")),
        );
    Dialog::new("import-done", "Réunion importée").open(true).width(460.0).fit().on_close(|s: &mut App| s.process_imported = None).child(body).build(k, layer);
}
