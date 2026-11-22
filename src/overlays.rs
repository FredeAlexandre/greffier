//! Les fenêtres par-dessus l'app : la recherche ⌘K et la confirmation de suppression.

use rune::{El, ScrollSpec, Span, col, el, rich, row, text};
use rune_kit::{Button, ButtonVariant, Dialog, Kit, TextInput};

use greffier_core::search::Location;
use greffier_core::{MeetingId, Meeting, Status};

use crate::ui::{self, glyph, muted, tnum};
use crate::{App, Tab, meeting};

pub struct Hit {
    pub id: MeetingId,
    pub at: Option<f64>,
    pub kind: &'static str,
    pub meeting: String,
    pub who: Option<String>,
    pub snippet: String,
    pub range: Option<(usize, usize)>,
    pub location: Option<Location>,
}

/// Les résultats de la recherche du cœur ; sans requête, les réunions récentes.
pub fn hits(s: &App) -> Vec<Hit> {
    let q = s.search.as_ref().map(|e| e.text.trim().to_string()).unwrap_or_default();
    let when = |m: &Meeting| format!("{} · {}", m.start.short_day(), m.start.short_time());
    if q.is_empty() {
        return s.core.meetings().iter().take(6).map(|m| Hit { id: m.id, at: None, kind: "Réunion", meeting: when(m), who: None, snippet: meeting::display_title(m, false), range: None, location: None }).collect();
    }
    s.core
        .search(&q)
        .into_iter()
        .filter_map(|r| {
            let m = s.core.meeting(r.meeting)?;
            let (kind, meeting) = match r.location {
                Location::Title => ("Réunion", when(m)),
                Location::Summary => ("Bilan", format!("{} · {}", meeting::display_title(m, false), when(m))),
                Location::Transcription => ("Transcription", format!("{} · {}", meeting::display_title(m, false), when(m))),
            };
            Some(Hit { id: r.meeting, at: r.instant, kind, meeting, who: r.speaker, snippet: r.excerpt, range: Some(r.highlight), location: Some(r.location) })
        })
        .collect()
}

/// Ouvre la réunion du résultat à l'instant visé : la réplique dans la transcription, le sujet
/// (la décision) d'une ligne du bilan dans l'onglet Bilan.
fn go(s: &mut App, h: &Hit) {
    s.open(h.id);
    if let Some(t) = h.at {
        s.seek(t);
    }
    match h.location {
        Some(Location::Transcription) => s.tab = Tab::Transcript,
        Some(Location::Summary) => s.tab = Tab::Summary,
        _ => {}
    }
    s.search = None;
}

fn hit_row(k: &Kit, i: usize, h: Hit) -> El {
    let mut snippet = match h.range {
        Some((a, b)) => rich(h.snippet.clone(), vec![Span { bg: Some(k.c.warning.icon.alpha(0.3)), bg_pad: 1.0, ..Span::new(a, b).bold() }]),
        None => text(h.snippet.clone()).wrap(),
    }
    .font(k.ty.ui)
    .color(k.c.text_base)
    .max_lines(2);
    if h.kind == "Réunion" {
        snippet = snippet.font(k.ty.ui_medium);
    }
    let mut meta = row().gap(6.0).center_y().child(text(h.kind).font(k.ty.text_xsmall_medium).color(k.c.text_accent));
    if let Some(w) = &h.who {
        meta = meta.child(text(format!("· {w}")).font(k.ty.text_xsmall_medium).color(k.c.text_muted));
    }
    meta = meta.child(text(format!("· {}", h.meeting)).font(k.ty.text_xsmall).color(k.c.text_faint).truncate().min_w(0.0));
    let at = h.at;
    row()
        .key(format!("hit{i}"))
        .w_full()
        .gap(12.0)
        .px(12.0)
        .py(9.0)
        .radius(8.0)
        .items_start()
        .hover(k.c.hover)
        .press(k.c.pressed)
        .cursor(rune::Cursor::Pointer)
        .on_press(move |s: &mut App| {
            if let Some(h) = hits(s).into_iter().nth(i) {
                go(s, &h);
            }
        })
        .child(col().grow(1.0).min_w(0.0).gap(3.0).child(meta).child(snippet))
        .child(match at {
            Some(t) => text(ui::ts(t)).font(tnum(k.ty.small)).color(k.c.text_faint),
            None => el(),
        })
}

