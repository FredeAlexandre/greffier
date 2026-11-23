//! A meeting's page: header, full-width video and its controls, timeline, then the tabs Bilan /
//! Transcription / À vérifier / Locuteurs / Infos. Before the summary, the page shows the
//! processing, and below it what already exists (recording, transcription).

use std::rc::Rc;

use rune::{Cx, El, ScrollSpec, Span, SurfaceFit, col, el, rich, row, text};
use rune_kit::{Button, ButtonVariant, ControlSize, IconButton, IconButtonVariant, Kit, Markdown, Progress, SegmentedControl, Spinner, Switch};

use greffier_core::{Command, Stage, Meeting, Role, Status, PassageStatus, summary};

use crate::ui::{self, card, dot, glyph, heading, muted, spk_color, title_style, tnum, ts_chip};
use crate::check::{self, Target};
use crate::{App, TABS, Tab, speakers, redo, settings, timeline};

/// There is something to review: a recording, a transcription, or a finished meeting.
pub fn playable(m: &Meeting) -> bool {
    m.status == Status::Done || m.files.contains(&Role::Recording) || !m.utterances.is_empty()
}

pub fn page(s: &App, m: &Meeting, k: &Kit, cx: &mut Cx<App>, layer: &mut Vec<El>) -> El {
    let view_h = cx.layout_size("meeting").map_or(780.0, |v| v[1]);
    let mut inner = col().w_full().px(28.0).pt(24.0).pb(32.0).gap(16.0).child(header(m, k));
    if m.status != Status::Done {
        inner = inner.child(pending(s, m, k));
    }
    if playable(m) {
        inner = inner
            .child(player(s, m, k, cx, view_h))
            .child(timeline::view(s, m, k, cx))
            .child(tabs_row(s, m, k, layer))
            .child(match s.tab {
                Tab::Summary => summary(m, k),
                Tab::Transcript => transcript(s, m, k, view_h),
                Tab::Doubts => check::tab(s, m, k),
                Tab::Speakers => speakers::tab(s, m, k),
                Tab::Info => info(s, m, k),
            });
    }
    col().key("meeting").w_full().h_full().min_h(0.0).scroll(ScrollSpec { stick_bottom: false }).child(inner)
}

/// The displayed title: the summary's, otherwise « Réunion du sam. 21/11 · 14h02 ».
pub fn display_title(m: &Meeting, with_time: bool) -> String {
    match m.title.as_str() {
        "Réunion" | "" if with_time => format!("Réunion du {} · {}", m.start.short_day(), m.start.short_time()),
        "Réunion" | "" => format!("Réunion du {}", m.start.short_day()),
        t => t.to_string(),
    }
}

