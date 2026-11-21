//! Supprimer une réunion : son dossier entier part à la corbeille du Mac, pour pouvoir s'en
//! rattraper. Le cœur passe par l'adaptateur de corbeille (ici une fausse corbeille).

mod banc;

use banc::Banc;
use greffier_coeur::adaptateurs::Adaptateurs;
use greffier_coeur::faux::FausseCorbeille;
use greffier_coeur::{Commande, Refus, Statut};

const RELEASE: &str = "2026-11-20 10h00 - Revue de release";

fn banc_avec(corbeille: &FausseCorbeille) -> Banc {
    let mut banc = Banc::sur_les_fixtures();
    banc.greffier = greffier_coeur::Greffier::ouvrir(&banc.dossier, Adaptateurs { corbeille: Box::new(corbeille.clone()), ..banc::adaptateurs() });
    banc
}

#[test]
fn supprimer_envoie_le_dossier_a_la_corbeille() {
    let corbeille = FausseCorbeille::new();
    let mut banc = banc_avec(&corbeille);
    let id = banc.reunion_nommee(RELEASE).id;

    banc.executer(Commande::Supprimer(id));

    assert!(banc.greffier.reunion(id).is_none());
    assert!(!banc.dossier.join(RELEASE).exists());
    assert_eq!(corbeille.jetes(), [banc.dossier.join(RELEASE)]);
    // Tout le dossier est dans la corbeille, enregistrement et bilan compris.
    let jete = corbeille.dossier().join(RELEASE);
    assert!(jete.join(format!("{RELEASE} - bilan.md")).is_file());
    assert!(jete.join(format!("{RELEASE} - enregistrement.mp4")).is_file());
    assert!(banc.greffier.rechercher("prévient").is_empty());

    // Rien ne revient : ni à la relecture du dossier, ni au lancement suivant.
    assert!(banc.greffier.relire_le_dossier().is_empty());
    banc.rouvrir();
    assert!(banc.greffier.reunions().iter().all(|r| r.nom_du_dossier != RELEASE));
}

#[test]
fn une_corbeille_qui_refuse_garde_la_reunion() {
    let corbeille = FausseCorbeille::new().en_panne("Le volume est en lecture seule.");
    let mut banc = banc_avec(&corbeille);
    let id = banc.reunion_nommee(RELEASE).id;

    let refus = banc.greffier.executer(Commande::Supprimer(id)).unwrap_err();

    let Refus::Impossible(message) = refus else { panic!("{refus:?}") };
    assert!(message.contains("Le volume est en lecture seule."), "{message}");
    assert!(banc.greffier.reunion(id).is_some());
    assert!(banc.dossier.join(RELEASE).join(format!("{RELEASE} - bilan.md")).is_file());
    assert_eq!(banc.greffier.rechercher("prévient").len(), 1);
}

#[test]
fn supprimer_une_reunion_en_traitement_arrete_le_traitement() {
    let corbeille = FausseCorbeille::new();
    let mut banc = Banc::avec(Adaptateurs { corbeille: Box::new(corbeille.clone()), ..banc::adaptateurs() });
    let id = banc.reunion_a_traiter();
    let nom = banc.reunion(id).nom_du_dossier.clone();
    banc.executer(Commande::Traiter(id));
    banc.avancer(0.5);
    assert!(matches!(banc.reunion(id).statut, Statut::EnCours { .. }));

    banc.executer(Commande::Supprimer(id));
    banc.avancer(5.0);

    assert!(banc.greffier.reunion(id).is_none());
    assert!(!banc.greffier.occupe());
    assert!(!banc.dossier.join(&nom).exists(), "le dossier ne doit pas être réécrit");
    assert_eq!(corbeille.jetes(), [banc.dossier.join(&nom)]);
}

#[test]
fn une_reunion_dont_le_dossier_a_deja_disparu_se_supprime_sans_corbeille() {
    let corbeille = FausseCorbeille::new();
    let mut banc = banc_avec(&corbeille);
    let id = banc.reunion_nommee(RELEASE).id;
    std::fs::remove_dir_all(banc.dossier.join(RELEASE)).unwrap();

    banc.executer(Commande::Supprimer(id));

    assert!(banc.greffier.reunion(id).is_none());
    assert!(corbeille.jetes().is_empty());
}
