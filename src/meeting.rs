//! La page d'une réunion : en-tête, vidéo pleine largeur et ses contrôles, timeline, puis les
//! onglets Bilan / Transcription / À vérifier / Locuteurs / Infos. Avant le bilan, la page
//! montre le traitement, et dessous ce qui existe déjà (enregistrement, transcription).

use std::rc::Rc;

use rune::{Cx, El, ScrollSpec, Span, SurfaceFit, col, el, rich, row, text};
use rune_kit::{Button, ButtonVariant, ControlSize, IconButton, IconButtonVariant, Kit, Markdown, Progress, SegmentedControl, Spinner, Switch};

use greffier_coeur::fictif::PERSONNES;
use greffier_coeur::{Commande, Etape, Reunion, Role, Statut, StatutDuPassage, Tranche, bilan};

use crate::ui::{self, card, dot, eyebrow, glyph, heading, muted, spk_color, title_style, tnum, ts_chip};
use crate::{App, TABS, Tab, reglages, timeline};

/// Il y a de quoi revoir : un enregistrement, une transcription, ou une réunion terminée.
pub fn playable(m: &Reunion) -> bool {
    m.statut == Statut::Terminee || m.fichiers.contains(&Role::Enregistrement) || !m.repliques.is_empty()
}

pub fn page(s: &App, m: &Reunion, k: &Kit, cx: &mut Cx<App>) -> El {
    let view_h = cx.layout_size("meeting").map_or(780.0, |v| v[1]);
    let mut inner = col().w_full().px(28.0).pt(24.0).pb(32.0).gap(16.0).child(header(m, k));
    if m.statut != Statut::Terminee {
        inner = inner.child(pending(s, m, k));
    }
    if playable(m) {
        inner = inner
            .child(player(s, m, k, cx, view_h))
            .child(timeline::view(s, m, k, cx))
            .child(tabs_row(s, m, k))
            .child(match s.tab {
                Tab::Summary => summary(m, k),
                Tab::Transcript => transcript(s, m, k, view_h),
                Tab::Doubts => doubts(m, k),
                Tab::Speakers => speakers(s, m, k),
                Tab::Info => info(s, m, k),
            });
    }
    col().key("meeting").w_full().h_full().min_h(0.0).scroll(ScrollSpec { stick_bottom: false }).child(inner)
}

/// Le titre affiché : celui du bilan, sinon « Réunion du sam. 21/11 · 14h02 ».
pub fn display_title(m: &Reunion, with_time: bool) -> String {
    match m.titre.as_str() {
        "Réunion" | "" if with_time => format!("Réunion du {} · {}", m.debut.jour_court(), m.debut.heure_courte()),
        "Réunion" | "" => format!("Réunion du {}", m.debut.jour_court()),
        t => t.to_string(),
    }
}

fn header(m: &Reunion, k: &Kit) -> El {
    let title = display_title(m, true);
    let people = m.locuteurs_actifs().count();
    let mut meta = format!("{} · {} · {}", m.debut.jour_court(), m.debut.heure_courte(), ui::dur(m.duree));
    if people > 0 {
        meta += &format!(" · {people} locuteurs");
    }
    let id = m.id;
    col()
        .w_full()
        .gap(4.0)
        .child(text(title).font(title_style(22.0)).color(k.c.text_base).wrap().key("meeting-title").help("Changer le titre").on_press(move |s: &mut App| s.start_rename(id)))
        .child(text(meta).font(k.ty.small).color(k.c.text_muted))
}

