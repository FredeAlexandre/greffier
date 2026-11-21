//! Les fichiers font foi : ce qui change dans le dossier de réunions hors de l'app (un bilan
//! retouché dans un éditeur, une transcription corrigée par un agent, un dossier supprimé dans le
//! Finder) se voit dans l'app dès que le cœur relit le dossier, sans relancer.

mod banc;

use std::path::PathBuf;

use banc::Banc;
use greffier_coeur::{Commande, Evenement, Role, Statut};

const RELEASE: &str = "2026-11-20 10h00 - Revue de release";

fn fichier(banc: &Banc, nom: &str, role: Role) -> PathBuf {
    banc.greffier.dossier().fichier(nom, role)
}

#[test]
fn un_bilan_retouche_hors_de_l_app_est_recharge() {
    let mut banc = Banc::sur_les_fixtures();
    let id = banc.reunion_nommee(RELEASE).id;
    let chemin = fichier(&banc, RELEASE, Role::Bilan);
    let md = banc.lire(&chemin).replace("### Support (0:00:05)", "### Support client (0:00:07)");
    std::fs::write(&chemin, md).unwrap();

    let evenements = banc.greffier.relire_le_dossier();

    assert_eq!(evenements, [Evenement::ReunionRelue(id)]);
    let r = banc.reunion(id);
    assert!(r.bilan.contains("### Support client (0:00:07)"));
    let sujets: Vec<(f64, &str)> = r.sujets.iter().map(|s| (s.debut, s.titre.as_str())).collect();
    assert_eq!(sujets, [(0.0, "Date de la release"), (7.0, "Support client")]);
}

#[test]
fn une_fiche_modifiee_hors_de_l_app_est_rechargee() {
    let mut banc = Banc::sur_les_fixtures();
    let id = banc.reunion_nommee(RELEASE).id;
    let chemin = fichier(&banc, RELEASE, Role::Fiche);
    let md = banc.lire(&chemin).replace("SPEAKER_00: \"Alexandre\"", "SPEAKER_00: \"Alex\"\n  SPEAKER_01: \"Khalid\"");
    std::fs::write(&chemin, md).unwrap();

    assert_eq!(banc.greffier.relire_le_dossier(), [Evenement::ReunionRelue(id)]);
    let r = banc.reunion(id);
    let noms: Vec<String> = r.locuteurs.iter().map(|l| l.affiche()).collect();
    assert_eq!(noms, ["Alex", "Khalid", "SPEAKER_02"]);
}

#[test]
fn une_transcription_corrigee_hors_de_l_app_est_rechargee() {
    let mut banc = Banc::sur_les_fixtures();
    let id = banc.reunion_nommee(RELEASE).id;
    let chemin = fichier(&banc, RELEASE, Role::TranscriptionStructuree);
    let json = banc.lire(&chemin).replace("Je préviens le support demain.", "Je préviens le support ce soir.");
    std::fs::write(&chemin, json).unwrap();

    assert_eq!(banc.greffier.relire_le_dossier(), [Evenement::ReunionRelue(id)]);
    assert_eq!(banc.reunion(id).repliques[2].texte, "Je préviens le support ce soir.");
}

fn noms(banc: &Banc) -> Vec<&str> {
    banc.greffier.reunions().iter().map(|r| r.nom_du_dossier.as_str()).collect()
}

#[test]
fn un_dossier_supprime_dans_le_finder_fait_disparaitre_la_reunion() {
    let mut banc = Banc::sur_les_fixtures();
    let id = banc.reunion_nommee(RELEASE).id;
    std::fs::remove_dir_all(banc.dossier.join(RELEASE)).unwrap();

    assert_eq!(banc.greffier.relire_le_dossier(), [Evenement::ReunionDisparue(id)]);
    assert!(banc.greffier.reunion(id).is_none());
    assert_eq!(noms(&banc), ["2026-11-19 15h30 - Réunion", "2026-11-18 09h15 - Réunion"]);
}

