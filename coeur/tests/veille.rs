//! Le Mac ne se met pas en veille pendant un enregistrement ou un traitement : le cœur dit quand
//! il faut le garder éveillé (l'interface en fait une assertion de non-veille de macOS).

mod banc;

use banc::Banc;
use greffier_coeur::faux::FaussesRessources;
use greffier_coeur::{Commande, Reglages, Statut};

#[test]
fn le_mac_reste_eveille_pendant_un_enregistrement_et_plus_apres() {
    let mut banc = Banc::new();
    banc.executer(Commande::ChangerReglages(Reglages { traiter_a_l_arret: false, ..Reglages::default() }));
    assert_eq!(banc.greffier.garder_le_mac_eveille(), None);

    banc.executer(Commande::DemarrerEnregistrement);
    banc.avancer(2.0);
    assert_eq!(banc.greffier.garder_le_mac_eveille(), Some("Greffier enregistre une réunion"));

    banc.enregistrer_la_suite();
    assert_eq!(banc.greffier.garder_le_mac_eveille(), None);
}

#[test]
fn le_mac_reste_eveille_du_debut_a_la_fin_du_traitement() {
    let mut banc = Banc::new();
    let id = banc.reunion_a_traiter();
    banc.executer(Commande::Traiter(id));
    // Dans la file, l'étape démarre au prochain pas : la veille est déjà empêchée.
    assert_eq!(banc.greffier.garder_le_mac_eveille(), Some("Greffier traite une réunion"));

    banc.avancer_jusqu_a(id, |s| matches!(s, Statut::EnCours { .. }));
    assert_eq!(banc.greffier.garder_le_mac_eveille(), Some("Greffier traite une réunion"));

    banc.avancer_jusqu_a(id, |s| *s == Statut::Terminee);
    assert_eq!(banc.greffier.garder_le_mac_eveille(), None);
}

#[test]
fn l_enregistrement_prime_quand_la_file_attend() {
    let mut banc = Banc::new();
    let id = banc.reunion_a_traiter();
    banc.executer(Commande::Traiter(id));
    banc.executer(Commande::DemarrerEnregistrement);
    banc.avancer(1.0);
    assert_eq!(banc.greffier.garder_le_mac_eveille(), Some("Greffier enregistre une réunion"));
}

#[test]
fn un_traitement_arrete_ou_en_echec_laisse_le_mac_se_mettre_en_veille() {
    let mut banc = Banc::new();
    let id = banc.reunion_a_traiter();
    banc.executer(Commande::Traiter(id));
    banc.avancer(0.5);
    banc.executer(Commande::ArreterTraitement(id));
    assert_eq!(banc.greffier.garder_le_mac_eveille(), None);

    let mut a = banc::adaptateurs();
    a.ressources = Box::new(FaussesRessources::absentes(1.0));
    let mut banc = Banc::avec(a);
    let id = banc.reunion_a_traiter();
    banc.executer(Commande::Traiter(id));
    banc.avancer_jusqu_a(id, |s| matches!(s, Statut::Echec { .. }));
    assert_eq!(banc.greffier.garder_le_mac_eveille(), None);
}
