//! Ce que montre l'icône de la barre des menus, selon l'état de l'app.

use greffier_barre::{Choix, Etat, Icone, Ligne, menu};

fn choix(texte: &str, choix: Choix) -> Ligne {
    Ligne::Choix { texte: texte.into(), choix, active: true }
}

#[test]
fn au_repos_le_menu_propose_de_demarrer_et_d_ouvrir_greffier() {
    let m = menu(Etat::Pret { possible: true });
    assert_eq!(m.icone, Icone::Repos);
    assert_eq!(
        m.lignes,
        vec![
            choix("Démarrer l'enregistrement", Choix::Demarrer),
            Ligne::Separateur,
            choix("Ouvrir Greffier", Choix::Ouvrir),
            Ligne::Separateur,
            choix("Quitter Greffier", Choix::Quitter),
        ]
    );
}

#[test]
fn pendant_l_enregistrement_l_icone_change_et_le_menu_affiche_la_duree() {
    let m = menu(Etat::Enregistrement { duree: 754.6 });
    assert_eq!(m.icone, Icone::Enregistrement);
    assert_eq!(m.lignes[0], Ligne::Info("Enregistrement en cours · 0:12:34".into()));
    assert_eq!(m.lignes[1], choix("Arrêter l'enregistrement", Choix::Arreter));
    assert!(m.lignes.contains(&choix("Ouvrir Greffier", Choix::Ouvrir)));
    assert!(!m.lignes.iter().any(|l| matches!(l, Ligne::Choix { choix: Choix::Demarrer, .. })));
}

#[test]
fn la_duree_passe_aux_heures() {
    let m = menu(Etat::Enregistrement { duree: 2.0 * 3600.0 + 5.0 * 60.0 + 9.99 });
    assert_eq!(m.lignes[0], Ligne::Info("Enregistrement en cours · 2:05:09".into()));
}

#[test]
fn sans_dossier_de_reunions_demarrer_est_grise() {
    let m = menu(Etat::Pret { possible: false });
    assert_eq!(m.lignes[0], Ligne::Choix { texte: "Démarrer l'enregistrement".into(), choix: Choix::Demarrer, active: false });
    assert!(m.lignes.contains(&choix("Ouvrir Greffier", Choix::Ouvrir)));
}

#[test]
fn on_ne_quitte_pas_depuis_le_menu_pendant_un_enregistrement() {
    let m = menu(Etat::Enregistrement { duree: 3.0 });
    assert!(m.lignes.contains(&Ligne::Choix { texte: "Quitter Greffier".into(), choix: Choix::Quitter, active: false }));
}
