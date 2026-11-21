//! La file de traitement : une réunion à la fois, Transcription → Relecture → Bilan, arrêt, pause
//! pendant un enregistrement.

mod banc;

use banc::Banc;
use greffier_coeur::faux::FausseCapture;
use greffier_coeur::{Commande, Etape, Statut};

#[test]
fn une_reunion_a_traiter_passe_par_les_trois_etapes_jusqu_a_terminee() {
    let mut banc = Banc::new();
    let id = banc.reunion_a_traiter();
    assert_eq!(banc.reunion(id).statut, Statut::ATraiter);

    banc.executer(Commande::Traiter(id));
    let etapes = banc.avancer_jusqu_a(id, |s| *s == Statut::Terminee);

    assert_eq!(etapes, [Etape::Transcription, Etape::Relecture, Etape::Bilan]);
    let r = banc.reunion(id);
    assert_eq!(r.titre, "Point d'équipe : démo");
    let repliques: Vec<(&str, &str)> = r.repliques.iter().map(|x| (r.locuteurs[x.locuteur].etiquette.as_str(), x.texte.as_str())).collect();
    assert_eq!(repliques, [("SPEAKER_00", "Bonjour à tous."), ("SPEAKER_01", "Salut Alexandre.")]);
    assert_eq!(r.sujets.len(), 1);
    assert_eq!(r.sujets[0].titre, "Salutations");
}

#[test]
fn arreter_un_traitement_la_remet_a_traiter() {
    let mut banc = Banc::new();
    let id = banc.reunion_a_traiter();
    banc.executer(Commande::Traiter(id));
    banc.avancer(0.5);
    assert!(matches!(banc.reunion(id).statut, Statut::EnCours { etape: Etape::Transcription, .. }));

    banc.executer(Commande::ArreterTraitement(id));
    assert_eq!(banc.reunion(id).statut, Statut::ATraiter);
    banc.avancer(5.0);
    assert_eq!(banc.reunion(id).statut, Statut::ATraiter);
    assert_eq!(banc.greffier.rang(id), None);
    assert!(banc.reunion(id).repliques.is_empty());
}

#[test]
fn la_file_traite_une_reunion_a_la_fois_dans_l_ordre() {
    let mut banc = Banc::new();
    let premiere = banc.reunion_a_traiter();
    let seconde = banc.reunion_a_traiter();
    banc.executer(Commande::Traiter(premiere));
    banc.executer(Commande::Traiter(seconde));
    banc.avancer(0.5);

    assert!(banc.reunion(premiere).statut.en_traitement());
    assert_eq!(banc.reunion(seconde).statut, Statut::EnAttente(Etape::Transcription));
    assert_eq!((banc.greffier.rang(premiere), banc.greffier.rang(seconde)), (Some(0), Some(1)));

    banc.avancer_jusqu_a(premiere, |s| *s == Statut::Terminee);
    assert!(!matches!(banc.reunion(seconde).statut, Statut::Terminee));
    banc.avancer_jusqu_a(seconde, |s| *s == Statut::Terminee);
}

#[test]
fn la_reunion_entre_dans_la_file_a_l_arret_de_l_enregistrement() {
    let mut banc = Banc::new();
    assert!(banc.greffier.reglages().traiter_a_l_arret);
    let id = banc.enregistrer(3.0);
    assert_eq!(banc.reunion(id).statut, Statut::EnAttente(Etape::Transcription));
    banc.avancer_jusqu_a(id, |s| *s == Statut::Terminee);
}

#[test]
fn la_file_attend_la_fin_de_l_enregistrement() {
    let mut banc = Banc::new();
    let id = banc.reunion_a_traiter();
    banc.executer(Commande::Traiter(id));
    banc.executer(Commande::DemarrerEnregistrement);
    banc.avancer(5.0);
    assert_eq!(banc.reunion(id).statut, Statut::EnAttente(Etape::Transcription));

    banc.executer(Commande::ArreterEnregistrement);
    banc.avancer(0.1);
    assert!(banc.reunion(id).statut.en_traitement());
}

#[test]
fn l_etape_commencee_va_jusqu_au_bout_pendant_un_enregistrement() {
    let mut banc = Banc::new();
    let id = banc.reunion_a_traiter();
    banc.executer(Commande::Traiter(id));
    banc.avancer(0.5);
    banc.executer(Commande::DemarrerEnregistrement);
    banc.avancer(3.0);

    let r = banc.reunion(id);
    assert_eq!(r.statut, Statut::EnAttente(Etape::Relecture));
    assert_eq!(r.repliques.len(), 2);

    banc.executer(Commande::ArreterEnregistrement);
    let etapes = banc.avancer_jusqu_a(id, |s| *s == Statut::Terminee);
    assert_eq!(etapes, [Etape::Relecture, Etape::Bilan]);
}

