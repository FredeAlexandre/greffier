//! Les autres pages : premier lancement, vue d'ensemble, enregistrement, réglages, et le bandeau
//! de notification.

use rune::{El, ScrollSpec, col, el, row, text};
use rune_kit::{Button, ButtonVariant, ControlSize, Keybind, Kit, Switch};

use greffier_coeur::{Commande, Statut, bilan};

use crate::ui::{self, card, check_row, eyebrow, glyph, heading, muted, red, title_style, tnum};
use crate::{App, reglages};

fn page(key: &str, title: &str, subtitle: &str, k: &Kit, body: El) -> El {
    col()
        .key(key.to_owned())
        .w_full()
        .h_full()
        .min_h(0.0)
        .scroll(ScrollSpec { stick_bottom: false })
        .child(
            col()
                .w_full()
                .px(28.0)
                .pt(24.0)
                .pb(28.0)
                .gap(20.0)
                .child(col().gap(4.0).child(text(title.to_owned()).font(title_style(24.0)).color(k.c.text_base)).child(muted(k, subtitle.to_owned())))
                .child(body),
        )
}

pub fn with_toast(s: &App, k: &Kit, main: El) -> El {
    let Some(t) = &s.toast else { return col().w_full().h_full().min_h(0.0).child(main) };
    let id = t.meeting;
    let banner = row()
        .key("toast")
        .w_full()
        .px(16.0)
        .py(10.0)
        .gap(10.0)
        .center_y()
        .fill(k.c.success.bg)
        .child(glyph(&ui::CHECK, 14.0, k.c.success.icon))
        .child(text(t.text.clone()).font(k.ty.ui_medium).color(k.c.success.fg).grow(1.0).min_w(0.0).truncate())
        .child(
            Button::new("Ouvrir")
                .variant(ButtonVariant::Outline)
                .control_size(ControlSize::Small)
                .on_press(move |s: &mut App| {
                    if let Some(id) = id {
                        s.open(id);
                    }
                    s.toast = None;
                })
                .build(k),
        );
    col().w_full().h_full().min_h(0.0).child(banner).child(col().w_full().grow(1.0).min_h(0.0).child(main))
}

fn stat(k: &Kit, value: String, label: &str) -> El {
    col()
        .grow(1.0)
        .min_w(0.0)
        .gap(2.0)
        .padding(14.0)
        .radius(k.radius.lg)
        .fill(k.c.layer_01)
        .border(1.0, k.c.border_muted)
        .child(text(value).font(tnum(title_style(22.0))).color(k.c.text_base))
        .child(text(label.to_owned()).font(k.ty.small).color(k.c.text_muted))
}

/// Premier lancement : où ranger les réunions.
pub fn welcome(s: &App, k: &Kit) -> El {
    let proposed = reglages::dossier_propose();
    let mut body = card(k)
        .gap(14.0)
        .padding(24.0)
        .child(heading(k, "Où ranger tes réunions ?"))
        .child(muted(k, "Chaque réunion devient un dossier de fichiers lisibles hors de l'app : la fiche, le bilan, la transcription et l'enregistrement. Toi et tes agents IA pouvez les ouvrir directement."))
        .child(row().gap(8.0).center_y().child(glyph(&ui::FOLDER, 14.0, k.c.icon_muted)).child(text(reglages::afficher(&proposed)).font(k.ty.ui_medium).color(k.c.text_base)))
        .child(
            row().gap(8.0).pt(4.0)
                .child(Button::new("Utiliser ce dossier").variant(ButtonVariant::Submit).on_press(move |s: &mut App| s.use_folder(proposed.clone())).build(k).key("use-proposed"))
                .child(Button::new("Choisir un autre dossier…").icon(&ui::FOLDER).variant(ButtonVariant::Outline).on_press_tx(|s: &mut App, tx| s.pick_folder(tx)).build(k).key("pick-folder")),
        )
        .child(muted(k, "Tu pourras le changer dans les réglages. Greffier n'écrit rien d'autre dans ce dossier que les dossiers des réunions."));
    if let Some(e) = &s.folder_error {
        body = body.child(text(e.clone()).font(k.ty.small).color(k.c.danger.fg).wrap());
    }
    page("welcome", "Bienvenue dans Greffier", "Greffier enregistre tes réunions, les transcrit et en fait le bilan, sur ce Mac.", k, body)
}