fn header(m: &Meeting, k: &Kit) -> El {
    let title = display_title(m, true);
    let people = m.active_speakers().count();
    let mut meta = format!("{} · {} · {}", m.start.short_day(), m.start.short_time(), ui::dur(m.duration));
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

/// The simulated video picture, at the given size: current speaker, play button, captions.
fn picture(s: &App, m: &Meeting, k: &Kit, w: f32, h: f32, big: bool) -> El {
    let cur = s.current.map(|i| &m.utterances[i]);
    let spk = cur.map(|u| &m.speakers[m.displayed_speaker(u.speaker)]);
    let caption = cur.filter(|u| s.pos <= u.end + 1.0).map(|u| u.text.clone()).unwrap_or_default();
    let speaking = spk.map(|p| p.display_name()).unwrap_or_else(|| "—".into());
    let hue = spk.map_or(250.0, |p| p.tint);
    let white = rune::Color::WHITE;
    let play = if big { 84.0 } else { 56.0 };
    let frame = col().key("video").w(w).h(h);
    // The real video, otherwise (fake data) a simulated picture.
    let frame = match s.player() {
        Some(p) => frame.fill(rune::Color::BLACK).video(p, SurfaceFit::Fit),
        None => frame.gradient(rune::oklch(0.3, 0.03, 260.0), rune::oklch(0.17, 0.02, 260.0)),
    };
    frame
        .padding(if big { 28.0 } else { 16.0 })
        .justify_between()
        .child(if m.utterances.is_empty() {
            // No transcription yet: nobody to name.
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

/// The control bar under the video (or at the bottom in full screen).
fn controls(s: &App, m: &Meeting, k: &Kit, full: bool) -> El {
    let cur = s.current.map(|i| &m.utterances[i]);
    let spk = cur.map(|u| &m.speakers[m.displayed_speaker(u.speaker)]);
    let topic = m.topic_at(s.pos).map(|i| m.topics[i].title.clone()).unwrap_or_default();
    let speaking = spk.map(|p| p.display_name()).unwrap_or_else(|| "—".into());
    let hue = spk.map_or(250.0, |p| p.tint);
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

fn player(s: &App, m: &Meeting, k: &Kit, cx: &mut Cx<App>, view_h: f32) -> El {
    // The video takes the full width, up to a little more than half the visible height: beyond
    // that, the picture keeps its 16:9 ratio with black bars.
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

/// Full screen: the video over the whole window, a progress bar split by topic, and the controls
/// at the bottom.
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
    let d = m.duration.max(1.0);
    let mut scrub = row().key("fs-scrub").w_full().h(14.0).gap(2.0).center_y().cursor(rune::Cursor::Pointer);
    for (i, tp) in m.topics.iter().enumerate() {
        let end = m.topics.get(i + 1).map_or(m.duration, |n| n.start);
        let on = m.topic_at(s.pos) == Some(i);
        let done = ((s.pos - tp.start) / (end - tp.start)).clamp(0.0, 1.0) as f32;
        scrub = scrub.child(
            row().grow((end - tp.start) as f32).h(if on { 8.0 } else { 5.0 }).radius(3.0).fill(rune::Color::WHITE.alpha(0.22)).help(tp.title.clone())
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

fn tabs_row(s: &App, m: &Meeting, k: &Kit, layer: &mut Vec<El>) -> El {
    let unconfirmed = m.active_speakers().filter(|&i| m.speakers[i].name.is_none()).count();
    let speakers = if unconfirmed > 0 { format!("Locuteurs · {unconfirmed}") } else { "Locuteurs".to_string() };
    let labels = ["Bilan".to_string(), "Transcription".to_string(), format!("À vérifier · {}", m.pending_passages()), speakers, "Infos".to_string()];
    let tabs = SegmentedControl::new(&labels, TABS.iter().position(|t| *t == s.tab))
        .label("Vue")
        .fit()
        // Picked with the pointer, focus leaves the strip; the arrows keep it there.
        .on_pick(|s: &mut App, i, tx| if tx.by_keys() { s.tab = TABS[i] } else { s.show_tab(TABS[i], tx) })
        .build(k)
        .key("tabs");
    let id = m.id;
    let trailing = match s.tab {
        Tab::Summary if !redo::can_redo(m, Stage::Summary) => el(),
        Tab::Summary => Button::new("Refaire le bilan")
            .icon(&ui::SPARK)
            .variant(ButtonVariant::Outline)
            .control_size(ControlSize::Small)
            .help("Relance seulement le bilan IA, avec les noms et corrections actuels")
            .on_press(move |s: &mut App| s.redo(id, Stage::Summary))
            .build(k)
            .key("redo"),
        Tab::Transcript => {
            let follow = Switch::new(s.follow).label("Suivre la lecture").on_toggle(|s: &mut App| s.follow = !s.follow).build(k).key("follow");
            let trailing = row()
                .gap(16.0)
                .center_y()
                .child(text("Clic droit ou C : corriger la réplique").font(k.ty.small).color(k.c.text_faint))
                .child(follow);
            match redo::transcript_menu(s, m, k, layer) {
                Some(buttons) => row().gap(16.0).center_y().flex_wrap().child(buttons).child(trailing),
                None => trailing,
            }
        }
        _ => el(),
    };
    row().w_full().pt(8.0).gap(12.0).center_y().justify_between().flex_wrap().child(tabs).child(trailing)
}

/// The summary is the agent's Markdown, rendered as is. Timestamps become links that start
/// playback, and the actions' checkboxes can be ticked.
fn summary(m: &Meeting, k: &Kit) -> El {
    if m.summary.trim().is_empty() {
        return col().key("summary").w_full().pt(4.0).child(muted(k, "Pas encore de bilan."));
    }
    let src = Rc::new(summary::with_playback_links(&m.summary));
    let tasks = src.clone();
    let id = m.id;
    let md = Markdown::new(src.as_str())
        .on_link_tx(|s: &mut App, url: &str, tx| {
            if let Some(t) = url.strip_prefix("t:").and_then(|x| x.parse::<f64>().ok()) {
                s.seek_and_play(t);
                // The video is above, often off screen: bring it back to watch the moment.
                tx.scroll_into_view_in("meeting", "video-frame");
            }
        })
        .on_task(move |s: &mut App, _block: &str, offset: usize, done: bool| {
            let action = summary::checkbox_index(&tasks, offset);
            s.command(Command::CheckAction { meeting: id, action, done });
        })
        .build(k)
        .key("summary-md");
    col().key("summary").w_full().pt(4.0).child(md)
}

pub fn speaker_name(m: &Meeting, i: usize) -> (String, rune::Color, bool) {
    let p = &m.speakers[m.displayed_speaker(i)];
    (p.display_name(), spk_color(p.tint), p.name.is_some())
}

/// An utterance's text. The core has already put in it what its passages to check became:
/// highlighted while pending, underlined once replaced.
pub fn utterance_text(m: &Meeting, i: usize, k: &Kit) -> El {
    let u = &m.utterances[i];
    let ink = k.c.text_base;
    let mut spans: Vec<Span> = Vec::new();
    for d in u.passages.iter().filter_map(|&di| m.passages.get(di)) {
        let shown = d.text(&d.status);
        let Some(a) = u.text.find(shown) else { continue };
        if spans.iter().any(|s| (a as u32) < s.end && s.start < (a + shown.len()) as u32) {
            continue;
        }
        match &d.status {
            PassageStatus::HypothesisAccepted | PassageStatus::CorrectedManually(_) => spans.push(Span { underline: Some(k.c.success.icon), ..Span::new(a, a + shown.len()) }),
            PassageStatus::Pending => spans.push(Span { bg: Some(k.c.warning.icon.alpha(0.28)), bg_pad: 2.0, ..Span::new(a, a + shown.len()) }),
            PassageStatus::OriginalKept => {}
        }
    }
    if spans.is_empty() {
        return text(u.text.clone()).font(k.ty.body).color(ink).wrap();
    }
    spans.sort_by_key(|s| s.start);
    rich(u.text.clone(), spans).font(k.ty.body).color(ink)
}

fn transcript(s: &App, m: &Meeting, k: &Kit, view_h: f32) -> El {
    if m.utterances.is_empty() {
        return col().key("transcript-scroll").w_full().child(muted(k, "Pas encore de transcription."));
    }
    let mut lines = col().key("lines").w_full().gap(2.0);
    for (i, u) in m.utterances.iter().enumerate() {
        let (name, color, sure) = speaker_name(m, u.speaker);
        let on = s.current == Some(i);
        let start = u.start;
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
                .on_secondary_press(move |s: &mut App, _, _| s.start_correction(Target::Utterance(i)))
                .child(
                    col().w(116.0).gap(2.0)
                        .child(text(name).font(k.ty.small_medium).color(if sure { color } else { color.alpha(0.7) }).truncate())
                        .child(text(ui::ts(u.start)).font(tnum(k.ty.text_xsmall)).color(k.c.text_faint)),
                )
                .child(if s.correction.as_ref().is_some_and(|c| c.target == Target::Utterance(i)) {
                    check::editor(s, k, &format!("fix{i}")).grow(1.0).min_w(0.0)
                } else {
                    utterance_text(m, i, k).grow(1.0).min_w(0.0)
                }),
        );
    }
    // Its own scroll area, to follow playback without scrolling the video away.
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

fn info(s: &App, m: &Meeting, k: &Kit) -> El {
    let done = |e: Stage| m.completed_stage.is_some_and(|f| f >= e);
    let line = |label: &str, value: String| {
        row().w_full().gap(8.0).center_y()
            .child(text(label.to_owned()).font(k.ty.small_medium).color(k.c.text_base).w(116.0))
            .child(text(value).font(k.ty.small).color(k.c.text_muted).wrap().grow(1.0).min_w(0.0))
    };
    let mut meeting = card(k).w(0.0).grow(1.0).min_w(320.0).child(heading(k, "Réunion"))
        .child(line("Date", format!("{} à {}", m.start.long_day(), m.start.short_time())))
        .child(line("Durée", ui::dur(m.duration)));
    // The agent's sessions, to pick up its conversation with `claude --resume` from the meetings
    // folder, where the agent ran.
    let sessions = [("Relecture", &m.sessions.review), ("Bilan", &m.sessions.summary)];
    if sessions.iter().all(|(_, id)| id.is_none()) {
        meeting = meeting.child(line("Claude", "Aucune session pour l'instant".into()));
    }
    for (i, (label, id)) in sessions.into_iter().enumerate() {
        let Some(id) = id.clone() else { continue };
        let command = match &s.folder {
            Some(dir) => format!("cd '{}' && claude --resume {id}", dir.display().to_string().replace('\'', "'\\''")),
            None => format!("claude --resume {id}"),
        };
        meeting = meeting.child(
            row().w_full().gap(8.0).center_y()
                .child(text(format!("Session {}", label.to_lowercase())).font(k.ty.small_medium).color(k.c.text_base).w(116.0).help("Session Claude"))
                .child(text(id).font(tnum(k.ty.small)).color(k.c.text_muted).selectable(true).truncate_middle().grow(1.0).min_w(0.0))
                .child(Button::new("Copier").variant(ButtonVariant::GhostFaint).control_size(ControlSize::Small)
                    .help("Copie la commande du Terminal qui reprend cette conversation avec Claude")
                    .on_press_tx(move |s: &mut App, tx| {
                        tx.copy(command.clone());
                        s.toast = Some(crate::Toast { text: "Commande de reprise de la session copiée.".into(), meeting: None, until: s.clock + 4.0, error: false });
                    })
                    .build(k)
                    .key(format!("session{i}"))),
        );
    }
    let mut processing = card(k).w(0.0).grow(1.0).min_w(320.0).child(heading(k, "Traitement"));
    for (ok, label, detail) in [
        (m.files.contains(&Role::Recording), "Enregistrement", ui::dur(m.duration)),
        (done(Stage::Transcription), "Transcription", format!("{} répliques · {} locuteurs", m.utterances.len(), m.active_speakers().count())),
        (done(Stage::Review), "Relecture IA", format!("{} corrections · {} à vérifier", m.corrections, m.pending_passages())),
        (done(Stage::Summary), "Bilan IA", format!("{} sujets · {} décisions", m.topics.len(), m.decisions.len())),
    ] {
        processing = processing.child(ui::check_row(k, ok, label, &detail));
    }
    let folder = format!("{}/", s.core.meeting_folder(m.id).map(|d| settings::display(&d)).unwrap_or_default());
    let mut files = card(k).w(0.0).grow(1.0).min_w(320.0).child(heading(k, "Fichiers dans le dossier de réunions"))
        .child(row().gap(6.0).center_y().child(glyph(&ui::FOLDER, 13.0, k.c.icon_muted)).child(text(folder).font(k.ty.small).color(k.c.text_muted).truncate_middle().min_w(0.0)));
    for role in Role::ALL {
        let present = m.files.contains(&role);
        let name = format!("{} {} - {}", if present { "·" } else { "–" }, m.folder_name, role.suffix());
        files = files.child(text(name).font(k.ty.small).color(if present { k.c.text_muted } else { k.c.text_faint.alpha(0.5) }).truncate_middle().min_w(0.0).pl(20.0));
    }
    let mut marks = card(k).w(0.0).grow(1.0).min_w(320.0).child(heading(k, "Repères"));
    if m.markers.is_empty() {
        marks = marks.child(muted(k, "Aucun repère posé pendant l'enregistrement."));
    }
    for (i, mk) in m.markers.iter().enumerate() {
        marks = marks.child(row().gap(8.0).center_y().child(ts_chip(k, mk.instant, format!("mk-ts{i}"))).child(text(mk.label.clone()).font(k.ty.ui).color(k.c.text_base)));
    }
    row().w_full().gap(12.0).flex_wrap().items_start().child(meeting).child(processing).child(files).child(marks)
}

fn pending(s: &App, m: &Meeting, k: &Kit) -> El {
    let id = m.id;
    // An imported transcription: processing resumes at the review (#5).
    if m.status == Status::ToProcess && m.completed_stage == Some(Stage::Transcription) {
        return card(k)
            .gap(14.0)
            .child(row().gap(10.0).center_y().child(glyph(&ui::WAVE, 18.0, k.c.text_accent)).child(heading(k, "Transcription importée, pas encore relue")))
            .child(muted(k, "La relecture et le bilan partent vers Claude. La transcription est reprise telle quelle."))
            .child(row().gap(8.0)
                .child(Button::new("Relire et faire le bilan").icon(&ui::SPARK).variant(ButtonVariant::Submit)
                    .on_press(move |s: &mut App| s.process(id)).build(k).key("process")));
    }
    // A recording cut by a crash or a write failure is processed like any other: what was
    // captured is kept.
    let interrupted = m.status == Status::RecordingInterrupted;
    if m.status == Status::ToProcess || interrupted {
        let people = m.speaker_count;
        let stepper = row().gap(6.0).center_y()
            .child(Button::new("−").variant(ButtonVariant::Outline).control_size(ControlSize::Small)
                .on_press(move |s: &mut App| s.command(Command::SpeakerCount { meeting: id, number: match people { Some(n) if n > 2 => Some(n - 1), _ => None } })).build(k).key("ppl-minus"))
            .child(text(people.map_or("Détecté".to_string(), |n| format!("{n} locuteurs"))).font(k.ty.ui_medium).color(k.c.text_base).w(100.0).align(rune::Align::Center))
            .child(Button::new("+").variant(ButtonVariant::Outline).control_size(ControlSize::Small)
                .on_press(move |s: &mut App| s.command(Command::SpeakerCount { meeting: id, number: Some(people.map_or(2, |n| (n + 1).min(12))) })).build(k).key("ppl-plus"));
        let mut panel = card(k)
            .gap(14.0)
            .child(row().gap(10.0).center_y().child(glyph(&ui::WAVE, 18.0, k.c.text_accent)).child(heading(k, if interrupted { "Enregistrement interrompu, pas encore traité" } else { "Enregistrée, pas encore traitée" })));
        if interrupted {
            panel = panel.child(muted(k, "L'enregistrement s'est arrêté avant la fin. Ce qui a été capturé est gardé et peut être traité."));
        }
        return panel
            .child(muted(k, "La transcription et les locuteurs tournent sur ton Mac. Seuls la relecture et le bilan partent vers Claude. Compte environ 1 min 30 de transcription pour 10 min de réunion."))
            .child(row().gap(12.0).center_y().child(text("Nombre de locuteurs").font(k.ty.small_medium).color(k.c.text_muted).w(130.0)).child(stepper))
            .child(muted(k, "Indiquer le bon nombre de locuteurs améliore beaucoup leur séparation. Laisse « Détecté » si tu ne sais pas."))
            .child(row().gap(8.0)
                .child(Button::new("Transcrire et faire le bilan").icon(&ui::SPARK).variant(ButtonVariant::Submit)
                    .on_press(move |s: &mut App| s.process(id)).build(k).key("process")));
    }
    let steps = Stage::ALL;
    let details = [
        "Whisper large-v3 et pyannote, sur le Mac",
        "Claude corrige, réécoute les passages douteux",
        "Claude remplit le template du bilan, en Markdown",
    ];
    let idx = match &m.status {
        Status::Running { stage, .. } => steps.iter().position(|x| x == stage),
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
            col_ = col_.child(Progress::new(Some(m.status.progress() as f32)).build(k));
        }
        list = list.child(row().key(format!("step{i}")).w_full().gap(12.0).items_start().child(row().w(18.0).h(18.0).center().child(icon)).child(col_));
    }
    let eta = ui::eta(s, id);
    let (status, action) = match &m.status {
        Status::Pending(_) => match s.core.rank(id).unwrap_or(0) {
            0 | 1 => (format!("En attente : une autre réunion est en cours de traitement. Environ {eta} avant la fin."), None),
            n => (format!("En attente : {n} réunions passent avant celle-ci. Environ {eta} avant la fin."), None),
        },
        Status::Failed { stage, message } => (format!("{} : {message}", ui::step(*stage)), Some("Réessayer cette étape")),
        _ if s.core.time_remaining(id).unwrap_or(0.0) <= 60.0 => ("Moins d'une minute restante. Une notification arrive à la fin.".to_string(), None),
        _ => (format!("Environ {eta} restantes. Une notification arrive à la fin."), None),
    };
    let button = match action {
        Some(label) => Button::new(label).variant(ButtonVariant::Outline).control_size(ControlSize::Small).on_press(move |s: &mut App| s.process(id)).build(k).key("retry"),
        None => Button::new("Arrêter").variant(ButtonVariant::GhostMuted).control_size(ControlSize::Small).on_press(move |s: &mut App| s.stop_processing(id)).build(k).key("stop-proc"),
    };
    card(k)
        .gap(16.0)
        .child(row().gap(10.0).center_y().child(heading(k, if matches!(m.status, Status::Failed { .. }) { "Traitement en échec" } else { "Traitement en cours" })).child(el().grow(1.0)).child(button))
        .child(muted(k, status))
        .child(list)
}
