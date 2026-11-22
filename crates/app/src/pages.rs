//! The other pages: first launch (meetings folder, resources), overview, recording, settings,
//! and the notification toast.

use rune::{El, ScrollSpec, col, el, row, text};
use rune_kit::{Button, ButtonVariant, ControlSize, Keybind, Kit, Progress, ShortcutRecorder, Spinner, Switch};

use greffier_core::adapters::{Permission, PermissionState, ResourceState, Resource};
use greffier_core::{Command, Status, summary};

use crate::ui::{self, card, check_row, eyebrow, glyph, heading, muted, red, title_style, tnum};
use crate::{App, Page, record_shortcut, settings};

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

/// First launch: where to keep the meetings.
pub fn welcome(s: &App, k: &Kit) -> El {
    let proposed = settings::suggested_folder();
    let mut body = card(k)
        .gap(14.0)
        .padding(24.0)
        .child(heading(k, "Où ranger tes réunions ?"))
        .child(muted(k, "Chaque réunion devient un dossier de fichiers lisibles hors de l'app : la fiche, le bilan, la transcription et l'enregistrement. Toi et tes agents IA pouvez les ouvrir directement."))
        .child(row().gap(8.0).center_y().child(glyph(&ui::FOLDER, 14.0, k.c.icon_muted)).child(text(settings::display(&proposed)).font(k.ty.ui_medium).color(k.c.text_base)))
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
    let meetings = s.core.meetings();
    let total: f64 = meetings.iter().map(|m| m.duration).sum();
    let without_summary = meetings.iter().filter(|m| m.status != Status::Done).count();
    let doubts: usize = meetings.iter().map(|m| m.pending_passages()).sum();
    let since = match meetings.iter().map(|m| m.start).min() {
        Some(d) => format!("réunions depuis le {:02}/{:02}", d.day, d.month),
        None => "réunions".to_string(),
    };
    let stats = row()
        .w_full()
        .gap(10.0)
        .child(stat(k, format!("{}", meetings.len()), &since))
        .child(stat(k, ui::dur(total), "enregistrées"))
        .child(stat(k, format!("{without_summary}"), "en attente de bilan"))
        .child(stat(k, format!("{doubts}"), "passages à vérifier"));

    // Time in meetings per day, over the last 12 days that have some.
    let mut days: Vec<(greffier_core::DateTime, f64)> = Vec::new();
    for m in meetings {
        let day = greffier_core::DateTime { hour: 0, minute: 0, ..m.start };
        match days.iter_mut().find(|(d, _)| *d == day) {
            Some((_, mins)) => *mins += m.duration / 60.0,
            None => days.push((day, m.duration / 60.0)),
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
                .child(text(d.short_day()).font(tnum(k.ty.small)).color(k.c.text_muted).w(78.0))
                .child(row().grow(1.0).min_w(0.0).h(10.0).child(el().grow((*mins as f32).max(0.0001)).h(10.0).radius(3.0).fill(k.c.text_accent.alpha(0.75))).child(el().grow((max - *mins).max(0.0001) as f32)))
                .child(text(ui::dur(*mins * 60.0)).font(tnum(k.ty.small)).color(k.c.text_faint).w(52.0).align(rune::Align::End)),
        );
    }

    let mut recent = card(k).child(heading(k, "Dernières réunions"));
    for m in meetings.iter().take(5) {
        let id = m.id;
        let title = if m.status == Status::Done { crate::meeting::display_title(m, false) } else { format!("{} · {}", crate::meeting::display_title(m, false), ui::status(&m.status).to_lowercase()) };
        let summary = if !m.brief.is_empty() { m.brief.clone() } else if m.summary.is_empty() { "Pas encore de bilan.".into() } else { summary::brief(&m.summary).replace("**", "") };
        recent = recent.child(
            row().key(format!("rec{id}")).w_full().gap(12.0).center_y().px(8.0).py(8.0).radius(k.radius.md).hover(k.c.hover).cursor(rune::Cursor::Pointer)
                .on_press(move |s: &mut App| s.open(id))
                .child(col().w(78.0).child(text(m.start.short_day()).font(k.ty.small_medium).color(k.c.text_base)).child(text(m.start.short_time()).font(k.ty.text_xsmall).color(k.c.text_faint)))
                .child(col().grow(1.0).min_w(0.0).gap(2.0)
                    .child(text(title).font(k.ty.ui_medium).color(k.c.text_base).truncate())
                    .child(text(summary).font(k.ty.small).color(k.c.text_muted).truncate()))
                .child(text(ui::dur(m.duration)).font(tnum(k.ty.small)).color(k.c.text_faint)),
        );
    }

    page("overview", "Réunions", "Tout ce qui a été dit et décidé, enregistré sur ce Mac.", k, col().w_full().gap(16.0).child(stats).child(row().w_full().gap(16.0).items_start().flex_wrap().child(col().grow(3.0).min_w(420.0).child(recent)).child(col().grow(2.0).min_w(320.0).child(chart))))
}