pub fn overview(s: &App, k: &Kit) -> El {
    let meetings = s.core.reunions();
    let total: f64 = meetings.iter().map(|m| m.duree).sum();
    let todo = meetings.iter().filter(|m| m.statut != Statut::Terminee).count();
    let doubts: usize = meetings.iter().map(|m| m.passages_en_attente()).sum();
    let since = match meetings.iter().map(|m| m.debut).min() {
        Some(d) => format!("réunions depuis le {:02}/{:02}", d.jour, d.mois),
        None => "réunions".to_string(),
    };
    let stats = row()
        .w_full()
        .gap(10.0)
        .child(stat(k, format!("{}", meetings.len()), &since))
        .child(stat(k, ui::dur(total), "enregistrées"))
        .child(stat(k, format!("{todo}"), "en attente de bilan"))
        .child(stat(k, format!("{doubts}"), "passages à vérifier"));

    // Temps en réunion par jour, les 12 derniers jours qui en ont.
    let mut days: Vec<(greffier_coeur::DateHeure, f64)> = Vec::new();
    for m in meetings {
        let day = greffier_coeur::DateHeure { heure: 0, minute: 0, ..m.debut };
        match days.iter_mut().find(|(d, _)| *d == day) {
            Some((_, mins)) => *mins += m.duree / 60.0,
            None => days.push((day, m.duree / 60.0)),
        }
    }
    days.sort_by(|a, b| b.0.cmp(&a.0));
    days.truncate(12);
    let max = days.iter().map(|(_, m)| *m).fold(1.0, f64::max);
    let mut chart = card(k).gap(7.0).child(heading(k, "Temps en réunion par jour"));
    if days.is_empty() {
        chart = chart.child(muted(k, "Pas encore de réunion."));
    }
    for (i, (d, mins)) in days.iter().enumerate() {
        chart = chart.child(
            row().key(format!("day{i}")).w_full().gap(10.0).center_y()
                .child(text(d.jour_court()).font(tnum(k.ty.small)).color(k.c.text_muted).w(78.0))
                .child(row().grow(1.0).min_w(0.0).h(10.0).child(el().grow((*mins as f32).max(0.0001)).h(10.0).radius(3.0).fill(k.c.text_accent.alpha(0.75))).child(el().grow((max - *mins).max(0.0001) as f32)))
                .child(text(ui::dur(*mins * 60.0)).font(tnum(k.ty.small)).color(k.c.text_faint).w(52.0).align(rune::Align::End)),
        );
    }

    let mut recent = card(k).child(heading(k, "Dernières réunions"));
    for m in meetings.iter().take(5) {
        let id = m.id;
        let title = if m.statut == Statut::Terminee { crate::meeting::display_title(m, false) } else { format!("{} · {}", crate::meeting::display_title(m, false), ui::status(&m.statut).to_lowercase()) };
        let summary = if !m.resume.is_empty() { m.resume.clone() } else if m.bilan.is_empty() { "Pas encore de bilan.".into() } else { bilan::en_bref(&m.bilan).replace("**", "") };
        recent = recent.child(
            row().key(format!("rec{id}")).w_full().gap(12.0).center_y().px(8.0).py(8.0).radius(k.radius.md).hover(k.c.hover).cursor(rune::Cursor::Pointer)
                .on_press(move |s: &mut App| s.open(id))
                .child(col().w(78.0).child(text(m.debut.jour_court()).font(k.ty.small_medium).color(k.c.text_base)).child(text(m.debut.heure_courte()).font(k.ty.text_xsmall).color(k.c.text_faint)))
                .child(col().grow(1.0).min_w(0.0).gap(2.0)
                    .child(text(title).font(k.ty.ui_medium).color(k.c.text_base).truncate())
                    .child(text(summary).font(k.ty.small).color(k.c.text_muted).truncate()))
                .child(text(ui::dur(m.duree)).font(tnum(k.ty.small)).color(k.c.text_faint)),
        );
    }

    page("overview", "Réunions", "Tout ce qui a été dit et décidé, enregistré sur ce Mac.", k, col().w_full().gap(16.0).child(stats).child(row().w_full().gap(16.0).items_start().flex_wrap().child(col().grow(3.0).min_w(420.0).child(recent)).child(col().grow(2.0).min_w(320.0).child(chart))))
}

