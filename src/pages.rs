//! Les autres pages : premier lancement (dossier de réunions, ressources), vue d'ensemble,
//! enregistrement, réglages, et le bandeau de notification.

use rune::{El, ScrollSpec, col, el, row, text};
use rune_kit::{Button, ButtonVariant, ControlSize, Keybind, Kit, Progress, ShortcutRecorder, Spinner, Switch};

use greffier_coeur::adaptateurs::{Autorisation, EtatDAutorisation, EtatDeRessource, Ressource};
use greffier_coeur::{Commande, Statut, bilan};

use crate::ui::{self, card, check_row, eyebrow, glyph, heading, muted, red, title_style, tnum};
use crate::{App, Page, reglages};

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

/// Les barres d'un niveau : leur hauteur suit le niveau (0 à 1, `None` : source coupée), leur
/// ondulation l'animation.
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

/// Les autorisations de macOS dont les sources ont besoin, avec de quoi les demander.
pub fn permissions(s: &App, k: &Kit) -> El {
    let etats = s.core.autorisations();
    let mut c = card(k).key("permissions").child(heading(k, "Autorisations de macOS"));
    for a in Autorisation::TOUTES {
        let etat = etats.etat(a);
        let detail = match etat {
            EtatDAutorisation::Accordee => "accordée".to_string(),
            EtatDAutorisation::ADemander => "pas encore demandée".to_string(),
            EtatDAutorisation::Refusee => format!("refusée : Réglages Système › Confidentialité et sécurité › {}", a.rubrique()),
        };
        let mut line = row().key(format!("perm-{}", a.nom())).w_full().gap(8.0).center_y().child(check_row(k, etat == EtatDAutorisation::Accordee, a.nom(), &detail).grow(1.0).min_w(0.0));
        if etat != EtatDAutorisation::Accordee {
            let label = if etat == EtatDAutorisation::ADemander { "Autoriser" } else { "Ouvrir les Réglages" };
            line = line.child(Button::new(label).variant(ButtonVariant::Outline).control_size(ControlSize::Small).on_press(move |s: &mut App| s.command(Commande::DemanderAutorisation(a))).build(k));
        }
        c = c.child(line);
    }
    if etats.ecran != EtatDAutorisation::Accordee {
        c = c.child(muted(k, "Le son du Mac passe par l'autorisation de l'écran. Une fois accordée, macOS demande de relancer Greffier."));
    }
    c
}

/// Les autorisations en une ligne, pour les vérifications.
fn permissions_summary(s: &App, k: &Kit) -> El {
    let etats = s.core.autorisations();
    let manquantes: Vec<&str> = Autorisation::TOUTES.into_iter().filter(|a| etats.etat(*a) != EtatDAutorisation::Accordee).map(Autorisation::nom).collect();
    let detail = if manquantes.is_empty() { "écran, micro, son du système".to_string() } else { format!("manquantes : {}", manquantes.join(", ").to_lowercase()) };
    check_row(k, manquantes.is_empty(), "Autorisations macOS", &detail)
}

/// Les touches du raccourci global d'enregistrement (« ⇧⌘R »).
pub fn shortcut_caps(s: &App) -> Vec<String> {
    s.shortcut.shortcut().map(|sc| sc.keycaps()).unwrap_or_default()
}

