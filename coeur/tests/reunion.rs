//! Les commandes sur une réunion : enregistrement, repères, locuteurs, passages à vérifier,
//! actions du bilan, suppression.

mod banc;

use banc::Banc;
use greffier_coeur::adaptateurs::{PassageSignale, ResultatDeRelecture};
use greffier_coeur::faux::FauxAgent;
use greffier_coeur::{Commande, IdReunion, Role, Statut, StatutDuPassage, Tranche};

fn reunion_terminee(banc: &mut Banc) -> IdReunion {
    let id = banc.reunion_a_traiter();
    banc.executer(Commande::Traiter(id));
    banc.avancer_jusqu_a(id, |s| *s == Statut::Terminee);
    id
}

#[test]
fn un_enregistrement_cree_le_dossier_de_la_reunion_avec_ses_reperes() {
    let mut banc = Banc::new();
    banc.executer(Commande::DemarrerEnregistrement);
    banc.executer(Commande::NombreDeLocuteursEnregistres(Some(3)));
    banc.avancer(2.0);
    banc.executer(Commande::PoserRepere);
    banc.avancer(1.0);
    assert!((banc.greffier.enregistrement().expect("en cours").duree - 3.0).abs() < 1e-9);
    let id = banc.enregistrer_la_suite();

    let r = banc.reunion(id);
    assert_eq!(r.nom_du_dossier, "2026-11-21 14h02 - Réunion");
    assert_eq!(r.titre, "Réunion");
    assert_eq!(r.nombre_de_locuteurs, Some(3));
    assert_eq!(r.reperes.len(), 1);
    assert_eq!(r.reperes[0].libelle, "Repère 1");
    assert!((r.reperes[0].instant - 2.0).abs() < 1e-9);
    let fichier = banc.dossier.join("2026-11-21 14h02 - Réunion/2026-11-21 14h02 - Réunion - enregistrement.mp4");
    assert!(fichier.exists(), "{}", fichier.display());
    assert_eq!(banc.greffier.dossier().fichier(&r.nom_du_dossier, Role::Enregistrement), fichier);
}

#[test]
fn deux_enregistrements_dans_la_meme_minute_ont_chacun_leur_dossier() {
    let mut banc = Banc::new();
    let a = banc.reunion_a_traiter();
    let b = banc.reunion_a_traiter();
    assert_eq!(banc.reunion(a).nom_du_dossier, "2026-11-21 14h02 - Réunion");
    assert_eq!(banc.reunion(b).nom_du_dossier, "2026-11-21 14h02 - Réunion (2)");
}

#[test]
fn le_bilan_ecrit_son_fichier_et_propose_un_nom_par_locuteur() {
    let mut banc = Banc::new();
    let id = reunion_terminee(&mut banc);
    let r = banc.reunion(id);

    let fichier = banc.dossier.join("2026-11-21 14h02 - Point d'équipe - démo/2026-11-21 14h02 - Point d'équipe - démo - bilan.md");
    assert_eq!(banc.lire(&fichier), banc::BILAN);
    assert_eq!(r.resume, "Alexandre salue l'équipe.");
    assert_eq!(r.actions_principales, ["Alexandre — préparer la démo — lundi", "Khalid — relire la PR"]);
    assert_eq!(r.sessions.bilan.as_deref(), Some("session-bilan"));
    assert_eq!(r.decisions[0].instant, 3.0);
    let noms: Vec<String> = r.locuteurs.iter().map(|l| l.affiche()).collect();
    assert_eq!(noms, ["Alexandre ?", "Khalid ?"]);
    assert!(r.nouvelle);
}

#[test]
fn nommer_un_locuteur_puis_en_fusionner_un_autre_dedans() {
    let mut banc = Banc::new();
    let id = reunion_terminee(&mut banc);

    banc.executer(Commande::NommerLocuteur { reunion: id, locuteur: 0, nom: "Alexandre".into() });
    assert_eq!(banc.reunion(id).locuteurs[0].affiche(), "Alexandre");

    banc.executer(Commande::FusionnerLocuteurs { reunion: id, locuteur: 1, dans: 0 });
    let r = banc.reunion(id);
    assert_eq!(r.locuteurs_actifs().collect::<Vec<_>>(), [0]);
    assert_eq!(r.locuteur_affiche(r.repliques[1].locuteur), 0);
    assert_eq!(r.temps_de_parole(0), 1.0);

    let refus = banc.greffier.executer(Commande::FusionnerLocuteurs { reunion: id, locuteur: 0, dans: 1 });
    assert!(refus.is_err(), "fusionner un locuteur dans celui qu'il a absorbé");
}

