//! Barre latérale : bouton d'enregistrement, navigation, réunions par période, menu du clic droit.

use rune::{El, col, row, text};
use rune_kit::shell::{RowStatus, Sidebar, SidebarRow, SidebarSection};
use rune_kit::{Kit, MenuItem, context_menu};

use crate::data::{Meeting, Stage};
use crate::ui::{self, glyph, red, tnum};
use crate::{App, Page, Tab};

fn record_button(s: &App, k: &Kit) -> El {
    let r = red();
    let (label, icon) = match &s.rec {
        Some(rec) => (format!("Arrêter · {}", ui::ts(rec.elapsed)), &ui::STOP),
        None => ("Enregistrer une réunion".to_string(), &ui::RECORD),
    };
    let recording = s.rec.is_some();
    let pulse = s.rec.as_ref().map_or(1.0, |r| 0.55 + 0.45 * (r.phase * 3.2).cos().abs() as f32);
    row()
        .key("record")
        .w_full()
        .h(38.0)
        .px(12.0)
        .gap(10.0)
        .center_y()
        .radius(10.0)
        .fill(if recording { r.alpha(0.16) } else { k.c.layer_02 })
        .border(1.0, if recording { r.alpha(0.45) } else { k.c.border_muted })
        .hover(k.c.hover)
        .press(k.c.pressed)
        .cursor(rune::Cursor::Pointer)
        .role("button")
        .label(label.clone())
        .child(glyph(icon, 14.0, r).opacity(pulse))
        .child(text(label).font(tnum(k.ty.ui_medium)).color(k.c.text_base).grow(1.0).min_w(0.0).truncate())
        .child(text("⇧⌘R").font(k.ty.small).color(k.c.text_faint))
        .on_press(|s: &mut App| {
            if s.rec.is_some() {
                if s.page == Page::Recording { s.stop_recording() } else { s.page = Page::Recording }
            } else {
                s.start_recording()
            }
        })
}

fn meeting_row(s: &App, m: &Meeting, k: &Kit, layer: &mut Vec<El>) -> El {
    let id = m.id;
    let subtitle = match m.stage {
        Stage::Done if m.pending_doubts() > 0 => format!("{} · {} · {} à vérifier", m.start, ui::dur(m.duration), m.pending_doubts()),
        Stage::Done => format!("{} · {}", m.start, ui::dur(m.duration)),
        Stage::Recorded => format!("{} · {} · à traiter", m.start, ui::dur(m.duration)),
        Stage::Queued => format!("{} · en attente", m.start),
        st => format!("{} · {} {:.0} %", m.start, st.label(), m.progress * 100.0),
    };
    let title = if m.stage == Stage::Done { m.title.clone() } else { format!("Réunion du {}", m.date_label) };
    let mut r = SidebarRow::new(title)
        .height(46.0)
        .subtitle(subtitle)
        .selected(s.page == Page::Meeting(id) || s.menu.is_some_and(|(mid, _)| mid == id))
        .on_select(move |s: &mut App, _| s.open(id));
    if m.stage.processing() || m.stage == Stage::Queued {
        r = r.status(RowStatus::Busy);
    } else if m.stage == Stage::Recorded {
        r = r.status(RowStatus::Attention);
    } else if m.unread {
        r = r.status(RowStatus::Unread);
    }
    let el = r.build(k).key(format!("m{id}")).on_secondary_press(move |s: &mut App, p, _| s.menu = Some((id, p)));
    let point = s.menu.filter(|(mid, _)| *mid == id).map(|(_, p)| p);
    if point.is_none() {
        return el;
    }
    let done = m.stage == Stage::Done;
    let mut menu = context_menu(format!("menu{id}"), el, point)
        .item(MenuItem::new("Ouvrir").on_select(move |s: &mut App| {
            s.menu = None;
            s.open(id);
        }));
    if done {
        menu = menu
            .item(MenuItem::new("Lire la transcription").on_select(move |s: &mut App| {
                s.menu = None;
                s.open(id);
                s.tab = Tab::Transcript;
            }))
            .item(MenuItem::new("Refaire le bilan").icon(&ui::SPARK).on_select(move |s: &mut App| {
                s.menu = None;
                s.redo_bilan(id);
            }));
    } else if m.stage == Stage::Recorded {
        menu = menu.item(MenuItem::new("Transcrire et faire le bilan").icon(&ui::SPARK).on_select(move |s: &mut App| {
            s.menu = None;
            s.process(id);
        }));
    }
    menu.separator()
        .item(MenuItem::new("Afficher dans le Finder").icon(&ui::FOLDER).on_select(|s: &mut App| s.menu = None))
        .item(MenuItem::new("Copier le bilan en Markdown").disabled(!done).on_select(|s: &mut App| s.menu = None))
        .separator()
        .item(MenuItem::new("Supprimer…").danger(true).shortcut("⌘⌫").on_select(move |s: &mut App| {
            s.menu = None;
            s.confirm_delete = Some(id);
        }))
        .min_width(250.0)
        .on_close(|s: &mut App| s.menu = None)
        .build(k, layer)
}

pub fn view(s: &App, k: &Kit, layer: &mut Vec<El>) -> El {
    let nav = |label: &str, icon: &'static rune::Icon, page: Page| {
        SidebarRow::new(label.to_owned()).icon(icon).height(32.0).selected(s.page == page).on_select(move |s: &mut App, _| s.page = page)
    };
    let mut sb = Sidebar::new()
        .label("Réunions")
        .inset(0.0)
        .header_gap(4.0)
        .header_padding(0.0, 14.0)
        .row_gap(2.0)
        .header(col().w_full().pb(8.0).child(record_button(s, k)))
        .header(nav("Vue d'ensemble", &ui::HOME, Page::Overview).build(k).key("nav-home"))
        .header(
            SidebarRow::new("Rechercher")
                .icon(&ui::SEARCH)
                .detail("⌘K")
                .height(32.0)
                .selected(s.search.is_some())
                .on_select(|s: &mut App, _| s.open_search())
                .build(k)
                .key("nav-search"),
        )
        .footer(nav("Réglages", &ui::SLIDERS, Page::Settings).build(k).key("nav-settings"));
    let groups: [(&str, fn(u32) -> bool); 4] = [
        ("Aujourd'hui", |a| a == 0),
        ("Hier", |a| a == 1),
        ("Cette semaine", |a| (2..=5).contains(&a)),
        ("Semaine dernière", |a| a > 5),
    ];
    for (gi, (name, pred)) in groups.iter().enumerate() {
        let rows: Vec<El> = s.meetings.iter().filter(|m| pred(m.ago)).map(|m| meeting_row(s, m, k, layer)).collect();
        if rows.is_empty() {
            continue;
        }
        sb = sb.section(
            SidebarSection::new(*name)
                .key(format!("sec{gi}"))
                .count(rows.len())
                .collapsed(s.collapsed[gi])
                .on_toggle(move |s: &mut App, _| s.collapsed[gi] ^= true)
                .rows(rows),
        );
    }
    sb.build(k)
}