pub fn search(s: &App, k: &Kit, layer: &mut Vec<El>) {
    let Some(edit) = &s.search else { return };
    let input = TextInput::new(edit)
        .placeholder("Chercher dans les sujets, les bilans et les transcriptions")
        .leading_icon(&ui::SEARCH)
        .large()
        .width(None)
        .clear_button()
        .on_edit(|s: &mut App| s.search.as_mut().expect("recherche ouverte"))
        .on_key(|s: &mut App, ev, _| {
            if ev.key == rune::Key::Enter {
                if let Some(h) = hits(s).into_iter().next() {
                    go(s, &h);
                }
                return true;
            }
            false
        })
        .build(k)
        .key("search-input");
    let results = hits(s);
    let empty = edit.text.trim().is_empty();
    let mut list = col().w_full().gap(2.0);
    if results.is_empty() {
        list = list.child(col().w_full().padding(20.0).items_center().child(muted(k, "Aucun résultat.")));
    }
    let count = results.len();
    for (i, h) in results.into_iter().enumerate() {
        list = list.child(hit_row(k, i, h));
    }
    let label = if empty { "Réunions récentes".to_string() } else { format!("{count} résultats") };
    let body = col()
        .w_full()
        .px(16.0)
        .pb(16.0)
        .gap(10.0)
        .child(input)
        .child(row().w_full().justify_between().child(text(label).font(k.ty.small_medium).color(k.c.text_muted)).child(text("↩ ouvrir · esc fermer").font(k.ty.small).color(k.c.text_faint)))
        .child(col().key("search-results").w_full().max_h(420.0).scroll(ScrollSpec { stick_bottom: false }).child(list));
    Dialog::new("search", "Rechercher").open(true).width(700.0).fit().on_close(|s: &mut App| s.search = None).child(body).build(k, layer);
}

pub fn confirm_delete(s: &App, k: &Kit, layer: &mut Vec<El>) {
    let Some(id) = s.confirm_delete else { return };
    let Some(m) = s.core.meeting(id) else { return };
    let title = if m.status == Status::Done { m.title.clone() } else { format!("Réunion du {}", m.start.short_day()) };
    let body = col()
        .w_full()
        .px(16.0)
        .pb(16.0)
        .gap(14.0)
        .child(row().gap(8.0).center_y().child(glyph(&ui::FOLDER, 13.0, k.c.icon_muted)).child(text(title).font(k.ty.ui_medium).color(k.c.text_base).truncate().min_w(0.0)))
        .child(muted(k, format!("La vidéo ({}), la transcription et le bilan iront à la corbeille du Mac.", ui::dur(m.duration))))
        .child(
            row().w_full().gap(8.0).justify_end()
                .child(Button::new("Annuler").variant(ButtonVariant::Ghost).on_press(|s: &mut App| s.confirm_delete = None).build(k).key("del-cancel"))
                .child(Button::new("Supprimer").variant(ButtonVariant::Danger).on_press(move |s: &mut App| s.delete(id)).build(k).key("del-ok")),
        );
    Dialog::new("delete", "Supprimer la réunion ?").open(true).width(440.0).fit().on_close(|s: &mut App| s.confirm_delete = None).child(body).build(k, layer);
}

/// À l'arrêt d'un enregistrement de moins d'une minute : sans doute un faux départ.
pub fn short_recording(s: &App, k: &Kit, layer: &mut Vec<El>) {
    let Some(id) = s.short_recording else { return };
    let Some(m) = s.core.meeting(id) else { return };
    let body = col()
        .w_full()
        .px(16.0)
        .pb(16.0)
        .gap(14.0)
        .child(muted(k, format!("L'enregistrement n'a duré que {}. Si c'était un faux départ, il peut partir à la corbeille du Mac.", ui::dur(m.duration))))
        .child(
            row().w_full().gap(8.0).justify_end()
                .child(Button::new("Garder").variant(ButtonVariant::Ghost).on_press(|s: &mut App| s.short_recording = None).build(k).key("short-keep"))
                .child(Button::new("Supprimer").variant(ButtonVariant::Danger).on_press(move |s: &mut App| {
                    s.short_recording = None;
                    s.delete(id);
                }).build(k).key("short-delete")),
        );
    Dialog::new("short-recording", "Supprimer cet enregistrement ?").open(true).width(440.0).fit().on_close(|s: &mut App| s.short_recording = None).child(body).build(k, layer);
}
