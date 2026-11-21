//! Trancher les passages à vérifier et corriger la transcription à la main : la réplique, la
//! transcription structurée et la transcription lisible suivent.

mod banc;

use banc::Banc;
use greffier_coeur::adaptateurs::{PassageSignale, ResultatDeRelecture};
use greffier_coeur::dossier::transcription::TranscriptionStructuree;
use greffier_coeur::faux::FauxAgent;
use greffier_coeur::{Commande, IdReunion, Role, Statut, StatutDuPassage, Tranche};

/// Une réunion terminée dont la relecture a laissé deux passages à vérifier : « tous » sans
/// hypothèse dans « Bonjour à tous. », « Alexandre » (hypothèse « Alexandra ») dans « Salut
/// Alexandre. ».
fn reunion_a_verifier() -> (Banc, IdReunion) {
    let mut a = banc::adaptateurs();
    a.agent = Box::new(FauxAgent::new(
        banc::DUREE_D_ETAPE,
        |_| {
            Ok(ResultatDeRelecture {
                passages_a_verifier: vec![
                    PassageSignale { replique: 0, extrait: "tous".into(), hypothese: None, reecoute: "Bonjour à tous.".into() },
                    PassageSignale { replique: 1, extrait: "Alexandre".into(), hypothese: Some("Alexandra".into()), reecoute: "Salut Alexandra.".into() },
                ],
                ..Default::default()
            })
        },
        banc::DUREE_D_ETAPE,
        |_| Ok((banc::BILAN.to_string(), banc::resultat_du_bilan())),
    ));
    let mut banc = Banc::avec(a);
    let id = banc.reunion_a_traiter();
    banc.executer(Commande::Traiter(id));
    banc.avancer_jusqu_a(id, |s| *s == Statut::Terminee);
    (banc, id)
}

fn lisible(banc: &Banc, id: IdReunion) -> String {
    let r = banc.reunion(id);
    banc.lire(&banc.greffier.dossier().fichier(&r.nom_du_dossier, Role::TranscriptionLisible))
}

fn structuree(banc: &Banc, id: IdReunion) -> TranscriptionStructuree {
    let r = banc.reunion(id);
    TranscriptionStructuree::lire(&banc.lire(&banc.greffier.dossier().fichier(&r.nom_du_dossier, Role::TranscriptionStructuree))).expect("json")
}

fn textes(banc: &Banc, id: IdReunion) -> Vec<String> {
    banc.reunion(id).repliques.iter().map(|x| x.texte.clone()).collect()
}

#[test]
fn accepter_l_hypothese_remplace_l_extrait_partout_et_retire_le_marquage() {
    let (mut banc, id) = reunion_a_verifier();
    assert!(lisible(&banc, id).contains("> À vérifier : « Alexandre » (hypothèse : « Alexandra »)"));

    banc.executer(Commande::Trancher { reunion: id, passage: 1, tranche: Tranche::AccepterHypothese });

    assert_eq!(textes(&banc, id), ["Bonjour à tous.", "Salut Alexandra."]);
    assert_eq!(banc.reunion(id).passages_en_attente(), 1);
    let json = structuree(&banc, id);
    assert_eq!(json.repliques[1].texte, "Salut Alexandra.");
    assert_eq!(json.passages[1].statut, "hypothese_acceptee");
    let md = lisible(&banc, id);
    assert!(md.contains("Salut Alexandra.\n"), "{md}");
    assert!(!md.contains("« Alexandre »"), "{md}");
    assert!(md.contains("> À vérifier : « tous »"), "{md}");
}

#[test]
fn garder_l_original_laisse_le_texte_et_retire_le_marquage() {
    let (mut banc, id) = reunion_a_verifier();

    banc.executer(Commande::Trancher { reunion: id, passage: 0, tranche: Tranche::GarderOriginal });

    assert_eq!(textes(&banc, id), ["Bonjour à tous.", "Salut Alexandre."]);
    assert_eq!(banc.reunion(id).passages[0].statut, StatutDuPassage::OriginalGarde);
    assert_eq!(structuree(&banc, id).passages[0].statut, "original_garde");
    let md = lisible(&banc, id);
    assert!(!md.contains("« tous »"), "{md}");
    assert!(md.contains("Bonjour à tous.\n"), "{md}");
}

