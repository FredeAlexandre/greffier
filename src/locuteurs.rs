//! L'onglet Locuteurs : chaque locuteur détecté avec le nom proposé par l'agent du bilan, sa
//! justification et son temps de parole. On confirme le nom proposé, on le corrige (champ libre
//! ou un nom du lexique), on fusionne deux locuteurs qui sont la même voix et on défait une
//! fusion. Le cœur écrit les noms et les fusions dans la fiche et régénère la transcription.

use rune::{El, TextEdit, col, el, row, text};
use rune_kit::{Button, ButtonVariant, ControlSize, Kit, TextInput};

use greffier_coeur::{Commande, Reunion};

use crate::ui::{self, dot, glyph, muted, spk_color};
use crate::App;

/// Ce qu'on est en train de faire sur un locuteur.
pub struct Picking {
    pub locuteur: usize,
    pub mode: Mode,
}

pub enum Mode {
    /// Corriger le nom : un champ libre et les noms connus.
    Nom { edit: Box<TextEdit>, error: Option<String> },
    /// Choisir l'autre locuteur, qui est la même voix.
    Fusion,
}

impl Picking {
    fn edit(&mut self) -> &mut TextEdit {
        match &mut self.mode {
            Mode::Nom { edit, .. } => edit,
            Mode::Fusion => unreachable!("le champ du nom n'existe qu'en correction"),
        }
    }
}

impl App {
    /// Ouvre (ou referme) la correction du nom, prérempli avec le nom actuel ou proposé.
    pub fn start_naming(&mut self, locuteur: usize) {
        if matches!(&self.picking, Some(Picking { locuteur: l, mode: Mode::Nom { .. } }) if *l == locuteur) {
            self.picking = None;
            return;
        }
        let Some(spk) = self.meeting().and_then(|m| m.locuteurs.get(locuteur)) else { return };
        let mut edit = TextEdit::new(spk.nom.clone().or_else(|| spk.proposition.clone()).unwrap_or_default());
        edit.focused = true;
        self.picking = Some(Picking { locuteur, mode: Mode::Nom { edit: Box::new(edit), error: None } });
    }

    pub fn start_merging(&mut self, locuteur: usize) {
        self.picking = match &self.picking {
            Some(Picking { locuteur: l, mode: Mode::Fusion }) if *l == locuteur => None,
            _ => Some(Picking { locuteur, mode: Mode::Fusion }),
        };
    }

    /// Donne ce nom au locuteur ; un refus (nom vide) reste affiché sous le champ.
    pub fn name_speaker(&mut self, locuteur: usize, nom: String) {
        let Some(id) = self.meeting().map(|m| m.id) else { return };
        match self.try_command(Commande::NommerLocuteur { reunion: id, locuteur, nom }) {
            Ok(()) => self.picking = None,
            Err(e) => {
                if let Some(Picking { mode: Mode::Nom { error, .. }, .. }) = &mut self.picking {
                    *error = Some(e.to_string());
                }
            }
        }
    }

    fn confirm_naming(&mut self) {
        let Some(p) = &self.picking else { return };
        let (locuteur, nom) = match &p.mode {
            Mode::Nom { edit, .. } => (p.locuteur, edit.text.clone()),
            Mode::Fusion => return,
        };
        self.name_speaker(locuteur, nom);
    }
}

/// Les noms à proposer en un clic : ceux du lexique, puis ceux déjà donnés ou proposés dans la
/// réunion, sans doublon.
fn known_names(s: &App, m: &Reunion) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    let reunion = m.locuteurs.iter().flat_map(|l| l.nom.iter().chain(&l.proposition)).cloned();
    for n in s.people.iter().cloned().chain(reunion) {
        if !v.contains(&n) {
            v.push(n);
        }
    }
    v
}

pub fn tab(s: &App, m: &Reunion, k: &Kit) -> El {
    let active: Vec<usize> = m.locuteurs_actifs().collect();
    if active.is_empty() {
        return muted(k, "Pas encore de locuteurs : ils viennent avec la transcription.");
    }
    let unconfirmed = active.iter().filter(|&&i| m.locuteurs[i].nom.is_none()).count();
    let intro = match unconfirmed {
        0 => "Tous les locuteurs sont nommés. La fiche et la transcription portent leurs noms ; « Refaire le bilan » les reprend.".to_string(),
        1 => "1 locuteur à nommer. Confirmer, corriger ou fusionner met à jour la fiche et la transcription ; « Refaire le bilan » les reprend.".to_string(),
        n => format!("{n} locuteurs à nommer. Confirmer, corriger ou fusionner met à jour la fiche et la transcription ; « Refaire le bilan » les reprend."),
    };
    let mut grid = row().w_full().gap(12.0).flex_wrap().items_start();
    for &si in &active {
        grid = grid.child(card(s, m, si, k));
    }
    col().w_full().gap(12.0).child(muted(k, intro)).child(grid)
}

fn small(label: impl Into<String>, variant: ButtonVariant) -> Button {
    Button::new(label).variant(variant).control_size(ControlSize::Small)
}

fn chip(k: &Kit, label: String, color: Option<rune::Color>) -> El {
    let mut c = row().h(24.0).px(9.0).gap(6.0).center_y().radius(12.0).fill(k.c.layer_02).hover(k.c.hover).cursor(rune::Cursor::Pointer);
    if let Some(color) = color {
        c = c.child(dot(color, 7.0));
    }
    c.child(text(label).font(k.ty.small_medium).color(k.c.text_base))
}