pub fn recording(s: &App, k: &Kit) -> El {
    let Some(r) = s.core.enregistrement() else {
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
            .child(Switch::new(s.core.reglages().traiter_a_l_arret).label("Transcrire et faire le bilan à l'arrêt").on_toggle(|s: &mut App| s.toggle_auto_process()).build(k));
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
        .child(text(ui::ts(r.duree)).key("rec-timer").font(tnum(title_style(56.0))).color(k.c.text_base))
        .child(meter(k, "lvl-mic", &ui::MIC, "Micro", phase, 0.0, r.sources.micro.then_some(r.niveaux.micro)))
        .child(meter(k, "lvl-sys", &ui::SOUND, "Son du Mac", phase, 2.1, r.sources.son_du_mac.then_some(r.niveaux.son_du_mac)))
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
        .child(check_row(k, r.sources.ecran, "Écran", if r.sources.ecran { "Écran principal · 30 i/s" } else { "coupé" }))
        .child(check_row(k, r.sources.son_du_mac, "Son du Mac", if r.sources.son_du_mac { "Meet, Zoom, toute l'audio du système" } else { "coupé" }))
        .child(check_row(k, r.sources.micro, "Micro", if r.sources.micro { "Micro par défaut du Mac" } else { "coupé" }))
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
        .child(
            row().w_full().justify_between().center_y()
                .child(col().gap(2.0).child(text("Raccourci global").font(k.ty.ui).color(k.c.text_base)).child(muted(k, "Démarre et arrête l'enregistrement, même app en arrière-plan.")))
                .child(ShortcutRecorder::new("record-shortcut", &s.shortcut, |s: &mut App| &mut s.shortcut).on_change(|s: &mut App, next, _| s.change_shortcut(next)).build(k)),
        )
        .child(
            row().w_full().justify_between().center_y()
                .child(col().gap(2.0).child(text("Poser un repère").font(k.ty.ui).color(k.c.text_base)).child(muted(k, "De n'importe où, pendant l'enregistrement seulement.")))
                .child(Keybind::new(&["⌘", "M"]).build(k)),
        )
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
    let l = &s.lexique;
    let ai = card(k)
        .child(heading(k, "IA"))
        .child(
            row().w_full().gap(10.0).center_y()
                .child(check_row(k, true, "Lexique", &format!("{} erreurs connues, {} personnes, {} mots justes", l.erreurs_connues, l.personnes, l.mots_justes)).grow(1.0).min_w(0.0))
                .child(Button::new("Ouvrir le lexique").variant(ButtonVariant::Outline).control_size(ControlSize::Small)
                    .help("Le lexique s'ouvre dans l'éditeur de texte par défaut ; la relecture y ajoute les erreurs qu'elle apprend")
                    .on_press(|s: &mut App| s.open_lexique()).build(k).key("open-lexique")),
        )
        .child(check_row(k, true, "Prompt de relecture", "prompt-relecture.md"))
        .child(check_row(k, true, "Prompt du bilan", "prompt-bilan.md"));
    let ressources = s.core.ressources();
    let place: u64 = ressources.iter().map(|r| r.taille).sum();
    let mut health = card(k)
        .child(row().w_full().justify_between().center_y().child(heading(k, "Vérifications")).child(eyebrow(k, &format!("ressources : {}", ui::octets(place)))))
        .child(permissions_summary(s, k));
    for r in ressources {
        let detail = match &r.etat {
            EtatDeRessource::Prete if r.taille > 0 => format!("{} · {}", ui::octets(r.taille), r.detail),
            EtatDeRessource::Prete => r.detail.clone(),
            etat => format!("{} · {}", etat_de_ressource(r, etat), r.detail),
        };
        health = health.child(check_row(k, r.etat == EtatDeRessource::Prete, &r.nom, &detail));
    }
    let health = health
        .child(check_row(k, true, "Espace disque", "811 Go libres"))
        .child(
            row().w_full().gap(10.0).center_y().pt(4.0)
                .child(muted(k, "Modèles, Python privé et lexique sont dans ~/Library/Application Support/Greffier.").grow(1.0).min_w(0.0))
                .child(Button::new("Ressources…").variant(ButtonVariant::Outline).control_size(ControlSize::Small).on_press(|s: &mut App| s.page = Page::Installation).build(k).key("open-resources")),
        );
    page("settings", "Réglages", "Sources, raccourcis, stockage et santé de l'installation.", k, col().w_full().gap(16.0).child(rec).child(permissions(s, k)).child(storage).child(ai).child(health))
}

/// L'état d'une ressource en quelques mots.
fn etat_de_ressource(r: &Ressource, etat: &EtatDeRessource) -> String {
    let pourcent = |recu: u64, total: u64| (recu as f64 / total.max(1) as f64 * 100.0).floor() as u64;
    match etat {
        EtatDeRessource::Absente if r.total > 0 => format!("À télécharger · {}", ui::octets(r.total)),
        EtatDeRessource::Absente if r.aide.is_some() => "Introuvable".into(),
        EtatDeRessource::Absente => "À installer".into(),
        EtatDeRessource::Interrompue { recu, total } => format!("Interrompu à {} % · reprendra", pourcent(*recu, *total)),
        EtatDeRessource::EnTelechargement { recu, total } => format!("{} sur {} · {} %", ui::octets(*recu), ui::octets(*total), pourcent(*recu, *total)),
        EtatDeRessource::EnVerification => "Vérification…".into(),
        EtatDeRessource::EnInstallation => "Installation…".into(),
        EtatDeRessource::Prete if r.taille > 0 => format!("Prête · {}", ui::octets(r.taille)),
        EtatDeRessource::Prete => "Prête".into(),
        EtatDeRessource::Echec(_) => "Échec".into(),
    }
}

fn ligne_de_ressource(k: &Kit, i: usize, r: &Ressource) -> El {
    let marque = match &r.etat {
        EtatDeRessource::Prete => glyph(&ui::CHECK, 15.0, k.c.success.icon),
        EtatDeRessource::EnTelechargement { .. } | EtatDeRessource::EnVerification | EtatDeRessource::EnInstallation => Spinner::new().size(14.0).build(k),
        EtatDeRessource::Echec(_) => glyph(&ui::FLAG, 15.0, k.c.danger.icon),
        _ => glyph(&ui::FLAG, 15.0, k.c.warning.icon),
    };
    let couleur_etat = match &r.etat {
        EtatDeRessource::Prete => k.c.success.fg,
        EtatDeRessource::Echec(_) => k.c.danger.fg,
        _ => k.c.text_muted,
    };
    let mut texte = col()
        .grow(1.0)
        .min_w(0.0)
        .gap(3.0)
        .child(
            row().w_full().gap(10.0).center_y()
                .child(text(r.nom.clone()).font(k.ty.ui_medium).color(k.c.text_base).grow(1.0).min_w(0.0).truncate())
                .child(text(etat_de_ressource(r, &r.etat)).font(tnum(k.ty.small_medium)).color(couleur_etat)),
        )
        .child(text(r.detail.clone()).font(k.ty.small).color(k.c.text_faint).truncate());
    if let EtatDeRessource::EnTelechargement { recu, total } | EtatDeRessource::Interrompue { recu, total } = r.etat {
        texte = texte.child(el().h(2.0)).child(Progress::new(Some((recu as f64 / total.max(1) as f64) as f32)).build(k));
    }
    if let EtatDeRessource::Echec(m) = &r.etat {
        texte = texte.child(text(m.clone()).font(k.ty.small).color(k.c.danger.fg).wrap());
    }
    if let Some(aide) = &r.aide {
        texte = texte.child(muted(k, aide.clone()));
    }
    row().key(format!("res{i}")).w_full().gap(12.0).items_start().py(6.0).child(el().w(18.0).pt(2.0).child(marque)).child(texte)
}

/// Le premier lancement : l'état de chaque ressource, la progression des téléchargements et le
/// résultat des vérifications.
pub fn installation(s: &App, k: &Kit) -> El {
    let ressources = s.core.ressources();
    let en_cours = s.core.installation_en_cours();
    let pretes = ressources.iter().all(|r| r.etat == EtatDeRessource::Prete);
    let a_installer = ressources.iter().any(|r| !matches!(r.etat, EtatDeRessource::Prete) && (r.total > 0 || r.aide.is_none()));
    let a_telecharger: u64 = ressources
        .iter()
        .map(|r| match r.etat {
            EtatDeRessource::Prete => 0,
            EtatDeRessource::Interrompue { recu, total } | EtatDeRessource::EnTelechargement { recu, total } => total.saturating_sub(recu),
            _ => r.total,
        })
        .sum();

    let mut liste = card(k).gap(4.0).child(heading(k, "Ressources"));
    for (i, r) in ressources.iter().enumerate() {
        liste = liste.child(ligne_de_ressource(k, i, r));
    }

    let installer = if en_cours {
        Button::new("Installation en cours…").variant(ButtonVariant::Loading).disabled(true).build(k)
    } else if a_installer {
        let libelle = if a_telecharger > 0 { format!("Tout installer ({} à télécharger)", ui::octets(a_telecharger)) } else { "Tout installer".into() };
        Button::new(libelle).variant(ButtonVariant::Submit).on_press(|s: &mut App| s.command(Commande::InstallerRessources)).build(k)
    } else {
        Button::new("Commencer").variant(ButtonVariant::Submit).on_press(|s: &mut App| s.page = Page::Overview).build(k)
    };
    let mut actions = row().w_full().gap(10.0).center_y().child(installer.key("install"));
    if !en_cours {
        actions = actions.child(
            Button::new("Vérifier à nouveau")
                .variant(ButtonVariant::Outline)
                .disabled(s.core.verification_en_cours())
                .on_press(|s: &mut App| s.command(Commande::VerifierRessources))
                .build(k)
                .key("verify"),
        );
    }
    if !pretes {
        actions = actions.child(el().grow(1.0)).child(Button::new("Plus tard").variant(ButtonVariant::Ghost).on_press(|s: &mut App| s.page = Page::Overview).build(k).key("later"));
    }

    let dossier = card(k)
        .child(heading(k, "Où vont les fichiers"))
        .child(row().gap(8.0).center_y().child(glyph(&ui::FOLDER, 14.0, k.c.icon_muted)).child(text("~/Library/Application Support/Greffier").font(k.ty.ui).color(k.c.text_base)))
        .child(muted(k, "Les modèles, le Python privé de pyannote et le lexique y sont rangés. Rien n'est installé ailleurs sur ce Mac, ni dans le terminal, ni dans ~/.cache. Un téléchargement interrompu reprend là où il s'est arrêté ; chaque fichier est vérifié par son sha256."));

    page(
        "installation",
        "Bienvenue dans Greffier",
        "Avant la première réunion, Greffier installe ses modèles et son environnement de calcul.",
        k,
        col().w_full().gap(16.0).child(liste).child(actions).child(dossier),
    )
}