fn meter(k: &Kit, key: &str, icon: &'static rune::Icon, label: &str, phase: f64, seed: f64, on: bool) -> El {
    let mut bars = row().key(key.to_owned()).gap(3.0).center_y().h(36.0);
    for i in 0..28 {
        let f = i as f64;
        let v = if on {
            let wave = ((phase * 7.0 + f * 0.55 + seed).sin() * 0.5 + 0.5) * ((phase * 2.3 + seed * 3.0 + f * 0.13).sin() * 0.5 + 0.5);
            (0.12 + 0.88 * wave) as f32
        } else {
            0.08
        };
        let c = if v > 0.85 { k.c.warning.icon } else { k.c.success.icon };
        bars = bars.child(el().w(5.0).h(4.0 + 30.0 * v).radius(2.0).fill(if on { c } else { k.c.border_strong }));
    }
    row().w_full().gap(12.0).center_y()
        .child(glyph(icon, 15.0, k.c.icon_muted))
        .child(text(label.to_owned()).font(k.ty.ui_medium).color(k.c.text_base).w(130.0))
        .child(bars)
}

pub fn recording(s: &App, k: &Kit) -> El {
    let Some(r) = s.core.enregistrement() else {
        let body = card(k).gap(14.0).items_center().padding(32.0)
            .child(glyph(&ui::RECORD, 34.0, red()))
            .child(heading(k, "Prêt à enregistrer"))
            .child(muted(k, "L'écran, le son du Mac (la visio) et ton micro."))
            .child(Button::new("Démarrer l'enregistrement").icon(&ui::RECORD).variant(ButtonVariant::Danger).on_press(|s: &mut App| s.start_recording()).build(k).key("start"))
            .child(Keybind::new(&["⇧", "⌘", "R"]).build(k));
        return page("recording", "Enregistrer", "Un clic ou ⇧⌘R, de n'importe où.", k, body);
    };
    let phase = s.rec_phase;
    let pulse = 0.5 + 0.5 * (phase * 3.0).cos().abs() as f32;
    let timer = card(k)
        .key("rec-card")
        .gap(18.0)
        .padding(24.0)
        .child(
            row().w_full().gap(14.0).center_y()
                .child(el().size(14.0, 14.0).radius(7.0).fill(red()).opacity(pulse))
                .child(text("Enregistrement en cours").font(k.ty.body_medium).color(k.c.text_base).grow(1.0))
                .child(text("démo : temps accéléré ×20").font(k.ty.text_xsmall).color(k.c.text_faint)),
        )
        .child(text(ui::ts(r.duree)).key("rec-timer").font(tnum(title_style(56.0))).color(k.c.text_base))
        .child(meter(k, "lvl-mic", &ui::MIC, "Micro", phase, 0.0, s.source(2)))
        .child(meter(k, "lvl-sys", &ui::SOUND, "Son du Mac", phase, 2.1, s.source(1)))
        .child(
            row().w_full().gap(10.0).pt(4.0)
                .child(Button::new("Arrêter l'enregistrement").icon(&ui::STOP).variant(ButtonVariant::Danger).on_press(|s: &mut App| s.stop_recording()).build(k).key("stop"))
                .child(Button::new("Poser un repère").icon(&ui::FLAG).variant(ButtonVariant::Outline).shortcut("⌘M").on_press(|s: &mut App| s.add_marker()).build(k).key("marker")),
        );

    let mut markers = card(k).child(heading(k, "Repères"));
    if r.reperes.is_empty() {
        markers = markers.child(muted(k, "⌘M pendant la réunion pour marquer un moment important. Il apparaîtra sur la timeline."));
    }
    for (i, t) in r.reperes.iter().enumerate() {
        markers = markers.child(row().key(format!("rm{i}")).gap(8.0).center_y().child(glyph(&ui::FLAG, 13.0, k.c.text_accent)).child(text(format!("Repère {} · {}", i + 1, ui::ts(*t))).font(tnum(k.ty.small_medium)).color(k.c.text_base)));
    }

    let people = r.nombre_de_locuteurs;
    let setup = card(k)
        .child(heading(k, "Sources et réglages"))
        .child(check_row(k, s.source(0), "Écran", "Écran principal · 30 i/s"))
        .child(check_row(k, s.source(1), "Son du Mac", "Meet, Zoom, toute l'audio du système"))
        .child(check_row(k, s.source(2), "Micro", "Micro du MacBook Pro"))
        .child(
            row().w_full().gap(10.0).center_y().pt(6.0)
                .child(text("Personnes").font(k.ty.small_medium).color(k.c.text_muted).grow(1.0))
                .child(Button::new("−").variant(ButtonVariant::Outline).control_size(ControlSize::Small)
                    .on_press(move |s: &mut App| s.command(Commande::NombreDeLocuteursEnregistres(match people { Some(n) if n > 2 => Some(n - 1), _ => None }))).build(k))
                .child(text(people.map_or("Détecté".to_string(), |n| format!("{n}"))).font(k.ty.ui_medium).color(k.c.text_base).w(64.0).align(rune::Align::Center))
                .child(Button::new("+").variant(ButtonVariant::Outline).control_size(ControlSize::Small)
                    .on_press(move |s: &mut App| s.command(Commande::NombreDeLocuteursEnregistres(Some(people.map_or(2, |n| (n + 1).min(12)))))).build(k)),
        )
        .child(Switch::new(s.core.reglages().traiter_a_l_arret).label("Transcrire et faire le bilan à l'arrêt").on_toggle(|s: &mut App| s.toggle_auto_process()).build(k));

    page("recording", "Enregistrement", "La réunion arrive dans la liste à l'arrêt, avec son traitement.", k,
        col().w_full().gap(16.0).child(timer).child(row().w_full().gap(16.0).items_start().child(col().grow(1.0).min_w(0.0).child(setup)).child(col().grow(1.0).min_w(0.0).child(markers))))
}