/// L'image simulée de la vidéo, à la taille donnée : locuteur en cours, bouton lecture, sous-titres.
fn picture(s: &App, m: &Reunion, k: &Kit, w: f32, h: f32, big: bool) -> El {
    let cur = s.current.map(|i| &m.repliques[i]);
    let spk = cur.map(|u| &m.locuteurs[m.locuteur_affiche(u.locuteur)]);
    let caption = cur.filter(|u| s.pos <= u.fin + 1.0).map(|u| u.texte.clone()).unwrap_or_default();
    let speaking = spk.map(|p| p.affiche()).unwrap_or_else(|| "—".into());
    let hue = spk.map_or(250.0, |p| p.teinte);
    let white = rune::Color::WHITE;
    let play = if big { 84.0 } else { 56.0 };
    let frame = col().key("video").w(w).h(h);
    // La vraie vidéo, sinon (données fictives) une image simulée.
    let frame = match s.player() {
        Some(p) => frame.fill(rune::Color::BLACK).video(p, SurfaceFit::Fit),
        None => frame.gradient(rune::oklch(0.3, 0.03, 260.0), rune::oklch(0.17, 0.02, 260.0)),
    };
    frame
        .padding(if big { 28.0 } else { 16.0 })
        .justify_between()
        .child(if m.repliques.is_empty() {
            // Pas encore de transcription : personne à nommer.
            row().h(24.0)
        } else {
            row().gap(6.0).center_y().child(
                row().gap(6.0).center_y().px(8.0).py(4.0).radius(6.0).fill(rune::Color::BLACK.alpha(0.35))
                    .child(dot(spk_color(hue), 7.0))
                    .child(text(format!("Partage d'écran · {speaking}")).font(k.ty.text_xsmall_medium).color(white.alpha(0.85))),
            )
        })
        .child(
            row().w_full().justify_center().child(
                row().size(play, play).radius(play / 2.0).center()
                    .fill(white.alpha(if s.playing { 0.0 } else { 0.16 }))
                    .child(glyph(&ui::PLAY, play * 0.42, white.alpha(if s.playing { 0.0 } else { 0.9 }))),
            ),
        )
        .child(
            row().w_full().min_h(40.0).justify_center().items_end().child(if caption.is_empty() {
                el()
            } else {
                text(caption).font(if big { title_style(20.0) } else { k.ty.ui }).color(white).wrap().max_lines(2).truncate().align(rune::Align::Center)
                    .px(10.0).py(5.0).radius(6.0).fill(rune::Color::BLACK.alpha(0.55)).max_w(w * 0.8)
            }),
        )
}

/// La barre de contrôle sous la vidéo (ou en bas du plein écran).
fn controls(s: &App, m: &Reunion, k: &Kit, full: bool) -> El {
    let cur = s.current.map(|i| &m.repliques[i]);
    let spk = cur.map(|u| &m.locuteurs[m.locuteur_affiche(u.locuteur)]);
    let topic = m.sujet_a(s.pos).map(|i| m.sujets[i].titre.clone()).unwrap_or_default();
    let speaking = spk.map(|p| p.affiche()).unwrap_or_else(|| "—".into());
    let hue = spk.map_or(250.0, |p| p.teinte);
    let play = Button::new(if s.playing { "Pause" } else { "Lire" })
        .icon(if s.playing { &ui::PAUSE } else { &ui::PLAY })
        .variant(ButtonVariant::Contrast)
        .help("Espace")
        .on_press(|s: &mut App| s.toggle_play())
        .build(k)
        .key("play");
    let back = Button::new("10 s").icon(&ui::BACK).variant(ButtonVariant::Ghost).help("←").on_press(|s: &mut App| s.seek(s.pos - 10.0)).build(k).key("back10");
    let fwd = Button::new("10 s").icon(&ui::FORWARD).variant(ButtonVariant::Ghost).help("→").on_press(|s: &mut App| s.seek(s.pos + 10.0)).build(k).key("fwd10");
    let speed = SegmentedControl::new(&["1×", "1,5×", "2×"], Some(s.speed)).label("Vitesse").fit().on_pick(|s: &mut App, i, _| s.set_speed(i)).build(k).key("speed");
    let screen = IconButton::new(if full { &ui::SHRINK } else { &ui::EXPAND })
        .variant(IconButtonVariant::Ghost)
        .label(if full { "Quitter le plein écran" } else { "Plein écran" })
        .help(if full { "Quitter le plein écran (Échap)" } else { "Plein écran (F)" })
        .on_press_tx(|s: &mut App, tx| s.toggle_fullscreen(tx))
        .build(k)
        .key("fullscreen");
    row()
        .key("controls")
        .w_full()
        .px(12.0)
        .py(8.0)
        .gap(8.0)
        .center_y()
        .fill(k.c.layer_01)
        .child(back)
        .child(play)
        .child(fwd)
        .child(text(format!("{} / {}", ui::ts(s.pos), ui::ts(s.duration()))).font(tnum(k.ty.ui_medium)).color(k.c.text_base).px(8.0))
        .child(
            row().grow(1.0).min_w(0.0).gap(8.0).center_y()
                .child(dot(spk_color(hue), 8.0))
                .child(text(speaking).font(k.ty.small_medium).color(k.c.text_base))
                .child(text("·").font(k.ty.small).color(k.c.text_faint))
                .child(text(topic).font(k.ty.small).color(k.c.text_muted).truncate().min_w(0.0)),
        )
        .child(speed)
        .child(screen)
}

