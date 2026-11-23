//! The « À vérifier » tab and manual corrections: step through the passages to check with the
//! keyboard, listen to their excerpt, settle them; in the Transcript tab, correct an utterance's
//! text. Everything goes through the core's `Resolve` and `CorrectUtterance` commands, which
//! rewrite both transcriptions.

use rune::{El, Key, KeyEvent, TextEdit, Tx, col, row, text};
use rune_kit::{Button, ButtonVariant, ControlSize, Kit, Progress, TextArea};

use greffier_core::{Command, Rejection, Meeting, PassageStatus, Resolution};

use crate::meeting::{speaker_name, utterance_text};
use crate::ui::{self, card, dot, eyebrow, glyph, muted, tnum, ts_chip};
use crate::{App, Page, Tab, Toast};

/// What is being corrected by hand.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Target {
    /// A passage to check (index into `Meeting::passages`): the correction settles it.
    Passage(usize),
    /// An utterance's text (index into `Meeting::utterances`).
    Utterance(usize),
}

pub struct Correction {
    pub target: Target,
    pub edit: TextEdit,
    /// Why the core refused the correction, shown under the field.
    pub error: Option<String>,
}

/// The passage selected with the keyboard: `s.doubt` if it is still pending, otherwise the next
/// pending one (or the first).
pub fn selected(s: &App, m: &Meeting) -> Option<usize> {
    let pending = |i: &usize| m.passages[*i].status == PassageStatus::Pending;
    (s.doubt..m.passages.len()).find(pending).or_else(|| (0..m.passages.len()).find(pending))
}

/// The correction in progress is visible: its tab is open on its meeting's page.
pub fn correction_shown(s: &App) -> bool {
    let Some(c) = &s.correction else { return false };
    let Some(m) = s.meeting() else { return false };
    match c.target {
        Target::Passage(i) => s.tab == Tab::Doubts && m.passages.get(i).is_some_and(|p| p.status == PassageStatus::Pending),
        Target::Utterance(i) => s.tab == Tab::Transcript && i < m.utterances.len(),
    }
}

impl App {
    fn meeting_id(&self) -> Option<greffier_core::MeetingId> {
        match self.page {
            Page::Meeting(id) => Some(id),
            _ => None,
        }
    }

    /// Sends a correction command. A refusal shows under the open correction field, otherwise in
    /// the toast.
    fn correct(&mut self, c: Command) -> bool {
        let Err(rejection) = self.try_command(c) else { return true };
        let message = match rejection {
            Rejection::Impossible(m) => m,
            r => r.to_string(),
        };
        match &mut self.correction {
            Some(c) => c.error = Some(message),
            None => self.toast = Some(Toast { text: format!("Impossible : {message}"), meeting: None, until: self.clock + 6.0, error: true }),
        }
        false
    }

    pub fn resolve_doubt(&mut self, passage: usize, resolution: Resolution) {
        let Some(meeting) = self.meeting_id() else { return };
        let reopen = resolution == Resolution::Reopen;
        if self.correct(Command::Resolve { meeting, passage, resolution }) {
            self.correction = None;
            // Stay on a reopened passage; otherwise the selection moves to the next pending one.
            self.doubt = if reopen { passage } else { passage + 1 };
        }
    }

    /// Plays the passage's excerpt (the relistened one) and stops at its end.
    pub fn listen_doubt(&mut self, passage: usize) {
        let Some(window) = self.meeting().and_then(|m| m.excerpt_to_listen(passage)) else { return };
        self.doubt = passage;
        self.seek_and_play(window.0);
        self.listen_until = Some(window.1);
    }

    pub fn start_correction(&mut self, target: Target) {
        let Some(m) = self.meeting() else { return };
        let (text, all) = match target {
            Target::Passage(i) => {
                let Some(p) = m.passages.get(i) else { return };
                (p.hypothesis.clone().unwrap_or_else(|| p.excerpt.clone()), true)
            }
            Target::Utterance(i) => {
                let Some(x) = m.utterances.get(i) else { return };
                (x.text.clone(), false)
            }
        };
        let mut edit = TextEdit::new(text.clone());
        // A passage is retyped in full; an utterance is corrected from the end.
        edit.set_selection(if all { 0 } else { text.len() }, text.len());
        edit.focused = true;
        if let Target::Passage(i) = target {
            self.doubt = i;
        }
        self.correction = Some(Correction { target, edit, error: None });
    }

