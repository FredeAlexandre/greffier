//! Refaire une étape : la réunion repart dans la file à cette étape, et les suivantes sont
//! refaites aussi.

mod banc;

use std::cell::RefCell;
use std::rc::Rc;

use banc::Banc;
use greffier_coeur::adaptateurs::{Adaptateurs, ResultatDeRelecture};
use greffier_coeur::faux::{FauxAgent, FauxSeparateur};
use greffier_coeur::{Commande, Etape, IdReunion, Statut};

/// Une réunion enregistrée et traitée jusqu'au bout.
fn reunion_terminee(banc: &mut Banc) -> IdReunion {
    let id = banc.reunion_a_traiter();
    banc.executer(Commande::Traiter(id));
    banc.avancer_jusqu_a(id, |s| *s == Statut::Terminee);
    id
}

/// Refait `etape` et rend les étapes rejouées jusqu'à la fin du traitement.
fn refaire(banc: &mut Banc, id: IdReunion, etape: Etape) -> Vec<Etape> {
    banc.executer(Commande::Refaire { reunion: id, etape });
    assert_eq!(banc.reunion(id).statut, Statut::EnAttente(etape));
    banc.avancer_jusqu_a(id, |s| *s == Statut::Terminee)
}

#[test]
fn refaire_la_transcription_rejoue_les_trois_etapes() {
    let mut banc = Banc::new();
    let id = reunion_terminee(&mut banc);
    assert_eq!(refaire(&mut banc, id, Etape::Transcription), [Etape::Transcription, Etape::Relecture, Etape::Bilan]);
}

#[test]
fn refaire_la_relecture_rejoue_la_relecture_puis_le_bilan() {
    let mut banc = Banc::new();
    let id = reunion_terminee(&mut banc);
    assert_eq!(refaire(&mut banc, id, Etape::Relecture), [Etape::Relecture, Etape::Bilan]);
}

#[test]
fn refaire_le_bilan_ne_rejoue_que_le_bilan() {
    let mut banc = Banc::new();
    let id = reunion_terminee(&mut banc);
    assert_eq!(refaire(&mut banc, id, Etape::Bilan), [Etape::Bilan]);
}

#[test]
fn refaire_la_transcription_avec_un_autre_nombre_de_locuteurs_le_donne_au_separateur() {
    let demandes = Rc::new(RefCell::new(Vec::new()));
    let d = demandes.clone();
    let separateur = FauxSeparateur::new(banc::DUREE_D_ETAPE, move |demande| {
        d.borrow_mut().push(demande.nombre_de_locuteurs);
        Ok(banc::tours())
    });
    let mut banc = Banc::avec(Adaptateurs { separateur: Box::new(separateur), ..banc::adaptateurs() });
    let id = reunion_terminee(&mut banc);

    banc.executer(Commande::NombreDeLocuteurs { reunion: id, nombre: Some(3) });
    refaire(&mut banc, id, Etape::Transcription);

    assert_eq!(*demandes.borrow(), [None, Some(3)]);
    // Le nombre est gardé dans la fiche.
    let nom = banc.reunion(id).nom_du_dossier.clone();
    banc.rouvrir();
    assert_eq!(banc.reunion_nommee(&nom).nombre_de_locuteurs, Some(3));
}

#[test]
fn refaire_la_transcription_oublie_les_noms_des_anciens_locuteurs() {
    let mut banc = Banc::new();
    let id = reunion_terminee(&mut banc);
    banc.executer(Commande::NommerLocuteur { reunion: id, locuteur: 0, nom: "Odile".into() });

    refaire(&mut banc, id, Etape::Transcription);

    let r = banc.reunion(id);
    assert!(r.locuteurs.iter().all(|l| l.nom.is_none()), "de nouveaux locuteurs : {:?}", r.locuteurs);
    assert!(r.relecture.is_some(), "la relecture a été refaite");
}

#[test]
fn refaire_le_bilan_donne_a_l_agent_les_noms_donnes_depuis() {
    let noms = Rc::new(RefCell::new(Vec::new()));
    let n = noms.clone();
    let agent = FauxAgent::new(
        banc::DUREE_D_ETAPE,
        |_| Ok(ResultatDeRelecture::default()),
        banc::DUREE_D_ETAPE,
        move |d| {
            n.borrow_mut().push(d.locuteurs.iter().map(|l| l.nom.clone()).collect::<Vec<_>>());
            Ok((banc::BILAN.to_string(), banc::resultat_du_bilan()))
        },
    );
    let mut banc = Banc::avec(Adaptateurs { agent: Box::new(agent), ..banc::adaptateurs() });
    let id = reunion_terminee(&mut banc);
    banc.executer(Commande::NommerLocuteur { reunion: id, locuteur: 1, nom: "Odile".into() });

    refaire(&mut banc, id, Etape::Bilan);

    assert_eq!(*noms.borrow(), [vec![None, None], vec![None, Some("Odile".to_string())]]);
}