#[test]
fn une_reunion_en_attente_dont_le_dossier_disparait_sort_de_la_file() {
    let mut banc = Banc::sur_les_fixtures();
    let id = banc.reunion_nommee("2026-11-19 15h30 - Réunion").id;
    let autre = banc.reunion_nommee("2026-11-18 09h15 - Réunion").id;
    banc.executer(Commande::Traiter(autre));
    banc.executer(Commande::Traiter(id));
    assert_eq!(banc.greffier.rang(id), Some(1));
    std::fs::remove_dir_all(banc.dossier.join("2026-11-19 15h30 - Réunion")).unwrap();

    assert_eq!(banc.greffier.relire_le_dossier(), [Evenement::ReunionDisparue(id)]);
    assert_eq!(banc.greffier.rang(id), None);
    banc.avancer_jusqu_a(autre, |s| *s == Statut::Terminee);
}

#[test]
fn un_dossier_ajoute_dans_le_finder_fait_apparaitre_la_reunion_a_sa_place() {
    let mut banc = Banc::sur_les_fixtures();
    let nom = "2026-11-19 18h00 - Rétro";
    std::fs::create_dir_all(banc.dossier.join(nom)).unwrap();
    std::fs::copy(fichier(&banc, RELEASE, Role::Bilan), fichier(&banc, nom, Role::Bilan)).unwrap();

    let evenements = banc.greffier.relire_le_dossier();

    let r = banc.reunion_nommee(nom);
    assert_eq!(evenements, [Evenement::ReunionApparue(r.id)]);
    assert_eq!(r.titre, "Rétro");
    assert_eq!(r.statut, Statut::Terminee);
    assert!(banc.greffier.reunions().iter().filter(|x| x.id == r.id).count() == 1, "un identifiant neuf");
    assert_eq!(noms(&banc), [RELEASE, nom, "2026-11-19 15h30 - Réunion", "2026-11-18 09h15 - Réunion"]);
    assert_eq!(banc.greffier.relire_le_dossier(), [], "une seule fois");
}

#[test]
fn un_dossier_qui_ne_suit_pas_la_regle_de_nommage_est_ignore() {
    let mut banc = Banc::sur_les_fixtures();
    std::fs::create_dir_all(banc.dossier.join("Brouillons")).unwrap();
    assert_eq!(banc.greffier.relire_le_dossier(), []);
}

#[test]
fn une_reunion_supprimee_dans_l_app_ne_revient_pas() {
    let mut banc = Banc::sur_les_fixtures();
    let id = banc.reunion_nommee(RELEASE).id;
    banc.executer(Commande::Supprimer(id));
    assert_eq!(banc.greffier.relire_le_dossier(), []);
    assert!(banc.greffier.reunion(id).is_none());
}

#[test]
fn le_dossier_de_l_enregistrement_en_cours_n_apparait_pas_comme_une_reunion() {
    let mut banc = Banc::new();
    banc.executer(Commande::DemarrerEnregistrement);
    banc.avancer(2.0);
    assert_eq!(banc.greffier.relire_le_dossier(), []);
    assert!(banc.greffier.reunions().is_empty());
    banc.enregistrer_la_suite();
    assert_eq!(banc.greffier.relire_le_dossier(), [], "l'enregistrement arrêté est déjà connu");
    assert_eq!(banc.greffier.reunions().len(), 1);
}

