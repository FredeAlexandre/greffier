//! L'enregistrement par la fausse capture : règle de la minute, repères dans la fiche,
//! démarrage automatique du traitement, autorisations, récupération d'un enregistrement coupé.

mod banc;

use banc::Banc;
use greffier_coeur::adaptateurs::{Autorisation, Autorisations, EtatDAutorisation, Sources};
use greffier_coeur::faux::FausseCapture;
use greffier_coeur::{Commande, Etape, Evenement, Refus, Reglages, Role, Statut};

#[test]
fn un_enregistrement_de_moins_d_une_minute_est_propose_a_la_suppression() {
    let mut banc = Banc::new();
    banc.executer(Commande::DemarrerEnregistrement);
    banc.avancer(59.0);
    let evenements = banc.executer(Commande::ArreterEnregistrement);
    let id = banc.greffier.reunions()[0].id;
    assert_eq!(evenements[0], Evenement::ReunionEnregistree(id));
    assert!(evenements.contains(&Evenement::SuppressionProposee(id)), "{evenements:?}");
}

#[test]
fn un_enregistrement_d_une_minute_ou_plus_n_est_pas_propose_a_la_suppression() {
    let mut banc = Banc::new();
    banc.executer(Commande::DemarrerEnregistrement);
    banc.avancer(60.0);
    let evenements = banc.executer(Commande::ArreterEnregistrement);
    assert!(!evenements.iter().any(|e| matches!(e, Evenement::SuppressionProposee(_))), "{evenements:?}");
}

#[test]
fn les_reperes_poses_sont_gardes_dans_la_fiche_et_relus_au_lancement_suivant() {
    let mut banc = Banc::new();
    banc.executer(Commande::DemarrerEnregistrement);
    banc.avancer(312.0);
    banc.executer(Commande::PoserRepere);
    banc.avancer(10.5);
    banc.executer(Commande::PoserRepere);
    banc.avancer(5.0);
    let id = banc.enregistrer_la_suite();
    let nom = banc.reunion(id).nom_du_dossier.clone();

    let fiche = banc.lire(&banc.dossier.join(format!("{nom}/{nom} - réunion.md")));
    assert!(fiche.contains("reperes:\n  - instant: \"0:05:12\"\n    libelle: \"Repère 1\"\n  - instant: \"0:05:22.5\"\n    libelle: \"Repère 2\"\n"), "{fiche}");

    banc.rouvrir();
    let r = banc.reunion_nommee(&nom);
    let reperes: Vec<(f64, &str)> = r.reperes.iter().map(|x| (x.instant, x.libelle.as_str())).collect();
    assert_eq!(reperes, [(312.0, "Repère 1"), (322.5, "Repère 2")]);
}

#[test]
fn sans_le_demarrage_automatique_la_reunion_reste_a_traiter() {
    let mut banc = Banc::new();
    banc.executer(Commande::ChangerReglages(Reglages { traiter_a_l_arret: false, ..Reglages::default() }));
    let id = banc.enregistrer(90.0);
    assert_eq!(banc.reunion(id).statut, Statut::ATraiter);
    banc.avancer(5.0);
    assert_eq!(banc.reunion(id).statut, Statut::ATraiter);
}

#[test]
fn avec_le_demarrage_automatique_la_reunion_est_mise_en_file_et_traitee() {
    let mut banc = Banc::new();
    let id = banc.enregistrer(90.0);
    assert_eq!(banc.reunion(id).statut, Statut::EnAttente(Etape::Transcription));
    assert_eq!(banc.avancer_jusqu_a(id, |s| *s == Statut::Terminee), [Etape::Transcription, Etape::Relecture, Etape::Bilan]);
}

fn banc_avec_autorisations(autorisations: Autorisations) -> Banc {
    let mut a = banc::adaptateurs();
    a.capture = Box::new(FausseCapture::new(1.0, 0.0).avec_autorisations(autorisations));
    Banc::avec(a)
}

fn refus(banc: &mut Banc, commande: Commande) -> String {
    match banc.greffier.executer(commande) {
        Err(Refus::Impossible(m)) => m,
        autre => panic!("refus attendu, obtenu {autre:?}"),
    }
}