fn player(s: &App, m: &Reunion, k: &Kit, cx: &mut Cx<App>, view_h: f32) -> El {
    // La vidéo prend toute la largeur, sans dépasser un peu plus de la moitié de la hauteur
    // visible : au-delà, l'image garde son format 16:9 avec des bandes noires.
    let frame_w = cx.layout_size("video-frame").map_or(960.0, |v| v[0]);
    let h = (frame_w * 9.0 / 16.0).min(view_h * 0.58).max(220.0);
    let pic_w = (h * 16.0 / 9.0).min(frame_w);
    let frame = row()
        .key("video-frame")
        .w_full()
        .h(h)
        .justify_center()
        .fill(rune::Color::BLACK)
        .cursor(rune::Cursor::Pointer)
        .on_press(|s: &mut App| s.toggle_play())
        .on_double_press_tx(|s: &mut App, tx| s.toggle_fullscreen(tx))
        .child(picture(s, m, k, pic_w, h, false));
    col().w_full().radius(k.radius.lg).clip_rounded(k.radius.lg).border(1.0, k.c.border_muted).child(frame).child(controls(s, m, k, false))
}

/// Le plein écran : la vidéo sur toute la fenêtre, une barre de progression découpée par
/// sujet, et les contrôles en bas.
pub fn fullscreen(s: &App, k: &Kit, cx: &mut Cx<App>, layer: &mut Vec<El>) {
    if !s.fullscreen {
        return;
    }
    let Some(m) = s.meeting() else { return };
    let [w, h] = cx.layout_size("fs").unwrap_or([1360.0, 860.0]);
    let bar_h = 96.0;
    let vh = (h - bar_h).max(200.0);
    let pic_w = (vh * 16.0 / 9.0).min(w);
    let pic_h = pic_w * 9.0 / 16.0;
    let d = m.duree.max(1.0);
    let mut scrub = row().key("fs-scrub").w_full().h(14.0).gap(2.0).center_y().cursor(rune::Cursor::Pointer);
    for (i, tp) in m.sujets.iter().enumerate() {
        let end = m.sujets.get(i + 1).map_or(m.duree, |n| n.debut);
        let on = m.sujet_a(s.pos) == Some(i);
        let done = ((s.pos - tp.debut) / (end - tp.debut)).clamp(0.0, 1.0) as f32;
        scrub = scrub.child(
            row().grow((end - tp.debut) as f32).h(if on { 8.0 } else { 5.0 }).radius(3.0).fill(rune::Color::WHITE.alpha(0.22)).help(tp.titre.clone())
                .child(el().grow(done.max(0.0001)).h_full().radius(3.0).fill(k.c.text_accent))
                .child(el().grow((1.0 - done).max(0.0001))),
        );
    }
    let scrub = scrub.on_drag(move |s: &mut App, drag, _| {
        let f = (drag.x / drag.w.max(1.0)).clamp(0.0, 1.0);
        s.seek(f64::from(f) * d);
    });
    layer.push(
        col()
            .key("fs")
            .absolute(0.0, 0.0)
            .w_full()
            .h_full()
            .fill(rune::Color::BLACK)
            .child(
                row().w_full().grow(1.0).min_h(0.0).center()
                    .cursor(rune::Cursor::Pointer)
                    .on_press(|s: &mut App| s.toggle_play())
                    .on_double_press_tx(|s: &mut App, tx| s.toggle_fullscreen(tx))
                    .child(picture(s, m, k, pic_w, pic_h, true)),
            )
            .child(col().w_full().h(bar_h).px(20.0).pt(10.0).gap(6.0).fill(k.c.layer_01).child(scrub).child(controls(s, m, k, true).px(0.0).fill(rune::Color::TRANSPARENT))),
    );
}

