//! La recherche ⌘K : un mot d'un titre, d'un bilan ou d'une transcription, dans toutes les
//! réunions, et l'instant où ouvrir la réunion (la réplique, ou le sujet d'une ligne du bilan).

mod banc;

use banc::Banc;
use greffier_coeur::{Commande, Role};
use greffier_coeur::recherche::{Endroit, Resultat};

const RELEASE: &str = "2026-11-20 10h00 - Revue de release";

/// Ce qu'on regarde d'un résultat : où, à quel instant, et le passage surligné.
fn resume(r: &Resultat) -> (Endroit, Option<f64>, &str) {
    (r.endroit, r.instant, &r.extrait[r.surligne.0..r.surligne.1])
}

#[test]
fn une_ligne_du_bilan_ouvre_la_reunion_a_son_sujet() {
    let banc = Banc::sur_les_fixtures();
    let id = banc.reunion_nommee(RELEASE).id;

    let resultats = banc.greffier.rechercher("prévient");

    assert_eq!(resultats.len(), 1);
    assert_eq!(resultats[0].reunion, id);
    assert_eq!(resume(&resultats[0]), (Endroit::Bilan, Some(5.0), "prévient"));
    assert_eq!(resultats[0].extrait, "Alexandre prévient le support.");
}

#[test]
fn une_decision_ouvre_la_reunion_a_son_propre_instant() {
    let banc = Banc::sur_les_fixtures();

    let resultats = banc.greffier.rechercher("vendredi");

    let trouves: Vec<_> = resultats.iter().map(resume).collect();
    assert_eq!(
        trouves,
        [
            // En bref et Actions n'ont pas d'instant : la réunion s'ouvre au début.
            (Endroit::Bilan, None, "vendredi"),
            (Endroit::Bilan, Some(2.0), "vendredi"),
            (Endroit::Bilan, None, "vendredi"),
            (Endroit::Transcription, Some(2.0), "vendredi"),
        ]
    );
    assert_eq!(resultats[1].extrait, "(0:00:02) Release lundi si la recette passe vendredi.");
    assert_eq!(resultats[2].extrait, "Khalid — lancer la recette — vendredi");
}

#[test]
fn une_replique_ouvre_la_reunion_a_son_debut_avec_son_locuteur() {
    let banc = Banc::sur_les_fixtures();
    let id = banc.reunion_nommee("2026-11-18 09h15 - Réunion").id;

    let resultats = banc.greffier.rechercher("mesures");

    assert_eq!(resultats.len(), 1);
    assert_eq!(resultats[0].reunion, id);
    assert_eq!(resume(&resultats[0]), (Endroit::Transcription, Some(6.5), "mesures"));
    assert_eq!(resultats[0].extrait, "Il reste deux mesures à refaire.");
    assert_eq!(resultats[0].locuteur.as_deref(), Some("SPEAKER_00"));
}

#[test]
fn le_titre_est_trouve_sans_instant() {
    let banc = Banc::sur_les_fixtures();

    let resultats = banc.greffier.rechercher("revue");

    assert_eq!(resultats.iter().map(resume).collect::<Vec<_>>(), [(Endroit::Titre, None, "Revue")]);
    assert_eq!(resultats[0].extrait, "Revue de release");
}

#[test]
fn la_casse_et_les_accents_ne_comptent_pas() {
    let banc = Banc::sur_les_fixtures();

    let resultats = banc.greffier.rechercher("  PREVIENS le Support ");

    assert_eq!(resultats.iter().map(resume).collect::<Vec<_>>(), [(Endroit::Transcription, Some(5.0), "préviens le support")]);
    assert!(banc.greffier.rechercher("").is_empty());
    assert!(banc.greffier.rechercher("kangourou").is_empty());
}

#[test]
fn la_recherche_suit_les_changements_faits_dans_l_app() {
    let mut banc = Banc::sur_les_fixtures();
    let id = banc.reunion_nommee("2026-11-18 09h15 - Réunion").id;

    banc.executer(Commande::NommerLocuteur { reunion: id, locuteur: 0, nom: "Odile".into() });

    assert_eq!(banc.greffier.rechercher("mesures")[0].locuteur.as_deref(), Some("Odile"));
}

#[test]
fn la_recherche_suit_un_bilan_retouche_hors_de_l_app() {
    let mut banc = Banc::sur_les_fixtures();
    let chemin = banc.greffier.dossier().fichier(RELEASE, Role::Bilan);
    let md = banc.lire(&chemin).replace("- Alexandre prévient le support.", "- Alexandre prévient le support.\n- Le kangourou du support est en congé.");
    std::fs::write(&chemin, md).unwrap();

    banc.greffier.relire_le_dossier();

    assert_eq!(banc.greffier.rechercher("kangourou").iter().map(resume).collect::<Vec<_>>(), [(Endroit::Bilan, Some(5.0), "kangourou")]);
}