fn card(s: &App, m: &Reunion, si: usize, k: &Kit) -> El {
    let spk = &m.locuteurs[si];
    let id = m.id;
    let share = (m.temps_de_parole(si) * 100.0).round().max(0.0);
    let merged: Vec<usize> = (0..m.locuteurs.len()).filter(|&j| j != si && m.locuteur_affiche(j) == si).collect();
    let mut card = col().key(format!("spk{si}")).grow(1.0).min_w(320.0).gap(8.0).padding(14.0).radius(k.radius.lg).fill(k.c.layer_01).border(1.0, k.c.border_muted).child(
        row()
            .gap(8.0)
            .center_y()
            .child(dot(spk_color(spk.teinte), 10.0))
            .child(
                col()
                    .grow(1.0)
                    .min_w(0.0)
                    .child(text(spk.affiche()).font(k.ty.ui_medium).color(if spk.nom.is_some() { k.c.text_base } else { k.c.text_muted }))
                    .child(text(format!("{} · {share:.0} % du temps de parole", spk.etiquette)).font(k.ty.text_xsmall).color(k.c.text_faint)),
            )
            .child(if spk.nom.is_some() { glyph(&ui::CHECK, 13.0, k.c.success.icon) } else { el() }),
    );

    // Ce que l'agent propose, et pourquoi.
    let why = match (&spk.nom, &spk.proposition) {
        (Some(n), Some(p)) if n != p => Some(format!("Proposé par l'IA : « {p} ». {}", spk.justification)),
        _ if !spk.justification.is_empty() => Some(spk.justification.clone()),
        _ => None,
    };
    if let Some(why) = why {
        card = card.child(row().gap(6.0).items_start().child(glyph(&ui::SPARK, 10.0, k.c.text_accent).mt(3.0)).child(muted(k, why.trim().to_string()).grow(1.0).min_w(0.0)));
    }

    for j in merged {
        let label = format!("Réunit {}", m.locuteurs[j].etiquette);
        card = card.child(
            row()
                .key(format!("spk{si}-m{j}"))
                .gap(8.0)
                .center_y()
                .child(dot(spk_color(m.locuteurs[j].teinte), 7.0))
                .child(text(label).font(k.ty.small).color(k.c.text_muted).grow(1.0).min_w(0.0))
                .child(small("Séparer", ButtonVariant::GhostFaint).help("Défaire la fusion : ce locuteur retrouve ses répliques").on_press(move |s: &mut App| s.command(Commande::SeparerLocuteur { reunion: id, locuteur: j })).build(k)),
        );
    }

    let mut buttons = row().gap(6.0).flex_wrap();
    if let (None, None, Some(guess)) = (&spk.nom, spk.fusion_proposee, &spk.proposition) {
        let nom = guess.clone();
        buttons = buttons.child(small(format!("Confirmer « {guess} »"), ButtonVariant::Submit).on_press(move |s: &mut App| s.name_speaker(si, nom.clone())).build(k));
    }
    if let Some(target) = spk.fusion_proposee {
        let tname = m.locuteurs[m.locuteur_affiche(target)].affiche();
        buttons = buttons.child(small(format!("Fusionner avec {tname}"), ButtonVariant::Submit).on_press(move |s: &mut App| s.command(Commande::FusionnerLocuteurs { reunion: id, locuteur: si, dans: target })).build(k));
    }
    let naming = matches!(&s.picking, Some(Picking { locuteur, mode: Mode::Nom { .. } }) if *locuteur == si);
    let merging = matches!(&s.picking, Some(Picking { locuteur, mode: Mode::Fusion }) if *locuteur == si);
    buttons = buttons.child(small(if naming { "Fermer" } else { "Corriger" }, ButtonVariant::Ghost).on_press(move |s: &mut App| s.start_naming(si)).build(k));
    if m.locuteurs_actifs().count() > 1 {
        buttons = buttons.child(small(if merging { "Fermer" } else { "Fusionner…" }, ButtonVariant::Ghost).help("Ce locuteur est la même voix qu'un autre").on_press(move |s: &mut App| s.start_merging(si)).build(k));
    }
    card = card.child(buttons);

    if let Some(Picking { mode: Mode::Nom { edit, error }, .. }) = s.picking.as_ref().filter(|_| naming) {
        let input = TextInput::new(edit)
            .width(None)
            .on_edit(|s: &mut App| s.picking.as_mut().expect("correction du nom ouverte").edit())
            .on_submit(|s: &mut App, _| s.confirm_naming())
            .build(k)
            .key(format!("spk{si}-name"))
            .label("Nom du locuteur")
            .grow(1.0)
            .min_w(0.0);
        card = card.child(row().gap(6.0).center_y().child(input).child(small("Valider", ButtonVariant::Submit).on_press(|s: &mut App| s.confirm_naming()).build(k)));
        if let Some(e) = error {
            card = card.child(text(e.clone()).wrap().font(k.ty.small).color(ui::red()));
        }
        let mut chips = row().gap(6.0).flex_wrap();
        for n in known_names(s, m).into_iter().filter(|n| spk.nom.as_ref() != Some(n)) {
            let nom = n.clone();
            chips = chips.child(chip(k, n, None).on_press(move |s: &mut App| s.name_speaker(si, nom.clone())));
        }
        card = card.child(chips);
    }
    if merging {
        let mut chips = row().gap(6.0).flex_wrap().center_y().child(muted(k, "Même voix que :"));
        for j in m.locuteurs_actifs().filter(|&j| j != si) {
            let other = &m.locuteurs[j];
            chips = chips.child(chip(k, other.affiche(), Some(spk_color(other.teinte))).on_press(move |s: &mut App| {
                s.command(Commande::FusionnerLocuteurs { reunion: id, locuteur: si, dans: j });
                s.picking = None;
            }));
        }
        card = card.child(chips);
    }
    card
}