#[test]
fn un_bilan_sans_decisions_met_l_etape_en_echec_et_traiter_la_reessaie() {
    let incomplet = "## En bref\n\nRien.\n\n## Sujets abordés\n\n### Salutations (0:00:00)\n\n## Actions\n\n- [ ] **Alexandre** — rien\n";
    let mut a = banc::adaptateurs();
    let bilans = std::rc::Rc::new(std::cell::Cell::new(0));
    let n = bilans.clone();
    a.agent = Box::new(greffier_coeur::faux::FauxAgent::new(
        banc::DUREE_D_ETAPE,
        |_| Ok(Default::default()),
        banc::DUREE_D_ETAPE,
        move |_| {
            n.set(n.get() + 1);
            Ok(((if n.get() == 1 { incomplet } else { banc::BILAN }).to_string(), banc::resultat_du_bilan()))
        },
    ));
    let mut banc = Banc::avec(a);
    let id = banc.reunion_a_traiter();
    banc.executer(Commande::Traiter(id));
    banc.avancer_jusqu_a(id, |s| matches!(s, Statut::Echec { .. }));
    assert_eq!(banc.reunion(id).statut, Statut::Echec { etape: Etape::Bilan, message: "Le bilan n'a pas de décisions horodatées.".into() });

    banc.executer(Commande::Traiter(id));
    let etapes = banc.avancer_jusqu_a(id, |s| *s == Statut::Terminee);
    assert_eq!(etapes, [Etape::Bilan]);
    assert_eq!(bilans.get(), 2);
}

#[test]
fn un_transcripteur_en_echec_met_la_transcription_en_echec() {
    let mut a = banc::adaptateurs();
    a.transcripteur = Box::new(greffier_coeur::faux::FauxTranscripteur::new(1.0, |_| Err("whisper a planté".into())));
    let mut banc = Banc::avec(a);
    let id = banc.reunion_a_traiter();
    banc.executer(Commande::Traiter(id));
    banc.avancer(2.0);
    assert_eq!(banc.reunion(id).statut, Statut::Echec { etape: Etape::Transcription, message: "whisper a planté".into() });
    assert!(!banc.greffier.occupe());
}

#[test]
fn sans_ressources_la_transcription_echoue_jusqu_a_leur_installation() {
    let mut a = banc::adaptateurs();
    a.ressources = Box::new(greffier_coeur::faux::FaussesRessources::absentes(2.0));
    let mut banc = Banc::avec(a);
    let id = banc.reunion_a_traiter();
    banc.executer(Commande::Traiter(id));
    banc.avancer(0.1);
    assert!(matches!(&banc.reunion(id).statut, Statut::Echec { etape: Etape::Transcription, message } if message.starts_with("Ressources manquantes")));

    banc.executer(Commande::InstallerRessources);
    let evenements = banc.avancer(2.5);
    assert!(evenements.contains(&greffier_coeur::Evenement::RessourcesInstallees));
    banc.executer(Commande::Traiter(id));
    banc.avancer_jusqu_a(id, |s| *s == Statut::Terminee);
}

#[test]
fn refaire_le_bilan_ne_rejoue_que_le_bilan() {
    let mut banc = Banc::new();
    let id = banc.reunion_a_traiter();
    banc.executer(Commande::Traiter(id));
    banc.avancer_jusqu_a(id, |s| *s == Statut::Terminee);

    banc.executer(Commande::Refaire { reunion: id, etape: Etape::Bilan });
    assert_eq!(banc.reunion(id).statut, Statut::EnAttente(Etape::Bilan));
    let etapes = banc.avancer_jusqu_a(id, |s| *s == Statut::Terminee);
    assert_eq!(etapes, [Etape::Bilan]);
}

#[test]
fn la_file_reprend_aussi_quand_l_enregistrement_est_interrompu() {
    let mut a = banc::adaptateurs();
    a.capture = Box::new(FausseCapture::new(1.0, 0.0).en_panne_a(20.0, "Flux coupé."));
    let mut banc = Banc::avec(a);
    let id = banc.reunion_a_traiter();
    banc.executer(Commande::Traiter(id));
    banc.executer(Commande::DemarrerEnregistrement);
    banc.avancer(15.0);
    assert_eq!(banc.reunion(id).statut, Statut::EnAttente(Etape::Transcription));

    banc.avancer(5.5);
    assert!(banc.greffier.enregistrement().is_none());
    assert!(banc.reunion(id).statut.en_traitement(), "{:?}", banc.reunion(id).statut);
}