#[test]
fn corriger_a_la_main_met_la_correction_a_la_place_de_l_extrait() {
    let (mut banc, id) = reunion_a_verifier();

    banc.executer(Commande::Trancher { reunion: id, passage: 0, tranche: Tranche::Corriger(" toutes et tous ".into()) });

    assert_eq!(textes(&banc, id), ["Bonjour à toutes et tous.", "Salut Alexandre."]);
    assert_eq!(banc.reunion(id).passages[0].statut, StatutDuPassage::CorrigeALaMain("toutes et tous".into()));
    let json = structuree(&banc, id);
    assert_eq!(json.repliques[0].texte, "Bonjour à toutes et tous.");
    assert_eq!((json.passages[0].statut.as_str(), json.passages[0].correction.as_deref()), ("corrige_a_la_main", Some("toutes et tous")));
    let md = lisible(&banc, id);
    assert!(md.contains("Bonjour à toutes et tous.\n"), "{md}");
    assert!(!md.contains("« tous »"), "{md}");
}

#[test]
fn une_correction_vide_est_refusee() {
    let (mut banc, id) = reunion_a_verifier();
    assert!(banc.greffier.executer(Commande::Trancher { reunion: id, passage: 0, tranche: Tranche::Corriger("  ".into()) }).is_err());
    assert!(banc.greffier.executer(Commande::Trancher { reunion: id, passage: 0, tranche: Tranche::AccepterHypothese }).is_err(), "pas d'hypothèse");
    assert_eq!(banc.reunion(id).passages_en_attente(), 2);
}

#[test]
fn rouvrir_un_passage_remet_l_extrait_et_le_marquage() {
    let (mut banc, id) = reunion_a_verifier();
    banc.executer(Commande::Trancher { reunion: id, passage: 1, tranche: Tranche::AccepterHypothese });
    banc.executer(Commande::Trancher { reunion: id, passage: 1, tranche: Tranche::Corriger("Alexandrine".into()) });
    assert_eq!(textes(&banc, id)[1], "Salut Alexandrine.");

    banc.executer(Commande::Trancher { reunion: id, passage: 1, tranche: Tranche::Rouvrir });

    assert_eq!(textes(&banc, id)[1], "Salut Alexandre.");
    assert_eq!(banc.reunion(id).passages_en_attente(), 2);
    assert!(lisible(&banc, id).contains("> À vérifier : « Alexandre » (hypothèse : « Alexandra »)"));
}

#[test]
fn les_passages_tranches_se_retrouvent_au_lancement_suivant() {
    let (mut banc, id) = reunion_a_verifier();
    banc.executer(Commande::Trancher { reunion: id, passage: 0, tranche: Tranche::Corriger("vous".into()) });
    banc.executer(Commande::Trancher { reunion: id, passage: 1, tranche: Tranche::AccepterHypothese });
    let nom = banc.reunion(id).nom_du_dossier.clone();

    banc.rouvrir();

    let r = banc.reunion_nommee(&nom);
    let textes: Vec<&str> = r.repliques.iter().map(|x| x.texte.as_str()).collect();
    assert_eq!(textes, ["Bonjour à vous.", "Salut Alexandra."]);
    assert_eq!(r.passages_en_attente(), 0);
    assert_eq!(r.passages[0].statut, StatutDuPassage::CorrigeALaMain("vous".into()));
    let (id, passage) = (r.id, 0);
    banc.executer(Commande::Trancher { reunion: id, passage, tranche: Tranche::Rouvrir });
    assert_eq!(banc.reunion(id).repliques[0].texte, "Bonjour à tous.");
}

#[test]
fn corriger_un_mot_l_enregistre_comme_correction_a_la_main() {
    let (mut banc, id) = reunion_a_verifier();

    banc.executer(Commande::CorrigerReplique { reunion: id, replique: 0, texte: "Bonsoir à tous.".into() });

    assert_eq!(textes(&banc, id), ["Bonsoir à tous.", "Salut Alexandre."]);
    let json = structuree(&banc, id);
    let corrections: Vec<(usize, &str, &str)> = json.corrections_a_la_main.iter().map(|c| (c.replique, c.avant.as_str(), c.apres.as_str())).collect();
    assert_eq!(corrections, [(0, "Bonjour", "Bonsoir")]);
    assert_eq!(json.repliques[0].texte, "Bonsoir à tous.");
    let md = lisible(&banc, id);
    assert!(md.contains("Bonsoir à tous.\n"), "{md}");
    // Le passage à vérifier de la réplique est toujours là, et se tranche encore.
    assert!(md.contains("> À vérifier : « tous »"), "{md}");
    banc.executer(Commande::Trancher { reunion: id, passage: 0, tranche: Tranche::Corriger("vous".into()) });
    assert_eq!(textes(&banc, id)[0], "Bonsoir à vous.");
}