#[test]
fn refaire_depuis_la_relecture_donne_le_lexique_tel_qu_il_est_maintenant() {
    let lexiques = Rc::new(RefCell::new(Vec::new()));
    let l = lexiques.clone();
    let agent = FauxAgent::new(
        banc::DUREE_D_ETAPE,
        move |d| {
            l.borrow_mut().push(d.lexique.clone());
            Ok(ResultatDeRelecture::default())
        },
        banc::DUREE_D_ETAPE,
        |_| Ok((banc::BILAN.to_string(), banc::resultat_du_bilan())),
    );
    let mut banc = Banc::avec(Adaptateurs { agent: Box::new(agent), ..banc::adaptateurs() });
    let lexique = banc.dossier.join("lexique.md");
    std::fs::write(&lexique, "## Mots justes\n\n- Lumion\n").unwrap();
    banc.executer(Commande::ChangerReglages(greffier_coeur::Reglages { lexique: Some(lexique.clone()), ..banc.greffier.reglages().clone() }));
    let id = reunion_terminee(&mut banc);

    std::fs::write(&lexique, "## Mots justes\n\n- Lumion\n- Ormeau\n").unwrap();
    refaire(&mut banc, id, Etape::Relecture);

    let vus = lexiques.borrow();
    assert_eq!(vus.len(), 2);
    assert!(!vus[0].contains("Ormeau") && vus[1].contains("Ormeau"), "{vus:?}");
}

#[test]
fn une_etape_refaite_survit_a_un_redemarrage_et_reprend_a_cette_etape() {
    let mut banc = Banc::new();
    let id = reunion_terminee(&mut banc);
    banc.executer(Commande::Refaire { reunion: id, etape: Etape::Relecture });
    let nom = banc.reunion(id).nom_du_dossier.clone();

    banc.rouvrir();
    let r = banc.reunion_nommee(&nom);
    assert_eq!((r.statut.clone(), r.etape_faite), (Statut::EnAttente(Etape::Relecture), Some(Etape::Transcription)));
    let id = r.id;
    assert_eq!(banc.avancer_jusqu_a(id, |s| *s == Statut::Terminee), [Etape::Relecture, Etape::Bilan]);
}

#[test]
fn refaire_est_refuse_dans_la_file_ou_si_les_etapes_d_avant_manquent() {
    let mut banc = Banc::new();
    let neuve = banc.reunion_a_traiter();
    assert!(!banc.reunion(neuve).peut_refaire(Etape::Relecture));
    assert!(banc.greffier.executer(Commande::Refaire { reunion: neuve, etape: Etape::Relecture }).is_err());
    assert!(banc.reunion(neuve).peut_refaire(Etape::Transcription));

    banc.executer(Commande::Traiter(neuve));
    for etape in Etape::TOUTES {
        assert!(!banc.reunion(neuve).peut_refaire(etape));
        assert!(banc.greffier.executer(Commande::Refaire { reunion: neuve, etape }).is_err(), "{etape:?} pendant le traitement");
    }
    banc.avancer_jusqu_a(neuve, |s| *s == Statut::Terminee);
    for etape in Etape::TOUTES {
        assert!(banc.reunion(neuve).peut_refaire(etape), "{etape:?} une fois terminée");
    }
}

#[test]
fn une_etape_en_echec_peut_etre_refaite_depuis_plus_tot() {
    let agent = FauxAgent::new(banc::DUREE_D_ETAPE, |_| Ok(ResultatDeRelecture::default()), banc::DUREE_D_ETAPE, |_| Err("Claude ne répond pas.".into()));
    let mut banc = Banc::avec(Adaptateurs { agent: Box::new(agent), ..banc::adaptateurs() });
    let id = banc.reunion_a_traiter();
    banc.executer(Commande::Traiter(id));
    banc.avancer_jusqu_a(id, |s| matches!(s, Statut::Echec { .. }));

    banc.executer(Commande::Refaire { reunion: id, etape: Etape::Relecture });
    banc.avancer(0.1);
    assert!(matches!(banc.reunion(id).statut, Statut::EnCours { etape: Etape::Relecture, .. }));
}