fn tabs_row(s: &App, m: &Reunion, k: &Kit) -> El {
    let unconfirmed = m.locuteurs_actifs().filter(|&i| m.locuteurs[i].nom.is_none()).count();
    let speakers = if unconfirmed > 0 { format!("Locuteurs · {unconfirmed}") } else { "Locuteurs".to_string() };
    let labels = ["Bilan".to_string(), "Transcription".to_string(), format!("À vérifier · {}", m.passages_en_attente()), speakers, "Infos".to_string()];
    let tabs = SegmentedControl::new(&labels, TABS.iter().position(|t| *t == s.tab))
        .label("Vue")
        .fit()
        .on_pick(|s: &mut App, i, _| s.tab = TABS[i])
        .build(k)
        .key("tabs");
    let id = m.id;
    let trailing = match s.tab {
        Tab::Summary if m.statut != Statut::Terminee => el(),
        Tab::Summary => Button::new("Refaire le bilan")
            .icon(&ui::SPARK)
            .variant(ButtonVariant::Outline)
            .control_size(ControlSize::Small)
            .help("Relance seulement le bilan IA, avec les noms et corrections actuels")
            .on_press(move |s: &mut App| s.redo_bilan(id))
            .build(k)
            .key("redo"),
        Tab::Transcript => Switch::new(s.follow).label("Suivre la lecture").on_toggle(|s: &mut App| s.follow = !s.follow).build(k).key("follow"),
        _ => el(),
    };
    row().w_full().pt(8.0).gap(12.0).center_y().justify_between().child(tabs).child(trailing)
}

/// Le bilan est le Markdown de l'agent, rendu tel quel. Les horodatages deviennent des liens qui
/// lancent la lecture, et les cases des actions se cochent.
fn summary(m: &Reunion, k: &Kit) -> El {
    if m.bilan.trim().is_empty() {
        return col().key("bilan").w_full().pt(4.0).child(muted(k, "Pas encore de bilan."));
    }
    let src = Rc::new(bilan::avec_liens_de_lecture(&m.bilan));
    let tasks = src.clone();
    let id = m.id;
    let md = Markdown::new(src.as_str())
        .on_link(|s: &mut App, url: &str| {
            if let Some(t) = url.strip_prefix("t:").and_then(|x| x.parse::<f64>().ok()) {
                s.seek_and_play(t);
            }
        })
        .on_task(move |s: &mut App, _block: &str, offset: usize, done: bool| {
            let action = bilan::index_de_la_case(&tasks, offset);
            s.command(Commande::CocherAction { reunion: id, action, faite: done });
        })
        .build(k)
        .key("bilan-md");
    col().key("bilan").w_full().pt(4.0).child(md)
}

fn speaker_name(m: &Reunion, i: usize) -> (String, rune::Color, bool) {
    let p = &m.locuteurs[m.locuteur_affiche(i)];
    (p.affiche(), spk_color(p.teinte), p.nom.is_some())
}

fn utterance_text(m: &Reunion, i: usize, k: &Kit) -> El {
    let u = &m.repliques[i];
    let ink = k.c.text_base;
    if let Some(di) = u.passage {
        let d = &m.passages[di];
        let fixed = match &d.statut {
            StatutDuPassage::HypotheseAcceptee => d.hypothese.clone(),
            StatutDuPassage::CorrigeALaMain(t) => Some(t.clone()),
            _ => None,
        };
        match &d.statut {
            StatutDuPassage::HypotheseAcceptee | StatutDuPassage::CorrigeALaMain(_) => {
                let fixed = fixed.unwrap_or_default();
                let t = u.texte.replacen(&d.extrait, &fixed, 1);
                if let Some(a) = t.find(&fixed) {
                    return rich(t.clone(), vec![Span { underline: Some(k.c.success.icon), ..Span::new(a, a + fixed.len()) }]).font(k.ty.body).color(ink);
                }
            }
            StatutDuPassage::EnAttente => {
                if let Some(a) = u.texte.find(&d.extrait) {
                    let span = Span { bg: Some(k.c.warning.icon.alpha(0.28)), bg_pad: 2.0, ..Span::new(a, a + d.extrait.len()) };
                    return rich(u.texte.clone(), vec![span]).font(k.ty.body).color(ink);
                }
            }
            StatutDuPassage::OriginalGarde => {}
        }
    }
    text(u.texte.clone()).font(k.ty.body).color(ink).wrap()
}