#[test]
fn la_relecture_corrige_et_signale_les_passages_a_trancher() {
    let mut a = banc::adaptateurs();
    a.agent = Box::new(FauxAgent::new(
        banc::DUREE_D_ETAPE,
        |_| {
            Ok(ResultatDeRelecture {
                session: Some("session-relecture".into()),
                corrections: vec![greffier_coeur::adaptateurs::CorrectionSure { replique: 0, avant: "tous".into(), apres: "toutes et tous".into() }],
                passages_a_verifier: vec![PassageSignale { replique: 1, extrait: "Alexandre".into(), hypothese: Some("Alexandra".into()), reecoute: "« Salut Alexandre. »".into() }],
                ..Default::default()
            })
        },
        banc::DUREE_D_ETAPE,
        |_| Ok((banc::BILAN.to_string(), banc::resultat_du_bilan())),
    ));
    let mut banc = Banc::avec(a);
    let id = reunion_terminee(&mut banc);
    let r = banc.reunion(id);
    assert_eq!(r.repliques[0].texte, "Bonjour à toutes et tous.");
    assert_eq!(r.corrections, 1);
    assert_eq!(r.sessions.relecture.as_deref(), Some("session-relecture"));
    assert_eq!(r.passages_en_attente(), 1);
    assert_eq!(r.repliques[1].passages, [0]);

    banc.executer(Commande::Trancher { reunion: id, passage: 0, tranche: Tranche::AccepterHypothese });
    assert_eq!(banc.reunion(id).passages[0].statut, StatutDuPassage::HypotheseAcceptee);
    assert_eq!(banc.reunion(id).passages_en_attente(), 0);

    banc.executer(Commande::Trancher { reunion: id, passage: 0, tranche: Tranche::Corriger("Alexandre !".into()) });
    assert_eq!(banc.reunion(id).passages[0].statut, StatutDuPassage::CorrigeALaMain("Alexandre !".into()));

    banc.executer(Commande::Trancher { reunion: id, passage: 0, tranche: Tranche::Rouvrir });
    assert_eq!(banc.reunion(id).passages_en_attente(), 1);
}

#[test]
fn cocher_une_action_change_la_case_dans_le_fichier_du_bilan() {
    let mut banc = Banc::new();
    let id = reunion_terminee(&mut banc);
    banc.executer(Commande::CocherAction { reunion: id, action: 1, faite: true });

    let fichier = banc.dossier.join("2026-11-21 14h02 - Point d'équipe - démo/2026-11-21 14h02 - Point d'équipe - démo - bilan.md");
    let md = banc.lire(&fichier);
    assert!(md.contains("- [ ] **Alexandre** — préparer la démo — lundi\n- [x] **Khalid** — relire la PR\n"), "{md}");
    assert_eq!(banc.reunion(id).bilan, md);
}

#[test]
fn supprimer_une_reunion_en_traitement_libere_la_file() {
    let mut banc = Banc::new();
    let premiere = banc.reunion_a_traiter();
    let seconde = banc.reunion_a_traiter();
    banc.executer(Commande::Traiter(premiere));
    banc.executer(Commande::Traiter(seconde));
    banc.avancer(0.5);

    banc.executer(Commande::Supprimer(premiere));
    assert!(banc.greffier.reunion(premiere).is_none());
    banc.avancer(0.1);
    assert!(banc.reunion(seconde).statut.en_traitement());
}

#[test]
fn ouvrir_une_reunion_terminee_la_marque_lue() {
    let mut banc = Banc::new();
    let id = reunion_terminee(&mut banc);
    banc.executer(Commande::MarquerLue(id));
    assert!(!banc.reunion(id).nouvelle);
}