#[test]
fn les_ecritures_de_greffier_ne_rechargent_rien() {
    let mut banc = Banc::new();
    let id = banc.enregistrer(3.0);
    // Pendant tout le traitement (l'agent écrit le bilan), puis après.
    for _ in 0..60 {
        banc.avancer(0.1);
        assert_eq!(banc.greffier.relire_le_dossier(), [], "pendant {:?}", banc.reunion(id).statut);
    }
    assert_eq!(banc.reunion(id).statut, Statut::Terminee);
    banc.executer(Commande::NommerLocuteur { reunion: id, locuteur: 1, nom: "Khalid".into() });
    banc.executer(Commande::CocherAction { reunion: id, action: 0, faite: true });
    banc.executer(Commande::MarquerLue(id));
    assert_eq!(banc.greffier.relire_le_dossier(), []);
    banc.rouvrir();
    assert_eq!(banc.greffier.relire_le_dossier(), []);
}

#[test]
fn une_modification_pendant_le_traitement_est_relue_apres() {
    let mut banc = Banc::sur_les_fixtures();
    let id = banc.reunion_nommee(RELEASE).id;
    banc.executer(Commande::Refaire { reunion: id, etape: greffier_coeur::Etape::Bilan });
    banc.avancer(0.2);
    let chemin = fichier(&banc, RELEASE, Role::TranscriptionLisible);
    std::fs::write(&chemin, "retouchée").unwrap();
    assert_eq!(banc.greffier.relire_le_dossier(), [], "la réunion en cours de traitement attend");
    banc.avancer_jusqu_a(id, |s| *s == Statut::Terminee);
    // Greffier a réécrit la transcription lisible à la fin de l'étape : plus rien à relire.
    assert_eq!(banc.greffier.relire_le_dossier(), []);
}

#[test]
fn cocher_une_action_ne_change_que_sa_case_dans_le_bilan() {
    let mut banc = Banc::sur_les_fixtures();
    let id = banc.reunion_nommee(RELEASE).id;
    let chemin = fichier(&banc, RELEASE, Role::Bilan);
    // Fins de ligne Windows, pas de saut de ligne final, une case hors des actions, un [X].
    let avant = banc.lire(&chemin).trim_end().replace("- Aucune.", "- [X] Déjà tranchée").replace('\n', "\r\n");
    std::fs::write(&chemin, &avant).unwrap();
    banc.greffier.relire_le_dossier();

    banc.executer(Commande::CocherAction { reunion: id, action: 0, faite: true });
    let apres = banc.lire(&chemin);
    let differences: Vec<usize> = (0..avant.len()).filter(|&i| avant.as_bytes()[i] != apres.as_bytes()[i]).collect();
    assert_eq!(apres.len(), avant.len());
    assert_eq!(differences.len(), 1);
    assert!(apres.contains("- [x] **Alexandre** — publier la release — lundi\r\n"));
    assert_eq!(banc.reunion(id).bilan, apres, "le bilan de l'app est celui du fichier");

    banc.executer(Commande::CocherAction { reunion: id, action: 1, faite: false });
    banc.executer(Commande::CocherAction { reunion: id, action: 2, faite: true });
    let apres = banc.lire(&chemin);
    assert!(apres.contains("- [ ] **Khalid** — lancer la recette — vendredi\r\n"));
    assert!(apres.ends_with("- [X] Déjà tranchée"), "une case déjà cochée reste telle quelle");
    assert_eq!(banc.greffier.relire_le_dossier(), []);
}

#[test]
fn cocher_une_action_part_du_bilan_tel_qu_il_est_sur_le_disque() {
    let mut banc = Banc::sur_les_fixtures();
    let id = banc.reunion_nommee(RELEASE).id;
    let chemin = fichier(&banc, RELEASE, Role::Bilan);
    let md = banc.lire(&chemin).replace("## Questions ouvertes", "- [ ] Ajoutée dans l'éditeur\n\n## Questions ouvertes");
    std::fs::write(&chemin, md).unwrap();

    banc.executer(Commande::CocherAction { reunion: id, action: 0, faite: true });
    let r = banc.reunion(id);
    assert!(r.bilan.contains("- [ ] Ajoutée dans l'éditeur"));
    assert!(banc.lire(&chemin).contains("- [x] **Alexandre**"));
}
