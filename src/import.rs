//! L'import d'une réunion passée : fichiers déposés sur la fenêtre ou choisis par Fichier ›
//! Importer…, fenêtre d'import (date et titre modifiables), puis la question « maintenant ou plus
//! tard » pour le traitement.

use std::path::PathBuf;

use rune::{El, FileDialogOptions, Tx, col, row, text};
use rune_kit::{Button, ButtonVariant, ControlSize, Dialog, Kit, TextInput};

use greffier_coeur::dossier::{EXTENSIONS_DE_MEDIA, Import};
use greffier_coeur::{Commande, DateHeure, IdReunion, Statut};

use crate::ui::{self, glyph, muted};
use crate::{App, Toast, reglages};

/// La fenêtre d'import ouverte.
pub struct Draft {
    pub import: Import,
    pub date: rune::TextEdit,
    pub heure: rune::TextEdit,
    pub titre: rune::TextEdit,
    /// Le titre proposé, pour savoir si l'utilisateur a changé le sien.
    pub titre_propose: String,
    pub error: Option<String>,
}

const TRANSCRIPTIONS: [&str; 3] = ["vtt", "srt", "json"];

impl App {
    /// Ouvre la fenêtre d'import pour ces fichiers, ou y ajoute les fichiers si elle est ouverte.
    pub fn propose_import(&mut self, files: &[PathBuf]) {
        if self.needs_folder || files.is_empty() {
            return;
        }
        let mut all: Vec<PathBuf> = files.to_vec();
        if let Some(d) = &self.import {
            let i = &d.import;
            let kinds = |p: &PathBuf| kind(p);
            let new_kinds: Vec<u8> = files.iter().map(kinds).collect();
            for old in [Some(i.enregistrement.clone()), i.transcription.clone(), i.bilan.clone()].into_iter().flatten() {
                if !new_kinds.contains(&kind(&old)) {
                    all.push(old);
                }
            }
        }
        let proposal = match self.core.proposer_import(&all) {
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
            // Les fichiers changent ; la date et le titre déjà retouchés restent.
            Some(d) => {
                if d.titre.text.trim() == d.titre_propose {
                    d.titre = rune::TextEdit::new(proposal.titre.clone());
                }
                d.titre_propose = proposal.titre.clone();
                d.import = Import { debut: d.import.debut, titre: d.import.titre.clone(), ..proposal };
                d.error = None;
            }
            None => {
                self.import = Some(Draft {
                    date: rune::TextEdit::new(format!("{:04}-{:02}-{:02}", proposal.debut.annee, proposal.debut.mois, proposal.debut.jour)),
                    heure: rune::TextEdit::new(format!("{:02}:{:02}", proposal.debut.heure, proposal.debut.minute)),
                    titre: rune::TextEdit::new(proposal.titre.clone()),
                    titre_propose: proposal.titre.clone(),
                    import: proposal,
                    error: None,
                });
                self.menu = None;
                self.search = None;
            }
        }
    }