fn transcript(s: &App, m: &Reunion, k: &Kit, view_h: f32) -> El {
    if m.repliques.is_empty() {
        return col().key("transcript-scroll").w_full().child(muted(k, "Pas encore de transcription."));
    }
    let mut lines = col().key("lines").w_full().gap(2.0);
    for (i, u) in m.repliques.iter().enumerate() {
        let (name, color, sure) = speaker_name(m, u.locuteur);
        let on = s.current == Some(i);
        let start = u.debut;
        lines = lines.child(
            row()
                .key(format!("u{i}"))
                .w_full()
                .gap(14.0)
                .px(10.0)
                .py(7.0)
                .radius(8.0)
                .items_start()
                .fill(if on { k.c.layer_02 } else { rune::Color::TRANSPARENT })
                .border_sides([0.0, 0.0, 0.0, 3.0], if on { color } else { rune::Color::TRANSPARENT })
                .hover(k.c.hover)
                .cursor(rune::Cursor::Pointer)
                .on_press(move |s: &mut App| s.seek(start))
                .on_double_press(move |s: &mut App| s.seek_and_play(start))
                .child(
                    col().w(116.0).gap(2.0)
                        .child(text(name).font(k.ty.small_medium).color(if sure { color } else { color.alpha(0.7) }).truncate())
                        .child(text(ui::ts(u.debut)).font(tnum(k.ty.text_xsmall)).color(k.c.text_faint)),
                )
                .child(utterance_text(m, i, k).grow(1.0).min_w(0.0)),
        );
    }
    // Sa propre zone de défilement, pour suivre la lecture sans faire défiler la vidéo.
    col()
        .key("transcript-scroll")
        .w_full()
        .h((view_h * 0.72).max(360.0))
        .radius(k.radius.lg)
        .border(1.0, k.c.border_muted)
        .scroll(ScrollSpec { stick_bottom: false })
        .scroll_edge_fade(12.0)
        .scroll_margin(120.0)
        .child(col().w_full().padding(8.0).child(lines))
}

fn doubts(m: &Reunion, k: &Kit) -> El {
    let total = m.passages.len();
    let left = m.passages_en_attente();
    let mut c = col().w_full().gap(12.0);
    c = c.child(
        col().w_full().gap(8.0)
            .child(text(format!("{left} passages à vérifier sur {total}")).font(k.ty.body_medium).color(k.c.text_base))
            .child(muted(k, "La relecture IA a corrigé seule ce qui était sûr. Restent ces passages : écoute, puis garde l'hypothèse de l'IA ou le texte d'origine."))
            .child(Progress::new(Some(if total == 0 { 1.0 } else { (total - left) as f32 / total as f32 })).build(k)),
    );
    if total == 0 {
        return c.child(muted(k, "Rien à vérifier."));
    }
    for (di, d) in m.passages.iter().enumerate() {
        let u = &m.repliques[d.replique];
        let (name, color, _) = speaker_name(m, u.locuteur);
        let start = u.debut;
        if d.statut != StatutDuPassage::EnAttente {
            let (label, fg) = match &d.statut {
                StatutDuPassage::HypotheseAcceptee => (format!("« {} » → « {} »", d.extrait, d.hypothese.clone().unwrap_or_default()), k.c.success.icon),
                StatutDuPassage::CorrigeALaMain(t) => (format!("« {} » → « {t} »", d.extrait), k.c.success.icon),
                _ => (format!("« {} » gardé tel quel", d.extrait), k.c.text_muted),
            };
            c = c.child(
                row().key(format!("dz{di}")).w_full().gap(10.0).center_y().px(12.0).py(8.0).radius(k.radius.md).fill(k.c.layer_01)
                    .child(glyph(&ui::CHECK, 13.0, fg))
                    .child(text(ui::ts(start)).font(tnum(k.ty.small)).color(k.c.text_faint))
                    .child(text(label).font(k.ty.small).color(k.c.text_muted).truncate().grow(1.0).min_w(0.0))
                    .child(Button::new("Annuler").variant(ButtonVariant::GhostFaint).control_size(ControlSize::Small)
                        .on_press(move |s: &mut App| s.resolve_doubt(di, Tranche::Rouvrir)).build(k)),
            );
            continue;
        }
        let accept = match &d.hypothese {
            Some(h) => Button::new(format!("Remplacer par « {h} »")).variant(ButtonVariant::Submit).control_size(ControlSize::Small)
                .on_press(move |s: &mut App| s.resolve_doubt(di, Tranche::AccepterHypothese)).build(k),
            None => Button::new("Pas d'hypothèse").variant(ButtonVariant::Outline).control_size(ControlSize::Small).disabled(true).build(k),
        };
        c = c.child(
            card(k).key(format!("dc{di}"))
                .child(row().gap(10.0).center_y()
                    .child(ts_chip(k, start, format!("dts{di}")))
                    .child(dot(color, 8.0))
                    .child(text(name).font(k.ty.small_medium).color(k.c.text_base)))
                .child(utterance_text(m, d.replique, k))
                .child(
                    col().gap(4.0).padding(10.0).radius(k.radius.md).fill(k.c.layer_02)
                        .child(row().gap(6.0).center_y().child(glyph(&ui::SPARK, 11.0, k.c.text_accent)).child(eyebrow(k, "Réécoute du passage")))
                        .child(text(d.reecoute.clone()).font(k.ty.small).color(k.c.text_muted).wrap()),
                )
                .child(row().gap(8.0).flex_wrap()
                    .child(Button::new("Écouter").icon(&ui::PLAY).variant(ButtonVariant::Outline).control_size(ControlSize::Small)
                        .on_press(move |s: &mut App| s.seek_and_play((start - 2.0).max(0.0))).build(k))
                    .child(accept)
                    .child(Button::new("Garder l'original").variant(ButtonVariant::Ghost).control_size(ControlSize::Small)
                        .on_press(move |s: &mut App| s.resolve_doubt(di, Tranche::GarderOriginal)).build(k))),
        );
    }
    c
}

