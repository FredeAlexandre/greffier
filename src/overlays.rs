//! Les fenêtres par-dessus l'app : la recherche ⌘K et la confirmation de suppression.

use rune::{El, ScrollSpec, Span, col, el, rich, row, text};
use rune_kit::{Button, ButtonVariant, Dialog, Kit, TextInput};

use greffier_coeur::{IdReunion, Statut, bilan};

use crate::ui::{self, glyph, muted, tnum};
use crate::{App, Tab};

pub struct Hit {
    pub id: IdReunion,
    pub at: Option<f64>,
    pub kind: &'static str,
    pub meeting: String,
    pub who: Option<String>,
    pub snippet: String,
    pub range: Option<(usize, usize)>,
    pub transcript: bool,
}

/// Un extrait de `s` autour de `[a, b)`, coupé sur des limites de caractères.
fn excerpt(s: &str, a: usize, b: usize) -> (String, (usize, usize)) {
    let mut start = a.saturating_sub(50);
    while !s.is_char_boundary(start) {
        start -= 1;
    }
    let mut end = (b + 90).min(s.len());
    while !s.is_char_boundary(end) {
        end += 1;
    }
    let prefix = if start > 0 { "…" } else { "" };
    let suffix = if end < s.len() { "…" } else { "" };
    let out = format!("{prefix}{}{suffix}", &s[start..end]);
    let shift = prefix.len() + a - start;
    (out, (shift, shift + (b - a)))
}

fn find(hay: &str, q: &str) -> Option<(usize, usize)> {
    let low = hay.to_lowercase();
    if low.len() != hay.len() {
        return None;
    }
    low.find(q).map(|i| (i, i + q.len()))
}

pub fn hits(s: &App) -> Vec<Hit> {
    let q = s.search.as_ref().map(|e| e.text.trim().to_lowercase()).unwrap_or_default();
    let mut out = Vec::new();
    if q.is_empty() {
        for m in s.core.reunions().iter().take(6) {
            let title = if m.statut == Statut::Terminee { m.titre.clone() } else { format!("Réunion du {}", m.debut.jour_court()) };
            out.push(Hit { id: m.id, at: None, kind: "Réunion", meeting: format!("{} · {}", m.debut.jour_court(), m.debut.heure_courte()), who: None, snippet: title, range: None, transcript: false });
        }
        return out;
    }
    for m in s.core.reunions().iter().filter(|m| m.statut == Statut::Terminee) {
        let when = format!("{} · {}", m.debut.jour_court(), m.debut.heure_courte());
        let mut n = 0;
        if let Some(r) = find(&m.titre, &q) {
            out.push(Hit { id: m.id, at: None, kind: "Réunion", meeting: when.clone(), who: None, snippet: m.titre.clone(), range: Some(r), transcript: false });
        }
        // Une puce sans horodatage prend celui du sujet `###` qui la contient.
        let mut topic_at = None;
        for line in m.bilan.lines() {
            if line.starts_with("## ") {
                topic_at = None;
            }
            if let Some(t) = bilan::sujets(&format!("## Sujets\n{line}")).first() {
                topic_at = Some(t.debut);
            }
            let clean = line.trim_start_matches(['#', '-', ' ', '|']).replace("**", "");
            if let Some((a, b)) = find(&clean, &q) {
                let at = bilan::decisions(&format!("## Décisions\n{line}")).first().map(|d| d.instant).or(topic_at);
                let (snippet, range) = excerpt(&clean, a, b);
                out.push(Hit { id: m.id, at, kind: "Bilan", meeting: format!("{} · {when}", m.titre), who: None, snippet, range: Some(range), transcript: false });
                n += 1;
                if n >= 3 {
                    break;
                }
            }
        }
        let mut n = 0;
        for u in &m.repliques {
            if let Some((a, b)) = find(&u.texte, &q) {
                let (snippet, range) = excerpt(&u.texte, a, b);
                let who = m.locuteurs[m.locuteur_affiche(u.locuteur)].affiche();
                out.push(Hit { id: m.id, at: Some(u.debut), kind: "Transcription", meeting: format!("{} · {when}", m.titre), who: Some(who), snippet, range: Some(range), transcript: true });
                n += 1;
                if n >= 4 {
                    break;
                }
            }
        }
        if out.len() >= 40 {
            break;
        }
    }
    out
}

fn go(s: &mut App, h: &Hit) {
    s.open(h.id);
    if let Some(t) = h.at {
        s.seek(t);
    }
    if h.transcript {
        s.tab = Tab::Transcript;
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
    let Some(m) = s.core.reunion(id) else { return };
    let title = if m.statut == Statut::Terminee { m.titre.clone() } else { format!("Réunion du {}", m.debut.jour_court()) };
    let body = col()
        .w_full()
        .px(16.0)
        .pb(16.0)
        .gap(14.0)
        .child(row().gap(8.0).center_y().child(glyph(&ui::FOLDER, 13.0, k.c.icon_muted)).child(text(title).font(k.ty.ui_medium).color(k.c.text_base).truncate().min_w(0.0)))
        .child(muted(k, format!("La vidéo ({}), la transcription et le bilan iront à la corbeille du Mac.", ui::dur(m.duree))))
        .child(
            row().w_full().gap(8.0).justify_end()
                .child(Button::new("Annuler").variant(ButtonVariant::Ghost).on_press(|s: &mut App| s.confirm_delete = None).build(k).key("del-cancel"))
                .child(Button::new("Supprimer").variant(ButtonVariant::Danger).on_press(move |s: &mut App| s.delete(id)).build(k).key("del-ok")),
        );
    Dialog::new("delete", "Supprimer la réunion ?").open(true).width(440.0).fit().on_close(|s: &mut App| s.confirm_delete = None).child(body).build(k, layer);
}
