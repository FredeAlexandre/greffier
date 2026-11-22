//! La timeline : sujets, une piste par locuteur, décisions et repères, tête de lecture.
//! Glisser sur les pistes déplace la lecture ; cliquer un sujet y saute.

use rune::{Cx, El, Spring, col, el, row, text};
use rune_kit::Kit;

use greffier_core::Meeting;
use crate::ui::{self, dot, spk_color, tnum};
use crate::App;

const LABEL_W: f32 = 112.0;
const CHAPTER_H: f32 = 24.0;
const TRACK_H: f32 = 16.0;
const EVENTS_H: f32 = 16.0;
const GAP: f32 = 6.0;

pub fn view(s: &App, m: &Meeting, k: &Kit, cx: &mut Cx<App>) -> El {
    let w = cx.layout_size("tl-area").map_or(560.0, |v| v[0]).max(100.0);
    let d = m.duration.max(1.0);
    let x = move |t: f64| (t / d) as f32 * w;
    let label = |e: El| row().w(LABEL_W).h_full().pr(10.0).gap(6.0).center_y().child(e);
    let faint = k.c.text_faint;

    // Sujets
    let current_topic = m.topic_at(s.pos);
    let mut chapters = el().key("tl-area").h(CHAPTER_H).grow(1.0).min_w(0.0);
    for (i, tp) in m.topics.iter().enumerate() {
        let end = m.topics.get(i + 1).map_or(m.duration, |n| n.start);
        let (x0, x1) = (x(tp.start), x(end));
        let on = current_topic == Some(i);
        let start = tp.start;
        chapters = chapters.child(
            row()
                .key(format!("ch{i}"))
                .absolute(x0 + 1.0, 0.0)
                .size((x1 - x0 - 2.0).max(2.0), CHAPTER_H)
                .px(6.0)
                .center_y()
                .radius(5.0)
                .fill(if on { k.c.text_accent.alpha(0.18) } else { k.c.layer_02 })
                .hover(k.c.hover)
                .cursor(rune::Cursor::Pointer)
                .help(format!("{} · {}", ui::ts(tp.start), tp.title))
                .child(text(tp.title.clone()).font(k.ty.text_xsmall_medium).color(if on { k.c.text_accent } else { k.c.text_muted }).truncate().min_w(0.0))
                .on_press(move |s: &mut App| s.seek_and_play(start)),
        );
    }
    let chapters_row = row().w_full().h(CHAPTER_H).child(label(text("Sujets").font(k.ty.small).color(faint))).child(chapters);

    // Pistes des locuteurs
    let active: Vec<usize> = (0..m.speakers.len()).filter(|&i| m.speakers[i].merged_into.is_none()).collect();
    let mut tracks = col().key("tl-tracks").w_full().gap(GAP);
    for &si in &active {
        let spk = &m.speakers[si];
        let c = spk_color(spk.tint);
        let mut lane = el().h(TRACK_H).grow(1.0).min_w(0.0).radius(4.0).fill(k.c.hover);
        for (ui_, u) in m.utterances.iter().enumerate() {
            if m.displayed_speaker(u.speaker) != si {
                continue;
            }
            let on = s.current == Some(ui_);
            lane = lane.child(
                el().absolute(x(u.start), 3.0)
                    .size((x(u.end) - x(u.start)).max(1.5), TRACK_H - 6.0)
                    .radius(2.0)
                    .fill(if on { c } else { c.alpha(0.75) }),
            );
        }
        let name = text(spk.display_name()).font(k.ty.small_medium).color(if spk.name.is_some() { k.c.text_base } else { k.c.text_muted }).truncate().min_w(0.0);
        tracks = tracks.child(row().key(format!("lane{si}")).w_full().h(TRACK_H).child(label(dot(c, 8.0)).child(name)).child(lane));
    }

    // Décisions et repères
    let mut events = el().h(EVENTS_H).grow(1.0).min_w(0.0);
    for (i, dcs) in m.decisions.iter().enumerate() {
        let at = dcs.instant;
        events = events.child(
            el().key(format!("dec{i}"))
                .absolute(x(at) - 5.0, 3.0)
                .size(10.0, 10.0)
                .radius(2.0)
                .rotate(std::f32::consts::FRAC_PI_4)
                .fill(k.c.warning.icon)
                .cursor(rune::Cursor::Pointer)
                .help(format!("Décision · {} · {}", ui::ts(at), dcs.text))
                .on_press(move |s: &mut App| s.seek_and_play(at)),
        );
    }
    for (i, mk) in m.markers.iter().enumerate() {
        let at = mk.instant;
        events = events.child(
            col().key(format!("mk{i}"))
                .absolute(x(at) - 1.0, 0.0)
                .size(10.0, EVENTS_H)
                .cursor(rune::Cursor::Pointer)
                .help(format!("Repère · {} · {}", ui::ts(at), mk.label))
                .child(el().size(2.0, EVENTS_H).fill(k.c.text_accent))
                .on_press(move |s: &mut App| s.seek_and_play(at)),
        );
    }
    let legend = row()
        .gap(4.0)
        .center_y()
        .child(el().size(7.0, 7.0).radius(1.5).rotate(std::f32::consts::FRAC_PI_4).fill(k.c.warning.icon))
        .child(text("Décisions").font(k.ty.small).color(faint));
    tracks = tracks.child(row().w_full().h(EVENTS_H).child(label(legend)).child(events));

    // Graduations
    let step = if m.duration > 5400.0 { 1800.0 } else if m.duration > 1800.0 { 600.0 } else { 300.0 };
    let mut ruler = el().h(14.0).grow(1.0).min_w(0.0);
    let mut t = step;
    while t < m.duration - step * 0.3 {
        ruler = ruler.child(text(hm(t)).font(tnum(k.ty.text_xsmall)).color(faint).absolute(x(t) - 12.0, 0.0));
        t += step;
    }
    let ruler_row = row().w_full().h(14.0).child(el().w(LABEL_W)).child(ruler);

    // Tête de lecture, au-dessus des pistes
    let rows = active.len() as f32 + 1.0;
    let tracks_h = rows * TRACK_H + (rows - 1.0) * GAP;
    let dur_ = m.duration;
    let tracks = tracks
        .child(
            el().key("playhead")
                .absolute(LABEL_W + x(s.pos) - 1.0, -4.0)
                .size(2.0, tracks_h + 8.0)
                .radius(1.0)
                .fill(k.c.text_base)
                .motion(Spring::new(0.12, 0.0))
                .inert(),
        )
        .cursor(rune::Cursor::Pointer)
        .on_drag(move |s: &mut App, drag, _| {
            let f = ((drag.x - LABEL_W) / (drag.w - LABEL_W).max(1.0)).clamp(0.0, 1.0);
            s.seek(f64::from(f) * dur_);
        });

    col()
        .key("timeline")
        .w_full()
        .gap(8.0)
        .padding(14.0)
        .radius(k.radius.lg)
        .fill(k.c.layer_01)
        .border(1.0, k.c.border_muted)
        .child(chapters_row)
        .child(tracks)
        .child(ruler_row)
}

fn hm(t: f64) -> String {
    let m = (t / 60.0).round() as u64;
    format!("{}:{:02}", m / 60, m % 60)
}