fn speakers(s: &App, m: &Reunion, k: &Kit) -> El {
    let active: Vec<usize> = m.locuteurs_actifs().collect();
    let unconfirmed = active.iter().filter(|&&i| m.locuteurs[i].nom.is_none()).count();
    let intro = if unconfirmed > 0 {
        format!("{unconfirmed} noms proposés par l'IA à confirmer. Renommer met à jour la transcription ; « Refaire le bilan » les reprend.")
    } else {
        "Tous les locuteurs sont nommés.".to_string()
    };
    if active.is_empty() {
        return muted(k, "Pas encore de locuteurs : ils viennent avec la transcription.");
    }
    let mut grid = row().w_full().gap(12.0).flex_wrap().items_start();
    for &si in &active {
        grid = grid.child(speaker_card(s, m, si, k));
    }
    col().w_full().gap(12.0).child(muted(k, intro)).child(grid)
}

fn speaker_card(s: &App, m: &Reunion, si: usize, k: &Kit) -> El {
    let spk = &m.locuteurs[si];
    let id = m.id;
    let color = spk_color(spk.teinte);
    let share = (m.temps_de_parole(si) * 100.0).round().max(0.0);
    let mut card_ = col().key(format!("spk{si}")).grow(1.0).min_w(320.0).gap(8.0).padding(14.0).radius(k.radius.lg).fill(k.c.layer_01).border(1.0, k.c.border_muted)
        .child(
            row().gap(8.0).center_y()
                .child(dot(color, 10.0))
                .child(col().grow(1.0).min_w(0.0)
                    .child(text(spk.affiche()).font(k.ty.ui_medium).color(if spk.nom.is_some() { k.c.text_base } else { k.c.text_muted }))
                    .child(text(format!("{} · {share:.0} % du temps de parole", spk.etiquette)).font(k.ty.text_xsmall).color(k.c.text_faint)))
                .child(if spk.nom.is_some() { glyph(&ui::CHECK, 13.0, k.c.success.icon) } else { el() }),
        );
    if spk.nom.is_none() && !spk.justification.is_empty() {
        card_ = card_.child(row().gap(6.0).items_start().child(glyph(&ui::SPARK, 10.0, k.c.text_accent).mt(3.0)).child(muted(k, spk.justification.clone()).grow(1.0).min_w(0.0)));
    }
    let picking = s.picking == Some(si);
    let mut buttons = row().gap(6.0).flex_wrap();
    if let (None, None, Some(guess)) = (&spk.nom, spk.fusion_proposee, &spk.proposition) {
        let nom = guess.clone();
        buttons = buttons.child(Button::new(format!("Confirmer « {guess} »")).variant(ButtonVariant::Submit).control_size(ControlSize::Small)
            .on_press(move |s: &mut App| s.command(Commande::NommerLocuteur { reunion: id, locuteur: si, nom: nom.clone() })).build(k));
    }
    if let Some(target) = spk.fusion_proposee {
        let tname = m.locuteurs[target].affiche();
        buttons = buttons.child(Button::new(format!("Fusionner avec {tname}")).variant(ButtonVariant::Submit).control_size(ControlSize::Small)
            .on_press(move |s: &mut App| s.command(Commande::FusionnerLocuteurs { reunion: id, locuteur: si, dans: target })).build(k));
    }
    buttons = buttons.child(Button::new(if picking { "Fermer" } else { "Changer" }).variant(ButtonVariant::Ghost).control_size(ControlSize::Small)
        .on_press(move |s: &mut App| s.picking = if s.picking == Some(si) { None } else { Some(si) }).build(k));
    card_ = card_.child(buttons);
    if picking {
        let mut chips = row().gap(6.0).flex_wrap();
        for p in PERSONNES {
            chips = chips.child(
                row().h(24.0).px(9.0).center_y().radius(12.0).fill(k.c.layer_02).hover(k.c.hover).cursor(rune::Cursor::Pointer)
                    .child(text(p).font(k.ty.small_medium).color(k.c.text_base))
                    .on_press(move |s: &mut App| {
                        s.command(Commande::NommerLocuteur { reunion: id, locuteur: si, nom: p.to_string() });
                        s.picking = None;
                    }),
            );
        }
        card_ = card_.child(chips);
    }
    card_
}