    pub fn submit_correction(&mut self) {
        let (Some(meeting), Some(c)) = (self.meeting_id(), &self.correction) else { return };
        let text = c.edit.text.trim().to_string();
        match c.target {
            Target::Passage(passage) => self.resolve_doubt(passage, Resolution::Correct(text)),
            Target::Utterance(utterance) => {
                if self.correct(Command::CorrectUtterance { meeting, utterance, text }) {
                    self.correction = None;
                }
            }
        }
    }

    pub fn cancel_correction(&mut self) {
        self.correction = None;
    }

    /// Moves to the next (`+1`) or previous (`-1`) pending passage.
    fn step_doubt(&mut self, delta: isize, tx: &mut Tx) {
        let Some(m) = self.meeting() else { return };
        let Some(cur) = selected(self, m) else { return };
        let pending: Vec<usize> = (0..m.passages.len()).filter(|&i| m.passages[i].status == PassageStatus::Pending).collect();
        let at = pending.iter().position(|&i| i == cur).unwrap_or(0) as isize;
        let next = pending[(at + delta).clamp(0, pending.len() as isize - 1) as usize];
        self.doubt = next;
        tx.scroll_center_in("meeting", format!("dc{next}"));
    }
}

/// The « À vérifier » tab's keys: ↑ ↓ (or K J) from one passage to another, E listen,
/// A accept the hypothesis, G keep the original, C correct.
pub fn on_key(s: &mut App, ev: &KeyEvent, tx: &mut Tx) -> bool {
    let Some(cur) = s.meeting().and_then(|m| selected(s, m)) else { return false };
    let letter = |c: &str| matches!(&ev.key, Key::Text(t) if t.eq_ignore_ascii_case(c));
    match &ev.key {
        Key::Down => s.step_doubt(1, tx),
        Key::Up => s.step_doubt(-1, tx),
        _ if letter("j") => s.step_doubt(1, tx),
        _ if letter("k") => s.step_doubt(-1, tx),
        _ if letter("e") => s.listen_doubt(cur),
        _ if letter("a") => {
            if s.meeting().is_some_and(|m| m.passages[cur].hypothesis.is_some()) {
                s.resolve_doubt(cur, Resolution::AcceptHypothesis);
            }
        }
        _ if letter("g") => s.resolve_doubt(cur, Resolution::KeepOriginal),
        _ if letter("c") => s.start_correction(Target::Passage(cur)),
        _ => return false,
    }
    true
}

/// The correction field: Return saves, Escape cancels.
pub fn editor(s: &App, k: &Kit, key: &str) -> El {
    let Some(c) = &s.correction else { return rune::el() };
    let area = TextArea::new(&c.edit)
        .width(None)
        .auto_grow(1, 6)
        .placeholder("Texte corrigé")
        .on_edit(|s: &mut App| &mut s.correction.as_mut().expect("correction open").edit)
        .on_key(|s: &mut App, ev, _| match ev.key {
            Key::Enter if !ev.mods.shift => {
                s.submit_correction();
                true
            }
            Key::Escape => {
                s.cancel_correction();
                true
            }
            _ => false,
        })
        .build(k)
        .key(format!("{key}-input"));
    let error = match &c.error {
        Some(e) => text(e.clone()).font(k.ty.small).color(k.c.danger.fg).wrap(),
        None => rune::el(),
    };
    col()
        .key(key.to_owned())
        .w_full()
        .gap(8.0)
        .child(area.label("Correction"))
        .child(error)
        .child(
            row().gap(8.0).center_y()
                .child(Button::new("Enregistrer").variant(ButtonVariant::Submit).control_size(ControlSize::Small).on_press(|s: &mut App| s.submit_correction()).build(k).key(format!("{key}-ok")))
                .child(Button::new("Annuler").variant(ButtonVariant::Ghost).control_size(ControlSize::Small).on_press(|s: &mut App| s.cancel_correction()).build(k).key(format!("{key}-cancel")))
                .child(text("↩ enregistrer · esc annuler").font(k.ty.small).color(k.c.text_faint)),
        )
}