#[test]
fn une_correction_porte_sur_des_mots_entiers() {
    let (mut banc, id) = reunion_a_verifier();

    banc.executer(Commande::CorrigerReplique { reunion: id, replique: 0, texte: "Bonjou à tous.".into() });
    banc.executer(Commande::CorrigerReplique { reunion: id, replique: 0, texte: "Bonjou, à tous.".into() });
    banc.executer(Commande::CorrigerReplique { reunion: id, replique: 0, texte: "Bonjou, a tous.".into() });

    let c: Vec<(&str, &str)> = banc.reunion(id).corrections_a_la_main.iter().map(|c| (c.avant.as_str(), c.apres.as_str())).collect();
    assert_eq!(c, [("Bonjour", "Bonjou"), ("Bonjou", "Bonjou,"), ("à", "a")]);
}

#[test]
fn une_correction_sans_changement_n_enregistre_rien_et_une_vide_est_refusee() {
    let (mut banc, id) = reunion_a_verifier();
    banc.executer(Commande::CorrigerReplique { reunion: id, replique: 1, texte: "Salut Alexandre.".into() });
    assert!(banc.reunion(id).corrections_a_la_main.is_empty());
    assert!(banc.greffier.executer(Commande::CorrigerReplique { reunion: id, replique: 1, texte: " ".into() }).is_err());
    assert!(banc.greffier.executer(Commande::CorrigerReplique { reunion: id, replique: 9, texte: "Oui.".into() }).is_err());
}

#[test]
fn un_passage_a_verifier_se_tranche_dans_son_onglet_pas_en_corrigeant_la_replique() {
    let (mut banc, id) = reunion_a_verifier();
    assert!(banc.greffier.executer(Commande::CorrigerReplique { reunion: id, replique: 1, texte: "Salut Alexandrine.".into() }).is_err());
    assert_eq!(textes(&banc, id)[1], "Salut Alexandre.");
}

#[test]
fn les_corrections_a_la_main_survivent_a_une_relecture_refaite() {
    let (mut banc, id) = reunion_a_verifier();
    banc.executer(Commande::CorrigerReplique { reunion: id, replique: 0, texte: "Bonsoir à tous.".into() });

    banc.executer(Commande::Refaire { reunion: id, etape: greffier_coeur::Etape::Relecture });
    banc.avancer_jusqu_a(id, |s| *s == Statut::Terminee);

    assert_eq!(textes(&banc, id)[0], "Bonsoir à tous.");
    assert_eq!(banc.reunion(id).corrections_a_la_main.len(), 1);
}

#[test]
fn pendant_la_relecture_la_transcription_ne_se_corrige_pas() {
    let (mut banc, id) = reunion_a_verifier();
    banc.executer(Commande::Refaire { reunion: id, etape: greffier_coeur::Etape::Relecture });

    assert!(banc.greffier.executer(Commande::CorrigerReplique { reunion: id, replique: 0, texte: "Bonsoir à tous.".into() }).is_err());
    assert!(banc.greffier.executer(Commande::Trancher { reunion: id, passage: 0, tranche: Tranche::GarderOriginal }).is_err());
}

#[test]
fn ecouter_un_passage_lit_sa_replique_a_1_5_s_pres_comme_la_reecoute() {
    let (banc, id) = reunion_a_verifier();
    let r = banc.reunion(id);
    // « Salut Alexandre. » va de 3,0 à 4,0 s ; « Bonjour à tous. » commence à 0.
    assert_eq!(r.extrait_a_ecouter(1), Some((1.5, 5.5)));
    assert_eq!(r.extrait_a_ecouter(0), Some((0.0, 3.0)));
    assert_eq!(r.extrait_a_ecouter(2), None);
}