/// A level's bars: their height follows the level (0 to 1, `None`: source off), their ripple
/// the animation.
fn meter(k: &Kit, key: &str, icon: &'static rune::Icon, label: &str, phase: f64, seed: f64, level: Option<f32>) -> El {
    let on = level.is_some();
    let level = level.unwrap_or(0.0);
    let mut bars = row().key(key.to_owned()).gap(3.0).center_y().h(36.0);
    for i in 0..28 {
        let f = i as f64;
        let v = if on {
            let wave = ((phase * 7.0 + f * 0.55 + seed).sin() * 0.5 + 0.5) * ((phase * 2.3 + seed * 3.0 + f * 0.13).sin() * 0.5 + 0.5);
            0.08 + 0.92 * level.clamp(0.0, 1.0) * (0.25 + 0.75 * wave as f32)
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

/// The macOS permissions the sources need, with a way to request them.
pub fn permissions(s: &App, k: &Kit) -> El {
    let states = s.core.permissions();
    let mut c = card(k).key("permissions").child(heading(k, "Autorisations de macOS"));
    for a in Permission::ALL {
        let state = states.state(a);
        let detail = match state {
            PermissionState::Granted => "accordée".to_string(),
            PermissionState::ToRequest => "pas encore demandée".to_string(),
            PermissionState::Denied => format!("refusée : Réglages Système › Confidentialité et sécurité › {}", a.section()),
        };
        let mut line = row().key(format!("perm-{}", a.name())).w_full().gap(8.0).center_y().child(check_row(k, state == PermissionState::Granted, a.name(), &detail).grow(1.0).min_w(0.0));
        if state != PermissionState::Granted {
            let label = if state == PermissionState::ToRequest { "Autoriser" } else { "Ouvrir les Réglages" };
            line = line.child(Button::new(label).variant(ButtonVariant::Outline).control_size(ControlSize::Small).on_press(move |s: &mut App| s.command(Command::RequestPermission(a))).build(k));
        }
        c = c.child(line);
    }
    // Notifications: only the installed app can post them.
    if rune_notify::available() {
        let state = greffier_capture::notifications::state();
        let detail = match state {
            PermissionState::Granted => "accordées : « Bilan prêt » arrive même quand Greffier est en arrière-plan",
            PermissionState::ToRequest => "pas encore demandées : sans elles, « Bilan prêt » ne s'affiche que dans Greffier",
            PermissionState::Denied => "refusées : Réglages Système › Notifications › Greffier",
        };
        let mut line = row().key("perm-notifications").w_full().gap(8.0).center_y().child(check_row(k, state == PermissionState::Granted, "Notifications", detail).grow(1.0).min_w(0.0));
        if state != PermissionState::Granted {
            let label = if state == PermissionState::ToRequest { "Autoriser" } else { "Ouvrir les Réglages" };
            line = line.child(Button::new(label).variant(ButtonVariant::Outline).control_size(ControlSize::Small).on_press(|_: &mut App| greffier_capture::notifications::request()).build(k));
        }
        c = c.child(line);
    }
    if states.screen != PermissionState::Granted {
        c = c.child(muted(k, "Le son du Mac passe par l'autorisation de l'écran. Une fois accordée, macOS demande de relancer Greffier."));
    }
    c
}

/// The permissions on one line, for the checks.
fn permissions_summary(s: &App, k: &Kit) -> El {
    let states = s.core.permissions();
    let missing: Vec<&str> = Permission::ALL.into_iter().filter(|a| states.state(*a) != PermissionState::Granted).map(Permission::name).collect();
    let detail = if missing.is_empty() { "écran, micro, son du système".to_string() } else { format!("manquantes : {}", missing.join(", ").to_lowercase()) };
    check_row(k, missing.is_empty(), "Autorisations macOS", &detail)
}

/// The keys of the global recording shortcut ("⇧⌘R").
pub fn shortcut_caps(s: &App) -> Vec<String> {
    s.shortcut.shortcut().map(|sc| sc.keycaps()).unwrap_or_default()
}

pub fn recording(s: &App, k: &Kit) -> El {
    let Some(r) = s.core.recording() else {
        let caps = shortcut_caps(s);
        let mut start = card(k).gap(14.0).items_center().padding(32.0)
            .child(glyph(&ui::RECORD, 34.0, red()))
            .child(heading(k, "Prêt à enregistrer"))
            .child(muted(k, "L'écran, le son du Mac (la visio) et ton micro."))
            .child(Button::new("Démarrer l'enregistrement").icon(&ui::RECORD).variant(ButtonVariant::Danger).on_press(|s: &mut App| s.start_recording()).build(k).key("start"));
        if !caps.is_empty() {
            start = start.child(Keybind::new(&caps).build(k));
        }
        if let Some(e) = &s.record_error {
            start = start.child(text(e.clone()).key("record-error").font(k.ty.small).color(k.c.danger.fg).wrap());
        }
        let src = |i: usize, label: &str| Switch::new(s.source(i)).label(label.to_owned()).on_toggle(move |s: &mut App| s.toggle_source(i)).build(k);
        let sources = card(k)
            .child(heading(k, "Sources"))
            .child(src(0, "Écran"))
            .child(src(1, "Son du Mac"))
            .child(src(2, "Micro"))
            .child(Switch::new(s.core.settings().process_on_stop).label("Transcrire et faire le bilan à l'arrêt").on_toggle(|s: &mut App| s.toggle_auto_process()).build(k));
        let subtitle = if caps.is_empty() { "Un clic sur le bouton.".to_string() } else { format!("Un clic ou {}, de n'importe où.", caps.concat()) };
        let body = col().w_full().gap(16.0).child(start).child(row().w_full().gap(16.0).items_start().child(col().grow(1.0).min_w(0.0).child(sources)).child(col().grow(1.0).min_w(0.0).child(permissions(s, k))));
        return page("recording", "Enregistrer", &subtitle, k, body);
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
                .child(text(if s.folder.is_none() { "démo : temps accéléré ×20" } else { "" }).font(k.ty.text_xsmall).color(k.c.text_faint)),
        )
        .child(text(ui::ts(r.duration)).key("rec-timer").font(tnum(title_style(56.0))).color(k.c.text_base))
        .child(meter(k, "lvl-mic", &ui::MIC, "Micro", phase, 0.0, r.sources.micro.then_some(r.levels.micro)))
        .child(meter(k, "lvl-sys", &ui::SOUND, "Son du Mac", phase, 2.1, r.sources.mac_audio.then_some(r.levels.mac_audio)))
        .child(
            row().w_full().gap(10.0).pt(4.0)
                .child(Button::new("Arrêter l'enregistrement").icon(&ui::STOP).variant(ButtonVariant::Danger).on_press(|s: &mut App| s.stop_recording()).build(k).key("stop"))
                .child(Button::new("Poser un repère").icon(&ui::FLAG).variant(ButtonVariant::Outline).shortcut("⌘M").on_press(|s: &mut App| s.add_marker()).build(k).key("marker")),
        );

    let mut markers = card(k).child(heading(k, "Repères"));
    if r.markers.is_empty() {
        markers = markers.child(muted(k, "⌘M pendant la réunion pour marquer un moment important. Il apparaîtra sur la timeline."));
    }
    for (i, t) in r.markers.iter().enumerate() {
        markers = markers.child(row().key(format!("rm{i}")).gap(8.0).center_y().child(glyph(&ui::FLAG, 13.0, k.c.text_accent)).child(text(format!("Repère {} · {}", i + 1, ui::ts(*t))).font(tnum(k.ty.small_medium)).color(k.c.text_base)));
    }

    let people = r.speaker_count;
    let setup = card(k)
        .child(heading(k, "Sources et réglages"))
        .child(check_row(k, r.sources.screen, "Écran", if r.sources.screen { "Écran principal · 30 i/s" } else { "coupé" }))
        .child(check_row(k, r.sources.mac_audio, "Son du Mac", if r.sources.mac_audio { "Meet, Zoom, toute l'audio du système" } else { "coupé" }))
        .child(check_row(k, r.sources.micro, "Micro", if r.sources.micro { "Micro par défaut du Mac" } else { "coupé" }))
        .child(
            row().w_full().gap(10.0).center_y().pt(6.0)
                .child(text("Nombre de locuteurs").font(k.ty.small_medium).color(k.c.text_muted).grow(1.0))
                .child(Button::new("−").variant(ButtonVariant::Outline).control_size(ControlSize::Small)
                    .on_press(move |s: &mut App| s.command(Command::RecordingSpeakerCount(match people { Some(n) if n > 2 => Some(n - 1), _ => None }))).build(k))
                .child(text(people.map_or("Détecté".to_string(), |n| format!("{n}"))).font(k.ty.ui_medium).color(k.c.text_base).w(64.0).align(rune::Align::Center))
                .child(Button::new("+").variant(ButtonVariant::Outline).control_size(ControlSize::Small)
                    .on_press(move |s: &mut App| s.command(Command::RecordingSpeakerCount(Some(people.map_or(2, |n| (n + 1).min(12)))))).build(k)),
        )
        .child(Switch::new(s.core.settings().process_on_stop).label("Transcrire et faire le bilan à l'arrêt").on_toggle(|s: &mut App| s.toggle_auto_process()).build(k));

    page("recording", "Enregistrement", "La réunion arrive dans la liste à l'arrêt, avec son traitement.", k,
        col().w_full().gap(16.0).child(timer).child(row().w_full().gap(16.0).items_start().child(col().grow(1.0).min_w(0.0).child(setup)).child(col().grow(1.0).min_w(0.0).child(markers))))
}

/// The recording shortcut's field. rune-kit words its empty field in English ("Record
/// shortcut", "Press shortcut") with no option to change it: French words cover them, drawn as
/// rune-kit draws its own (inset by the field's border, 8 pt from its edge), and inert so the
/// field still takes the press. The cover stays mounted and shows or hides at once: mounted only
/// when needed, it would fade in over 0.2 s and let the English words show meanwhile.
/// VoiceOver still reads rune-kit's English value and description of the empty field.
fn shortcut_field(s: &App, k: &Kit) -> El {
    let field = ShortcutRecorder::new("record-shortcut", &s.shortcut, App::shortcut_field)
        .label("Raccourci d'enregistrement")
        .error_message(record_shortcut::error_message)
        .on_change(|s: &mut App, next, _| s.change_shortcut(next))
        .build(k);
    let empty = s.shortcut.keycaps().is_empty();
    let words = if s.shortcut.is_recording() { "Tape un raccourci…" } else { "Choisir un raccourci" };
    field.child(
        row().key("record-shortcut-words").pin(Some(1.0), Some(1.0), Some(1.0), None).h(30.0).px(7.0).center_y()
            .radius(k.radius.md - 1.0).fill(k.c.bg_base)
            .child(text(words).font(k.ty.ui_compact).color(k.c.text_faint))
            .opacity(if empty { 1.0 } else { 0.0 })
            .instant_changes()
            .inert().a11y_hidden(),
    )
}

pub fn settings(s: &App, k: &Kit) -> El {
    let src = |i: usize, label: &str| Switch::new(s.source(i)).label(label.to_owned()).on_toggle(move |s: &mut App| s.toggle_source(i)).build(k);
    let rec = card(k)
        .child(heading(k, "Enregistrement"))
        .child(src(0, "Écran"))
        .child(src(1, "Son du Mac"))
        .child(src(2, "Micro"))
        .child(
            row().w_full().justify_between().center_y()
                .child(col().gap(2.0).child(text("Raccourci global").font(k.ty.ui).color(k.c.text_base)).child(muted(k,
                    if s.shortcut.is_recording() { "Échap annule, ⌫ efface le raccourci." } else { "Démarre et arrête l'enregistrement, même app en arrière-plan." })))
                .child(shortcut_field(s, k)),
        )
        .child(
            row().w_full().justify_between().center_y()
                .child(col().gap(2.0).child(text("Poser un repère").font(k.ty.ui).color(k.c.text_base)).child(muted(k, "De n'importe où, pendant l'enregistrement seulement.")))
                .child(Keybind::new(&["⌘", "M"]).build(k)),
        )
        .child(Switch::new(s.core.settings().process_on_stop).label("Transcrire et faire le bilan à l'arrêt").on_toggle(|s: &mut App| s.toggle_auto_process()).build(k));
    let folder = match (&s.folder, s.needs_folder) {
        (Some(d), _) => settings::display(d),
        (None, true) => "Pas encore choisi".to_string(),
        (None, false) => "Données d'exemple du prototype".to_string(),
    };
    let busy = s.core.busy();
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
    let l = &s.lexicon;
    let ai = card(k)
        .child(heading(k, "IA"))
        .child(
            row().w_full().gap(10.0).center_y()
                .child(check_row(k, true, "Lexique", &format!("{} erreurs connues, {} personnes, {} mots justes", l.known_errors, l.people, l.correct_words)).grow(1.0).min_w(0.0))
                .child(Button::new("Ouvrir le lexique").variant(ButtonVariant::Outline).control_size(ControlSize::Small)
                    .help("Le lexique s'ouvre dans l'éditeur de texte par défaut ; la relecture y ajoute les erreurs qu'elle apprend")
                    .on_press(|s: &mut App| s.open_lexicon()).build(k).key("open-lexique")),
        )
        .child(check_row(k, true, "Prompt de relecture", "prompt-review.md"))
        .child(check_row(k, true, "Prompt du bilan", "prompt-summary.md"));
    let folders = &s.core.settings().context_folders;
    let mut context = card(k)
        .key("context")
        .child(
            row().w_full().justify_between().center_y()
                .child(heading(k, "Dossiers de contexte de l'agent"))
                .child(Button::new("Ajouter…").variant(ButtonVariant::Outline).control_size(ControlSize::Small)
                    .help("Ouvrir un dossier de plus à l'agent du bilan")
                    .on_press_tx(|s: &mut App, tx| s.pick_context_folder(tx)).build(k).key("add-context")),
        )
        .child(muted(k, "L'agent du bilan démarre dans le dossier de réunions et peut y relire les bilans précédents. Les dossiers ajoutés ici (notes, projets, documents) lui sont ouverts en plus, pour comprendre un nom ou un projet."));
    if folders.is_empty() {
        context = context.child(text("Aucun pour l'instant.").font(k.ty.small).color(k.c.text_faint));
    }
    for (i, d) in folders.iter().enumerate() {
        context = context.child(
            row().key(format!("context{i}")).w_full().gap(8.0).center_y()
                .child(glyph(&ui::FOLDER, 14.0, k.c.icon_muted))
                .child(text(settings::display(d)).font(k.ty.ui).color(k.c.text_base).truncate_middle().grow(1.0).min_w(0.0))
                .child(Button::new("Retirer").variant(ButtonVariant::Ghost).control_size(ControlSize::Small).on_press(move |s: &mut App| s.remove_context_folder(i)).build(k).key(format!("remove-context{i}"))),
        );
    }
    let resources = s.core.resources();
    let disk_usage: u64 = resources.iter().map(|r| r.size).sum();
    let mut health = card(k)
        .child(row().w_full().justify_between().center_y().child(heading(k, "Vérifications")).child(eyebrow(k, &format!("ressources : {}", ui::bytes(disk_usage)))))
        .child(permissions_summary(s, k));
    for r in resources {
        let detail = match &r.state {
            ResourceState::Ready if r.size > 0 => format!("{} · {}", ui::bytes(r.size), r.detail),
            ResourceState::Ready => r.detail.clone(),
            state => format!("{} · {}", resource_state(r, state), r.detail),
        };
        health = health.child(check_row(k, r.state == ResourceState::Ready, &r.name, &detail));
    }
    let health = health
        .child(check_row(k, true, "Espace disque", "811 Go libres"))
        .child(
            row().w_full().gap(10.0).center_y().pt(4.0)
                .child(muted(k, "Modèles, Python privé et lexique sont dans ~/Library/Application Support/Greffier.").grow(1.0).min_w(0.0))
                .child(Button::new("Ressources…").variant(ButtonVariant::Outline).control_size(ControlSize::Small).on_press(|s: &mut App| s.page = Page::Installation).build(k).key("open-resources")),
        );
    page("settings", "Réglages", "Sources, raccourcis, stockage et santé de l'installation.", k, col().w_full().gap(16.0).child(rec).child(permissions(s, k)).child(storage).child(ai).child(context).child(health))
}

/// A resource's state in a few words.
fn resource_state(r: &Resource, state: &ResourceState) -> String {
    let percent = |received: u64, total: u64| (received as f64 / total.max(1) as f64 * 100.0).floor() as u64;
    match state {
        ResourceState::Missing if r.total > 0 => format!("À télécharger · {}", ui::bytes(r.total)),
        ResourceState::Missing if r.help.is_some() => "Introuvable".into(),
        ResourceState::Missing => "À installer".into(),
        ResourceState::Interrupted { received, total } => format!("Interrompu à {} % · reprendra", percent(*received, *total)),
        ResourceState::Downloading { received, total } => format!("{} sur {} · {} %", ui::bytes(*received), ui::bytes(*total), percent(*received, *total)),
        ResourceState::Verifying => "Vérification…".into(),
        ResourceState::Installing => "Installation…".into(),
        ResourceState::Ready if r.size > 0 => format!("Prête · {}", ui::bytes(r.size)),
        ResourceState::Ready => "Prête".into(),
        ResourceState::Failed(_) => "Échec".into(),
    }
}

fn resource_row(k: &Kit, i: usize, r: &Resource) -> El {
    let badge = match &r.state {
        ResourceState::Ready => glyph(&ui::CHECK, 15.0, k.c.success.icon),
        ResourceState::Downloading { .. } | ResourceState::Verifying | ResourceState::Installing => Spinner::new().size(14.0).build(k),
        ResourceState::Failed(_) => glyph(&ui::FLAG, 15.0, k.c.danger.icon),
        _ => glyph(&ui::FLAG, 15.0, k.c.warning.icon),
    };
    let state_color = match &r.state {
        ResourceState::Ready => k.c.success.fg,
        ResourceState::Failed(_) => k.c.danger.fg,
        _ => k.c.text_muted,
    };
    let mut body = col()
        .grow(1.0)
        .min_w(0.0)
        .gap(3.0)
        .child(
            row().w_full().gap(10.0).center_y()
                .child(text(r.name.clone()).font(k.ty.ui_medium).color(k.c.text_base).grow(1.0).min_w(0.0).truncate())
                .child(text(resource_state(r, &r.state)).font(tnum(k.ty.small_medium)).color(state_color)),
        )
        .child(text(r.detail.clone()).font(k.ty.small).color(k.c.text_faint).truncate());
    if let ResourceState::Downloading { received, total } | ResourceState::Interrupted { received, total } = r.state {
        body = body.child(el().h(2.0)).child(Progress::new(Some((received as f64 / total.max(1) as f64) as f32)).build(k));
    }
    if let ResourceState::Failed(m) = &r.state {
        body = body.child(text(m.clone()).font(k.ty.small).color(k.c.danger.fg).wrap());
    }
    if let Some(help) = &r.help {
        body = body.child(muted(k, help.clone()));
    }
    row().key(format!("res{i}")).w_full().gap(12.0).items_start().py(6.0).child(el().w(18.0).pt(2.0).child(badge)).child(body)
}

/// First launch: each resource's state, the downloads' progress and the checks' results.
pub fn installation(s: &App, k: &Kit) -> El {
    let resources = s.core.resources();
    let running = s.core.installing();
    let ready = resources.iter().all(|r| r.state == ResourceState::Ready);
    let to_install = resources.iter().any(|r| !matches!(r.state, ResourceState::Ready) && (r.total > 0 || r.help.is_none()));
    let to_download: u64 = resources
        .iter()
        .map(|r| match r.state {
            ResourceState::Ready => 0,
            ResourceState::Interrupted { received, total } | ResourceState::Downloading { received, total } => total.saturating_sub(received),
            _ => r.total,
        })
        .sum();

    let mut list = card(k).gap(4.0).child(heading(k, "Ressources"));
    for (i, r) in resources.iter().enumerate() {
        list = list.child(resource_row(k, i, r));
    }

    let install = if running {
        Button::new("Installation en cours…").variant(ButtonVariant::Loading).disabled(true).build(k)
    } else if to_install {
        let label = if to_download > 0 { format!("Tout installer ({} à télécharger)", ui::bytes(to_download)) } else { "Tout installer".into() };
        Button::new(label).variant(ButtonVariant::Submit).on_press(|s: &mut App| s.command(Command::InstallResources)).build(k)
    } else {
        Button::new("Commencer").variant(ButtonVariant::Submit).on_press(|s: &mut App| s.page = Page::Overview).build(k)
    };
    let mut actions = row().w_full().gap(10.0).center_y().child(install.key("install"));
    if !running {
        actions = actions.child(
            Button::new("Vérifier à nouveau")
                .variant(ButtonVariant::Outline)
                .disabled(s.core.verification_in_progress())
                .on_press(|s: &mut App| s.command(Command::CheckResources))
                .build(k)
                .key("verify"),
        );
    }
    if !ready {
        actions = actions.child(el().grow(1.0)).child(Button::new("Plus tard").variant(ButtonVariant::Ghost).on_press(|s: &mut App| s.page = Page::Overview).build(k).key("later"));
    }

    let folder = card(k)
        .child(heading(k, "Où vont les fichiers"))
        .child(row().gap(8.0).center_y().child(glyph(&ui::FOLDER, 14.0, k.c.icon_muted)).child(text("~/Library/Application Support/Greffier").font(k.ty.ui).color(k.c.text_base)))
        .child(muted(k, "Les modèles, le Python privé de pyannote et le lexique y sont rangés. Rien n'est installé ailleurs sur ce Mac, ni dans le terminal, ni dans ~/.cache. Un téléchargement interrompu reprend là où il s'est arrêté ; chaque fichier est vérifié par son sha256."));

    page(
        "installation",
        "Bienvenue dans Greffier",
        "Avant la première réunion, Greffier installe ses modèles et son environnement de calcul.",
        k,
        col().w_full().gap(16.0).child(list).child(actions).child(folder),
    )
}