    /// Fichier › Importer… : le sélecteur de fichiers de macOS, pour le média.
    pub fn pick_import(&mut self, tx: &mut Tx) {
        if self.needs_folder {
            return;
        }
        let options = FileDialogOptions { title: "Importer une réunion".into(), ..Default::default() }.filter("Média", &EXTENSIONS_DE_MEDIA);
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

    /// Valide la fenêtre d'import : la réunion est créée, ouverte, et on demande quand la traiter.
    pub fn confirm_import(&mut self) {
        let Some(d) = &mut self.import else { return };
        let Some(debut) = parse_date(&d.date.text, &d.heure.text) else {
            d.error = Some("La date s'écrit AAAA-MM-JJ et l'heure HH:MM.".into());
            return;
        };
        let import = Import { debut, titre: d.titre.text.trim().to_string(), ..d.import.clone() };
        match self.core.executer(Commande::Importer(import)) {
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

    /// La réunion vient d'être importée : on l'ouvre et, s'il reste des étapes, on demande quand
    /// lancer le traitement.
    pub fn imported(&mut self, id: IdReunion) {
        self.open(id);
        if self.core.reunion(id).is_some_and(|m| m.statut == Statut::ATraiter) {
            self.process_imported = Some(id);
        }
    }

    /// « Lancer le traitement maintenant ». Tant que les étapes réelles ne sont pas branchées
    /// ([`reglages::ETAPES_REELLES`]), la réunion reste à traiter : les adaptateurs du prototype
    /// inventeraient une transcription dans le vrai dossier de réunions.
    pub fn process_imported_now(&mut self) {
        let Some(id) = self.process_imported.take() else { return };
        if reglages::ETAPES_REELLES || self.folder.is_none() {
            self.process(id);
        } else {
            self.toast = Some(Toast { text: "Le traitement n'est pas encore branché : la réunion reste à traiter.".into(), meeting: Some(id), until: self.clock + 6.0 });
        }
    }
}

/// 0 média, 1 transcription, 2 bilan, 3 autre.
fn kind(p: &std::path::Path) -> u8 {
    let e = p.extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase();
    if EXTENSIONS_DE_MEDIA.contains(&e.as_str()) {
        0
    } else if TRANSCRIPTIONS.contains(&e.as_str()) {
        1
    } else if e == "md" || e == "markdown" {
        2
    } else {
        3
    }
}

/// « 2026-10-02 » et « 09:30 » (ou « 9h30 »).
fn parse_date(date: &str, heure: &str) -> Option<DateHeure> {
    let n = |s: &str| s.trim().parse::<u32>().ok();
    let mut d = date.trim().split('-');
    let (a, m, j) = (n(d.next()?)?, n(d.next()?)?, n(d.next()?)?);
    let (h, mi) = heure.trim().split_once([':', 'h'])?;
    let (h, mi) = (n(h)?, n(mi)?);
    let ok = d.next().is_none() && (1..=12).contains(&m) && (1..=31).contains(&j) && h <= 23 && mi <= 59 && (1900..=9999).contains(&a);
    ok.then(|| DateHeure::new(a as i32, m as u8, j as u8, h as u8, mi as u8))
}

fn file_name(p: &std::path::Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Une ligne de fichier : rôle, nom (ou « Aucun »), bouton Ajouter / Retirer.
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
    let bilan = add_or_remove(
        i.bilan.is_some(),
        "import-bilan",
        |s| {
            if let Some(d) = &mut s.import {
                d.import.bilan = None;
            }
        },
        |s, tx| s.pick_import_part(tx, "Bilan de la réunion", "Bilan", &["md", "markdown"]),
    );
    let input = |edit: &rune::TextEdit, key: &str, width: Option<f32>, get: fn(&mut App) -> &mut rune::TextEdit| {
        TextInput::new(edit).width(width).on_edit(get).build(k).key(key.to_owned())
    };
    let what = match (&i.transcription, &i.bilan) {
        (None, _) => "Le traitement passera par la transcription, la relecture et le bilan.",
        (Some(_), None) => "La transcription sera reprise telle quelle, non relue ; le traitement passera par la relecture et le bilan.",
        (Some(_), Some(_)) => "La réunion arrive complète : aucune étape à faire.",
    };
    let mut body = col()
        .w_full()
        .px(16.0)
        .pb(16.0)
        .gap(12.0)
        .child(file_row(k, "import-media", "Média", Some(&i.enregistrement), None))
        .child(file_row(k, "import-transcription", "Transcription", i.transcription.as_ref(), Some(transcription)))
        .child(file_row(k, "import-bilan", "Bilan", i.bilan.as_ref(), Some(bilan)))
        .child(field(k, "Date", row().gap(8.0).child(input(&d.date, "import-date", Some(120.0), |s| &mut s.import.as_mut().expect("import ouvert").date)).child(input(&d.heure, "import-heure", Some(72.0), |s| &mut s.import.as_mut().expect("import ouvert").heure))))
        .child(field(k, "Titre", input(&d.titre, "import-titre", None, |s| &mut s.import.as_mut().expect("import ouvert").titre).grow(1.0)))
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
    // Un fichier déposé sur la fenêtre d'import s'y ajoute (ou remplace celui du même rôle).
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
    let Some(m) = s.core.reunion(id) else { return };
    let body = col()
        .w_full()
        .px(16.0)
        .pb(16.0)
        .gap(14.0)
        .child(row().gap(8.0).center_y().child(glyph(&ui::FOLDER, 13.0, k.c.icon_muted)).child(text(m.titre.clone()).font(k.ty.ui_medium).color(k.c.text_base).truncate().min_w(0.0)))
        .child(muted(k, format!("Réunion du {} à {}, {}.", m.debut.jour_court(), m.debut.heure_courte(), ui::dur(m.duree))))
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
