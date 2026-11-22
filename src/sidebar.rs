//! Barre latérale : bouton d'enregistrement, navigation, réunions par période, menu du clic droit.

use rune::{El, col, row, text};
use rune_kit::shell::{RowStatus, Sidebar, SidebarRow, SidebarSection};
use rune_kit::{Kit, MenuItem, context_menu};

use greffier_core::{Stage, Meeting, Status};

use crate::ui::{self, glyph, red, tnum};
use crate::{App, Page, Tab, redo};

fn record_button(s: &App, k: &Kit) -> El {
    let r = red();
    let (label, icon) = match s.core.recording() {
        Some(rec) => (format!("Arrêter · {}", ui::ts(rec.duration)), &ui::STOP),
        None => ("Enregistrer une réunion".to_string(), &ui::RECORD),
    };
    let recorded = s.recorded();
    let pulse = if recorded { 0.55 + 0.45 * (s.rec_phase * 3.2).cos().abs() as f32 } else { 1.0 };
    row()
        .key("record")
        .w_full()
        .h(38.0)
        .px(12.0)
        .gap(10.0)
        .center_y()
        .radius(10.0)
        .fill(if recorded { r.alpha(0.16) } else { k.c.layer_02 })
        .border(1.0, if recorded { r.alpha(0.45) } else { k.c.border_muted })
        .hover(k.c.hover)
        .press(k.c.pressed)
        .cursor(rune::Cursor::Pointer)
        .role("button")
        .label(label.clone())
        .child(glyph(icon, 14.0, r).opacity(pulse))
        .child(text(label).font(tnum(k.ty.ui_medium)).color(k.c.text_base).grow(1.0).min_w(0.0).truncate())
        .child(text(crate::pages::shortcut_caps(s).concat()).font(k.ty.small).color(k.c.text_faint))
        .on_press(|s: &mut App| {
            if s.recorded() {
                if s.page == Page::Recording { s.stop_recording() } else { s.page = Page::Recording }
            } else {
                s.start_recording()
            }
        })
}

fn meeting_row(s: &App, m: &Meeting, k: &Kit, layer: &mut Vec<El>) -> El {
    let id = m.id;
    let start = m.start.short_time();
    let subtitle = match &m.status {
        Status::Done if m.pending_passages() > 0 => format!("{start} · {} · {} à vérifier", ui::dur(m.duration), m.pending_passages()),
        Status::Done => format!("{start} · {}", ui::dur(m.duration)),
        Status::ToProcess => format!("{start} · {} · à traiter", ui::dur(m.duration)),
        _ => format!("{start} · {}", ui::status_line(s, m)),
    };
    let title = crate::meeting::display_title(m, false);
    let mut r = SidebarRow::new(title)
        .height(46.0)
        .subtitle(subtitle)
        .selected(s.page == Page::Meeting(id) || s.menu.is_some_and(|(mid, _)| mid == id))
        .on_select(move |s: &mut App, _| s.open(id));
    if m.status.in_queue() {
        r = r.status(RowStatus::Busy);
    } else if matches!(m.status, Status::ToProcess | Status::Failed { .. } | Status::RecordingInterrupted) {
        r = r.status(RowStatus::Attention);
    } else if m.is_new {
        r = r.status(RowStatus::Unread);
    }
    let el = r.build(k).key(format!("m{id}")).on_secondary_press(move |s: &mut App, p, _| s.menu = Some((id, p)));
    let point = s.menu.filter(|(mid, _)| *mid == id).map(|(_, p)| p);
    if point.is_none() {
        return el;
    }
    let mut menu = context_menu(format!("menu{id}"), el, point)
        .item(MenuItem::new("Ouvrir").on_select(move |s: &mut App| {
            s.menu = None;
            s.open(id);
        }));
    if !m.utterances.is_empty() {
        menu = menu.item(MenuItem::new("Lire la transcription").on_select(move |s: &mut App| {
            s.menu = None;
            s.open(id);
            s.tab = Tab::Transcript;
        }));
    }
    if m.status == Status::ToProcess {
        menu = menu.item(MenuItem::new("Transcrire et faire le bilan").icon(&ui::SPARK).on_select(move |s: &mut App| {
            s.menu = None;
            s.process(id);
        }));
    }
    if redo::can_redo(m, Stage::Summary) {
        menu = menu.item(MenuItem::new("Refaire le bilan").icon(&ui::SPARK).on_select(move |s: &mut App| s.redo(id, Stage::Summary)));
    }
    if redo::can_redo(m, Stage::Review) {
        menu = menu.item(MenuItem::new("Refaire la relecture").on_select(move |s: &mut App| s.redo(id, Stage::Review)));
    }
    if redo::can_redo(m, Stage::Transcription) {
        menu = menu.item(MenuItem::new("Refaire la transcription…").on_select(move |s: &mut App| s.start_redo_transcription(id)));
    }
    let busy = matches!(m.status, Status::Running { .. });
    menu.item(MenuItem::new("Changer le titre…").disabled(busy).on_select(move |s: &mut App| s.start_rename(id)))
        .separator()
        .item(MenuItem::new("Afficher dans le Finder").icon(&ui::FOLDER).on_select(move |s: &mut App| {
            s.menu = None;
            s.reveal(id);
        }))
        .item(MenuItem::new("Copier le bilan en Markdown").disabled(m.summary.trim().is_empty()).on_select_tx(move |s: &mut App, tx| {
            s.menu = None;
            s.copy_summary(id, tx);
        }))
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
    // Les semaines commencent le lundi : « Cette semaine » s'arrête au lundi d'aujourd'hui.
    let today = s.core.today();
    let monday = today.weekday() as i64;
    let groups: [(&str, Box<dyn Fn(i64) -> bool>); 5] = [
        ("Aujourd'hui", Box::new(|a| a <= 0)),
        ("Hier", Box::new(|a| a == 1)),
        ("Cette semaine", Box::new(move |a| a >= 2 && a <= monday)),
        ("Semaine dernière", Box::new(move |a| a >= 2 && a > monday && a <= monday + 7)),
        ("Plus tôt", Box::new(move |a| a >= 2 && a > monday + 7)),
    ];
    for (gi, (name, pred)) in groups.iter().enumerate() {
        let rows: Vec<El> = s.core.meetings().iter().filter(|m| pred(m.start.days_until(today))).map(|m| meeting_row(s, m, k, layer)).collect();
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