fn info(s: &App, m: &Reunion, k: &Kit) -> El {
    let done = |e: Etape| m.etape_faite.is_some_and(|f| f >= e);
    let mut processing = card(k).w(0.0).grow(1.0).min_w(320.0).child(heading(k, "Traitement"));
    for (ok, label, detail) in [
        (m.fichiers.contains(&Role::Enregistrement), "Enregistrement", ui::dur(m.duree)),
        (done(Etape::Transcription), "Transcription", format!("{} répliques · {} locuteurs", m.repliques.len(), m.locuteurs_actifs().count())),
        (done(Etape::Relecture), "Relecture IA", format!("{} corrections · {} à vérifier", m.corrections, m.passages_en_attente())),
        (done(Etape::Bilan), "Bilan IA", format!("{} sujets · {} décisions", m.sujets.len(), m.decisions.len())),
    ] {
        processing = processing.child(ui::check_row(k, ok, label, &detail));
    }
    let folder = format!("{}/", reglages::afficher(&s.core.dossier().dossier(&m.nom_du_dossier)));
    let mut files = card(k).w(0.0).grow(1.0).min_w(320.0).child(heading(k, "Fichiers dans le dossier de réunions"))
        .child(row().gap(6.0).center_y().child(glyph(&ui::FOLDER, 13.0, k.c.icon_muted)).child(text(folder).font(k.ty.small).color(k.c.text_muted).truncate_middle().min_w(0.0)));
    for role in Role::TOUS {
        let present = m.fichiers.contains(&role);
        let name = format!("{} {} - {}", if present { "·" } else { "–" }, m.nom_du_dossier, role.suffixe());
        files = files.child(text(name).font(k.ty.small).color(if present { k.c.text_muted } else { k.c.text_faint.alpha(0.5) }).truncate_middle().min_w(0.0).pl(20.0));
    }
    let mut marks = card(k).w(0.0).grow(1.0).min_w(320.0).child(heading(k, "Repères"));
    if m.reperes.is_empty() {
        marks = marks.child(muted(k, "Aucun repère posé pendant l'enregistrement."));
    }
    for (i, mk) in m.reperes.iter().enumerate() {
        marks = marks.child(row().gap(8.0).center_y().child(ts_chip(k, mk.instant, format!("mk-ts{i}"))).child(text(mk.libelle.clone()).font(k.ty.ui).color(k.c.text_base)));
    }
    row().w_full().gap(12.0).flex_wrap().items_start().child(processing).child(files).child(marks)
}