// --- Le cache dans le dossier de l'app -------------------------------------------------------

fn fichier_du_cache(cache: &std::path::Path, nom: &str) -> std::path::PathBuf {
    cache.join(format!("{nom}.json"))
}

#[test]
fn le_cache_se_reconstruit_s_il_est_absent() {
    let mut banc = Banc::sur_les_fixtures();
    let cache = banc.utiliser_un_cache_de_recherche();
    assert!(fichier_du_cache(&cache, RELEASE).is_file());
    assert!(fichier_du_cache(&cache, "2026-11-18 09h15 - Réunion").is_file());

    std::fs::remove_dir_all(&cache).unwrap();
    banc.rouvrir();
    banc.utiliser_un_cache_de_recherche();

    assert!(banc.lire(&fichier_du_cache(&cache, RELEASE)).contains("Alexandre prévient le support."));
    assert_eq!(banc.greffier.rechercher("prévient").len(), 1);
}

#[test]
fn le_cache_suit_les_changements_du_dossier_de_reunions() {
    let mut banc = Banc::sur_les_fixtures();
    let cache = banc.utiliser_un_cache_de_recherche();
    let chemin = banc.greffier.dossier().fichier(RELEASE, Role::Bilan);
    let md = banc.lire(&chemin).replace("- Alexandre prévient le support.", "- Le kangourou du support est en congé.");
    std::fs::write(&chemin, md).unwrap();

    banc.greffier.relire_le_dossier();

    let json = banc.lire(&fichier_du_cache(&cache, RELEASE));
    assert!(json.contains("kangourou") && !json.contains("prévient"));

    // Un dossier supprimé dans le Finder : son entrée quitte le cache.
    std::fs::remove_dir_all(banc.dossier.join(RELEASE)).unwrap();
    banc.greffier.relire_le_dossier();
    assert!(!fichier_du_cache(&cache, RELEASE).exists());
    assert!(banc.greffier.rechercher("kangourou").is_empty());
}

#[test]
fn un_cache_perime_ou_illisible_est_refait_au_lancement() {
    let mut banc = Banc::sur_les_fixtures();
    let cache = banc.utiliser_un_cache_de_recherche();
    // App fermée : le bilan change, et le cache d'une autre réunion est abîmé.
    let chemin = banc.greffier.dossier().fichier(RELEASE, Role::Bilan);
    let md = banc.lire(&chemin).replace("- Alexandre prévient le support.", "- Le kangourou du support est en congé.");
    std::fs::write(&chemin, md).unwrap();
    std::fs::write(fichier_du_cache(&cache, "2026-11-18 09h15 - Réunion"), "{pas du json").unwrap();

    banc.rouvrir();
    banc.utiliser_un_cache_de_recherche();

    assert_eq!(banc.greffier.rechercher("kangourou").len(), 1);
    assert!(banc.greffier.rechercher("prévient").is_empty());
    assert_eq!(banc.greffier.rechercher("mesures").len(), 1);
    assert!(banc.lire(&fichier_du_cache(&cache, "2026-11-18 09h15 - Réunion")).contains("mesures"));
}

#[test]
fn un_titre_change_suit_dans_la_recherche_et_le_cache() {
    let mut banc = Banc::sur_les_fixtures();
    let cache = banc.utiliser_un_cache_de_recherche();
    let id = banc.reunion_nommee(RELEASE).id;

    banc.executer(Commande::ChangerTitre { reunion: id, titre: "Revue de sortie".into() });

    assert_eq!(banc.greffier.rechercher("sortie").iter().map(resume).collect::<Vec<_>>(), [(Endroit::Titre, None, "sortie")]);
    assert_eq!(banc.greffier.rechercher("prévient")[0].reunion, id);
    assert!(!fichier_du_cache(&cache, RELEASE).exists());
    assert!(fichier_du_cache(&cache, "2026-11-20 10h00 - Revue de sortie").is_file());
}

#[test]
fn un_long_passage_est_coupe_entre_deux_mots_autour_du_mot_trouve() {
    let mut banc = Banc::sur_les_fixtures();
    let chemin = banc.greffier.dossier().fichier(RELEASE, Role::Bilan);
    let longue = "Alexandre rappelle que la recette de vendredi conditionne tout le calendrier de la publication, puis il présente le kangourou mascotte de la nouvelle version, que toute l'équipe trouve réussi même si le support préfère attendre les retours des clients avant de valider.";
    let md = banc.lire(&chemin).replace("- Alexandre prévient le support.", &format!("- {longue}"));
    std::fs::write(&chemin, md).unwrap();
    banc.greffier.relire_le_dossier();

    let r = &banc.greffier.rechercher("kangourou")[0];

    assert_eq!(r.extrait, "…de la publication, puis il présente le kangourou mascotte de la nouvelle version, que toute l'équipe trouve réussi même si le support…");
    assert_eq!(&r.extrait[r.surligne.0..r.surligne.1], "kangourou");
}