pub fn settings(s: &App, k: &Kit) -> El {
    let src = |i: usize, label: &str| Switch::new(s.source(i)).label(label.to_owned()).on_toggle(move |s: &mut App| s.toggle_source(i)).build(k);
    let rec = card(k)
        .child(heading(k, "Enregistrement"))
        .child(src(0, "Écran"))
        .child(src(1, "Son du Mac"))
        .child(src(2, "Micro"))
        .child(row().w_full().justify_between().center_y().child(text("Raccourci global").font(k.ty.ui).color(k.c.text_base)).child(Keybind::new(&["⇧", "⌘", "R"]).build(k)))
        .child(row().w_full().justify_between().center_y().child(text("Poser un repère").font(k.ty.ui).color(k.c.text_base)).child(Keybind::new(&["⌘", "M"]).build(k)))
        .child(Switch::new(s.core.reglages().traiter_a_l_arret).label("Transcrire et faire le bilan à l'arrêt").on_toggle(|s: &mut App| s.toggle_auto_process()).build(k));
    let folder = match (&s.folder, s.needs_folder) {
        (Some(d), _) => reglages::afficher(d),
        (None, true) => "Pas encore choisi".to_string(),
        (None, false) => "Données d'exemple du prototype".to_string(),
    };
    let busy = s.core.occupe();
    let mut storage = card(k)
        .child(heading(k, "Dossier de réunions"))
        .child(
            row().w_full().gap(8.0).center_y()
                .child(glyph(&ui::FOLDER, 14.0, k.c.icon_muted))
                .child(text(folder).font(k.ty.ui).color(k.c.text_base).truncate_middle().grow(1.0).min_w(0.0))
                .child(Button::new("Changer…").variant(ButtonVariant::Outline).control_size(ControlSize::Small).disabled(busy)
                    .help(if busy { "Pas pendant un enregistrement ou un traitement" } else { "Choisir un autre dossier de réunions" })
                    .on_press_tx(|s: &mut App, tx| s.pick_folder(tx)).build(k).key("change-folder")),
        )
        .child(muted(k, "Un dossier par réunion : la fiche, le bilan, la transcription (lisible et structurée) et l'enregistrement. Toi et tes agents IA pouvez les lire sans passer par l'app."));
    if let Some(e) = &s.folder_error {
        storage = storage.child(text(e.clone()).font(k.ty.small).color(k.c.danger.fg).wrap());
    }
    let ai = card(k)
        .child(heading(k, "IA"))
        .child(check_row(k, true, "Lexique", "35 erreurs connues, 7 personnes, 40 mots justes"))
        .child(check_row(k, true, "Prompt de relecture", "prompt-relecture.md"))
        .child(check_row(k, true, "Prompt du bilan", "prompt-bilan.md"));
    let health = card(k)
        .child(row().w_full().justify_between().center_y().child(heading(k, "Vérifications")).child(eyebrow(k, "vérifié il y a 2 min")))
        .child(check_row(k, true, "Autorisations macOS", "écran, micro, son du système"))
        .child(check_row(k, true, "Whisper large-v3", "3,1 Go, dans le vault"))
        .child(check_row(k, true, "pyannote community-1", "35 Mo, puce graphique"))
        .child(check_row(k, true, "Claude", "connecté (claude.ai)"))
        .child(check_row(k, true, "Espace disque", "811 Go libres"));
    page("settings", "Réglages", "Sources, raccourcis, stockage et santé de l'installation.", k, col().w_full().gap(16.0).child(rec).child(storage).child(ai).child(health))
}