fn pending(s: &App, m: &Reunion, k: &Kit) -> El {
    let id = m.id;
    // Une transcription importée : le traitement reprend à la relecture (#5).
    if m.statut == Statut::ATraiter && m.etape_faite == Some(Etape::Transcription) {
        return card(k)
            .gap(14.0)
            .child(row().gap(10.0).center_y().child(glyph(&ui::WAVE, 18.0, k.c.text_accent)).child(heading(k, "Transcription importée, pas encore relue")))
            .child(muted(k, "La relecture et le bilan partent vers Claude. La transcription est reprise telle quelle."))
            .child(row().gap(8.0)
                .child(Button::new("Relire et faire le bilan").icon(&ui::SPARK).variant(ButtonVariant::Submit)
                    .on_press(move |s: &mut App| s.process(id)).build(k).key("process")));
    }
    if m.statut == Statut::ATraiter {
        let people = m.nombre_de_locuteurs;
        let stepper = row().gap(6.0).center_y()
            .child(Button::new("−").variant(ButtonVariant::Outline).control_size(ControlSize::Small)
                .on_press(move |s: &mut App| s.command(Commande::NombreDeLocuteurs { reunion: id, nombre: match people { Some(n) if n > 2 => Some(n - 1), _ => None } })).build(k).key("ppl-minus"))
            .child(text(people.map_or("Détecté".to_string(), |n| format!("{n} personnes"))).font(k.ty.ui_medium).color(k.c.text_base).w(100.0).align(rune::Align::Center))
            .child(Button::new("+").variant(ButtonVariant::Outline).control_size(ControlSize::Small)
                .on_press(move |s: &mut App| s.command(Commande::NombreDeLocuteurs { reunion: id, nombre: Some(people.map_or(2, |n| (n + 1).min(12))) })).build(k).key("ppl-plus"));
        return card(k)
            .gap(14.0)
            .child(row().gap(10.0).center_y().child(glyph(&ui::WAVE, 18.0, k.c.text_accent)).child(heading(k, "Enregistrée, pas encore traitée")))
            .child(muted(k, "La transcription et les locuteurs tournent sur ton Mac. Seuls la relecture et le bilan partent vers Claude. Compte environ 1 min 30 de transcription pour 10 min de réunion."))
            .child(row().gap(12.0).center_y().child(text("Personnes").font(k.ty.small_medium).color(k.c.text_muted).w(80.0)).child(stepper))
            .child(muted(k, "Le bon nombre de personnes améliore beaucoup la séparation des locuteurs. Laisse « Détecté » si tu ne sais pas."))
            .child(row().gap(8.0)
                .child(Button::new("Transcrire et faire le bilan").icon(&ui::SPARK).variant(ButtonVariant::Submit)
                    .on_press(move |s: &mut App| s.process(id)).build(k).key("process")));
    }
    let steps = Etape::TOUTES;
    let details = [
        "Whisper large-v3 et pyannote, sur le Mac",
        "Claude corrige, réécoute les passages douteux",
        "Claude remplit le template du bilan, en Markdown",
    ];
    let idx = match &m.statut {
        Statut::EnCours { etape, .. } => steps.iter().position(|x| x == etape),
        _ => None,
    };
    let mut list = col().gap(12.0);
    for (i, st) in steps.iter().enumerate() {
        let state = match idx {
            Some(j) if i < j => 2,
            Some(j) if i == j => 1,
            _ => 0,
        };
        let icon: El = match state {
            2 => glyph(&ui::CHECK, 14.0, k.c.success.icon),
            1 => Spinner::new().size(14.0).build(k),
            _ => dot(k.c.border_strong, 8.0).mx(3.0),
        };
        let mut col_ = col().gap(6.0).grow(1.0).min_w(0.0)
            .child(row().gap(8.0).center_y()
                .child(text(ui::step(*st)).font(k.ty.ui_medium).color(if state == 0 { k.c.text_faint } else { k.c.text_base }))
                .child(text(details[i]).font(k.ty.small).color(k.c.text_faint).truncate().min_w(0.0)));
        if state == 1 {
            col_ = col_.child(Progress::new(Some(m.statut.progres() as f32)).build(k));
        }
        list = list.child(row().key(format!("step{i}")).w_full().gap(12.0).items_start().child(row().w(18.0).h(18.0).center().child(icon)).child(col_));
    }
    let eta = ui::dur(s.core.temps_restant(id).unwrap_or(0.0).max(60.0));
    let (status, action) = match &m.statut {
        Statut::EnAttente(_) => match s.core.rang(id).unwrap_or(0) {
            0 | 1 => (format!("En attente : une autre réunion est en cours de traitement. Environ {eta} avant la fin."), None),
            n => (format!("En attente : {n} réunions passent avant celle-ci. Environ {eta} avant la fin."), None),
        },
        Statut::Echec { etape, message } => (format!("{} : {message}", ui::step(*etape)), Some("Réessayer cette étape")),
        _ => (format!("Environ {eta} restantes. Une notification arrive à la fin."), None),
    };
    let button = match action {
        Some(label) => Button::new(label).variant(ButtonVariant::Outline).control_size(ControlSize::Small).on_press(move |s: &mut App| s.process(id)).build(k).key("retry"),
        None => Button::new("Arrêter").variant(ButtonVariant::GhostMuted).control_size(ControlSize::Small).on_press(move |s: &mut App| s.stop_processing(id)).build(k).key("stop-proc"),
    };
    card(k)
        .gap(16.0)
        .child(row().gap(10.0).center_y().child(heading(k, if matches!(m.statut, Statut::Echec { .. }) { "Traitement en échec" } else { "Traitement en cours" })).child(el().grow(1.0)).child(button))
        .child(muted(k, status))
        .child(list)
}