#[test]
fn sans_l_autorisation_de_l_ecran_l_enregistrement_est_refuse_sans_rien_creer() {
    let mut banc = banc_avec_autorisations(Autorisations { ecran: EtatDAutorisation::Refusee, ..Autorisations::ACCORDEES });
    assert_eq!(banc.greffier.autorisations().ecran, EtatDAutorisation::Refusee);

    let message = refus(&mut banc, Commande::DemarrerEnregistrement);
    assert!(message.contains("Enregistrement de l'écran"), "{message}");
    assert!(banc.greffier.enregistrement().is_none());
    assert_eq!(std::fs::read_dir(&banc.dossier).unwrap().count(), 0);
}

#[test]
fn une_source_coupee_n_a_pas_besoin_de_son_autorisation() {
    let mut banc = banc_avec_autorisations(Autorisations { micro: EtatDAutorisation::Refusee, ..Autorisations::ACCORDEES });
    assert!(refus(&mut banc, Commande::DemarrerEnregistrement).contains("Micro"));

    let mut reglages = Reglages::default();
    reglages.sources.micro = false;
    banc.executer(Commande::ChangerReglages(reglages));
    banc.executer(Commande::DemarrerEnregistrement);
    assert_eq!(banc.greffier.enregistrement().expect("en cours").sources, Sources { ecran: true, son_du_mac: true, micro: false });
}

#[test]
fn une_autorisation_jamais_demandee_l_est_au_demarrage() {
    let mut banc = banc_avec_autorisations(Autorisations { micro: EtatDAutorisation::ADemander, ..Autorisations::ACCORDEES });
    let message = refus(&mut banc, Commande::DemarrerEnregistrement);
    assert!(message.contains("micro"), "{message}");
    // La fausse capture accepte toute demande, comme l'utilisateur qui clique « Autoriser ».
    assert_eq!(banc.greffier.autorisations(), Autorisations::ACCORDEES);
    banc.executer(Commande::DemarrerEnregistrement);
    assert!(banc.greffier.enregistrement().is_some());
}

#[test]
fn demander_une_autorisation_depuis_les_reglages() {
    let mut banc = banc_avec_autorisations(Autorisations { ecran: EtatDAutorisation::ADemander, ..Autorisations::ACCORDEES });
    banc.executer(Commande::DemanderAutorisation(Autorisation::Ecran));
    assert_eq!(banc.greffier.autorisations().ecran, EtatDAutorisation::Accordee);
}

#[test]
fn sans_aucune_source_l_enregistrement_est_refuse() {
    let mut banc = Banc::new();
    banc.executer(Commande::ChangerReglages(Reglages { sources: Sources { ecran: false, son_du_mac: false, micro: false }, ..Reglages::default() }));
    assert!(refus(&mut banc, Commande::DemarrerEnregistrement).contains("source"));
}

#[test]
fn un_enregistrement_coupe_garde_la_duree_deja_ecrite_dans_ses_fragments() {
    let mut banc = Banc::new();
    banc.executer(Commande::DemarrerEnregistrement);
    banc.avancer(90.0);
    banc.executer(Commande::PoserRepere);
    banc.avancer(4.0);
    // L'app s'arrête net : pas d'ArreterEnregistrement.
    banc.rouvrir();

    let r = banc.reunion_nommee("2026-11-21 14h02 - Réunion");
    assert_eq!(r.statut, Statut::EnregistrementInterrompu);
    assert!((r.duree - 94.0).abs() < 0.05, "durée {}", r.duree);
}

#[test]
fn la_fausse_capture_ecrit_un_mp4_dont_on_lit_la_duree() {
    let mut banc = Banc::new();
    let id = banc.enregistrer(75.0);
    let nom = banc.reunion(id).nom_du_dossier.clone();
    let fichier = banc.greffier.dossier().fichier(&nom, Role::Enregistrement);
    let duree = greffier_coeur::dossier::duree_du_mp4(&fichier).expect("durée du mp4");
    assert!((duree - 75.0).abs() < 0.05, "{duree}");
}