/// The « À vérifier » tab: the remaining count, then each passage with its utterance, the agent's
/// hypothesis and the relisten.
pub fn tab(s: &App, m: &Meeting, k: &Kit) -> El {
    let total = m.passages.len();
    let left = m.pending_passages();
    let header = match left {
        0 => format!("Aucun passage à vérifier sur {total}"),
        1 => format!("1 passage à vérifier sur {total}"),
        n => format!("{n} passages à vérifier sur {total}"),
    };
    let mut c = col().w_full().gap(12.0);
    c = c.child(
        col().w_full().gap(8.0)
            .child(text(header).font(k.ty.body_medium).color(k.c.text_base))
            .child(muted(k, "La relecture IA a corrigé seule ce qui était sûr. Restent ces passages : écoute, puis remplace par l'hypothèse de l'IA, garde le texte d'origine ou tape ta correction."))
            .child(Progress::new(Some(if total == 0 { 1.0 } else { (total - left) as f32 / total as f32 })).build(k)),
    );
    if total == 0 {
        return c.child(muted(k, "Rien à vérifier."));
    }
    if left > 0 {
        c = c.child(text("↑ ↓ passage précédent ou suivant · E écouter · A accepter l'hypothèse · G garder l'original · C corriger").font(k.ty.small).color(k.c.text_faint).wrap());
    }
    let current = selected(s, m);
    for (di, d) in m.passages.iter().enumerate() {
        let u = &m.utterances[d.utterance];
        let (name, color, _) = speaker_name(m, u.speaker);
        let start = u.start;
        if d.status != PassageStatus::Pending {
            let (label, fg) = match &d.status {
                PassageStatus::HypothesisAccepted => (format!("« {} » → « {} »", d.excerpt, d.hypothesis.clone().unwrap_or_default()), k.c.success.icon),
                PassageStatus::CorrectedManually(t) => (format!("« {} » → « {t} »", d.excerpt), k.c.success.icon),
                _ => (format!("« {} » gardé tel quel", d.excerpt), k.c.text_muted),
            };
            c = c.child(
                row().key(format!("dz{di}")).w_full().gap(10.0).center_y().px(12.0).py(8.0).radius(k.radius.md).fill(k.c.layer_01)
                    .child(glyph(&ui::CHECK, 13.0, fg))
                    .child(text(ui::ts(start)).font(tnum(k.ty.small)).color(k.c.text_faint))
                    .child(text(label).font(k.ty.small).color(k.c.text_muted).truncate().grow(1.0).min_w(0.0))
                    .child(Button::new("Annuler").variant(ButtonVariant::GhostFaint).control_size(ControlSize::Small)
                        .on_press(move |s: &mut App| s.resolve_doubt(di, Resolution::Reopen)).build(k).key(format!("dz{di}-reopen"))),
            );
            continue;
        }
        let editing = s.correction.as_ref().is_some_and(|c| c.target == Target::Passage(di));
        let actions = if editing {
            editor(s, k, &format!("dfix{di}"))
        } else {
            let accept = match &d.hypothesis {
                Some(h) => Button::new(format!("Remplacer par « {h} »")).variant(ButtonVariant::Submit).control_size(ControlSize::Small)
                    .on_press(move |s: &mut App| s.resolve_doubt(di, Resolution::AcceptHypothesis)).build(k).key(format!("dc{di}-accept")),
                None => Button::new("Pas d'hypothèse").variant(ButtonVariant::Outline).control_size(ControlSize::Small).disabled(true).build(k),
            };
            row().gap(8.0).flex_wrap()
                .child(Button::new("Écouter").icon(&ui::PLAY).variant(ButtonVariant::Outline).control_size(ControlSize::Small)
                    .on_press(move |s: &mut App| s.listen_doubt(di)).build(k).key(format!("dc{di}-listen")))
                .child(accept)
                .child(Button::new("Garder l'original").variant(ButtonVariant::Ghost).control_size(ControlSize::Small)
                    .on_press(move |s: &mut App| s.resolve_doubt(di, Resolution::KeepOriginal)).build(k).key(format!("dc{di}-keep")))
                .child(Button::new("Corriger…").variant(ButtonVariant::Ghost).control_size(ControlSize::Small)
                    .on_press(move |s: &mut App| s.start_correction(Target::Passage(di))).build(k).key(format!("dc{di}-correct")))
        };
        let mut item = card(k).key(format!("dc{di}"));
        if current == Some(di) {
            item = item.border(1.5, k.c.text_accent);
        }
        c = c.child(
            item
                .child(row().gap(10.0).center_y()
                    .child(ts_chip(k, start, format!("dts{di}")))
                    .child(dot(color, 8.0))
                    .child(text(name).font(k.ty.small_medium).color(k.c.text_base)))
                .child(utterance_text(m, d.utterance, k))
                .child(
                    col().gap(4.0).padding(10.0).radius(k.radius.md).fill(k.c.layer_02)
                        .child(row().gap(6.0).center_y().child(glyph(&ui::SPARK, 11.0, k.c.text_accent)).child(eyebrow(k, "Réécoute du passage")))
                        .child(text(d.relisten.clone()).font(k.ty.small).color(k.c.text_muted).wrap()),
                )
                .child(actions),
        );
    }
    c
}
