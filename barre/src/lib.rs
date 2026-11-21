//! L'icône de Greffier dans la barre des menus. Rune ne la fournit pas encore sur macOS : on
//! l'écrit en `objc2` (NSStatusItem), depuis le thread principal (ADR 0004).
//!
//! Ce qu'elle montre se calcule ici, sans AppKit ([`menu`]) ; l'adaptateur macOS
//! ([`IconeDeLaBarre`]) ne fait que l'afficher et rendre les [`Choix`] de l'utilisateur. Ni
//! le cœur ni l'interface Rune ne voient `objc2`.

/// Ce que l'app dit à la barre des menus.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Etat {
    /// Aucun enregistrement. `possible` est faux tant qu'un enregistrement ne peut pas partir
    /// (dossier de réunions pas encore choisi).
    Pret { possible: bool },
    /// Un enregistrement tourne depuis `duree` secondes.
    Enregistrement { duree: f64 },
}

/// Ce que l'utilisateur choisit dans le menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choix {
    Demarrer,
    Arreter,
    Ouvrir,
    Quitter,
}

/// L'aspect de l'icône.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icone {
    Repos,
    Enregistrement,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Ligne {
    /// Une ligne qu'on peut choisir (grisée si `active` est faux).
    Choix { texte: String, choix: Choix, active: bool },
    /// Une ligne d'information, jamais choisie.
    Info(String),
    Separateur,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Menu {
    pub icone: Icone,
    pub lignes: Vec<Ligne>,
}

/// L'icône et son menu pour un état de l'app.
pub fn menu(etat: Etat) -> Menu {
    let ligne = |texte: &str, choix, active| Ligne::Choix { texte: texte.into(), choix, active };
    let (icone, mut lignes) = match etat {
        Etat::Pret { possible } => (Icone::Repos, vec![ligne("Démarrer l'enregistrement", Choix::Demarrer, possible)]),
        Etat::Enregistrement { duree } => (Icone::Enregistrement, vec![Ligne::Info(texte_de_la_duree(duree)), ligne("Arrêter l'enregistrement", Choix::Arreter, true)]),
    };
    // Quitter pendant un enregistrement le couperait : on l'arrête d'abord.
    let quitter = matches!(etat, Etat::Pret { .. });
    lignes.extend([Ligne::Separateur, ligne("Ouvrir Greffier", Choix::Ouvrir, true), Ligne::Separateur, ligne("Quitter Greffier", Choix::Quitter, quitter)]);
    Menu { icone, lignes }
}

/// La ligne d'information de l'enregistrement : « Enregistrement en cours · 0:12:34 ».
pub fn texte_de_la_duree(duree: f64) -> String {
    let t = duree.max(0.0) as u64;
    format!("Enregistrement en cours · {}:{:02}:{:02}", t / 3600, (t / 60) % 60, t % 60)
}

#[cfg(target_os = "macos")]
mod mac;
#[cfg(target_os = "macos")]
pub use mac::IconeDeLaBarre;
