//! La lecture du dossier de réunions : chaque sous-dossier `AAAA-MM-JJ HHhMM - Titre` donne une
//! réunion, avec ce qu'il contient (fiche, bilan, transcription structurée, enregistrement), même
//! incomplet. Les réunions d'exemple sont dans `tests/fixtures/dossier-de-reunions`.

mod banc;

use banc::Banc;
use greffier_coeur::faux::FaussesRessources;
use greffier_coeur::{Commande, DateHeure, Etape, IdReunion, Repere, Reunion, Role, Statut, StatutDuPassage};

#[test]
fn une_reunion_avec_seulement_son_enregistrement_est_a_traiter() {
    let mut banc = Banc::sur_les_fixtures();
    let r = banc.reunion_nommee("2026-11-19 15h30 - Réunion");
    assert_eq!(r.debut, DateHeure::new(2026, 11, 19, 15, 30));
    assert_eq!(r.titre, "Réunion");
    assert_eq!(r.duree, 12.0, "la durée est lue dans l'enregistrement");
    assert_eq!(r.statut, Statut::ATraiter);
    assert_eq!(r.etape_faite, None);
    assert_eq!(r.fichiers, [Role::Enregistrement]);
    assert!(r.repliques.is_empty() && r.bilan.is_empty() && r.locuteurs.is_empty());

    let id = r.id;
    banc.executer(Commande::Traiter(id));
    assert_eq!(banc.reunion(id).statut, Statut::EnAttente(Etape::Transcription));
}

#[test]
fn une_reunion_complete_est_lue_depuis_sa_fiche_son_bilan_et_sa_transcription() {
    let banc = Banc::sur_les_fixtures();
    let r = banc.reunion_nommee("2026-11-20 10h00 - Revue de release");
    assert_eq!(r.titre, "Revue de release");
    assert_eq!(r.debut, DateHeure::new(2026, 11, 20, 10, 0));
    assert_eq!(r.duree, 12.0);
    assert_eq!(r.statut, Statut::Terminee);
    assert_eq!(r.etape_faite, Some(Etape::Bilan));
    assert_eq!(r.nombre_de_locuteurs, Some(3));
    assert_eq!(r.fichiers, Role::TOUS);
    assert!(!r.nouvelle);

    // Fiche : noms, propositions, fusions, repères, sessions, résumé et actions principales.
    let locuteurs: Vec<(&str, String)> = r.locuteurs.iter().map(|l| (l.etiquette.as_str(), l.affiche())).collect();
    assert_eq!(locuteurs, [("SPEAKER_00", "Alexandre".into()), ("SPEAKER_01", "Khalid ?".into()), ("SPEAKER_02", "SPEAKER_02".into())]);
    assert_eq!(r.locuteurs[1].justification, "Alexandre l'appelle Khalid à 0:00:00.");
    assert_eq!(r.locuteurs[2].fusionne_dans, Some(0));
    assert_eq!(r.reperes, [Repere { instant: 5.0, libelle: "Le support".into() }]);
    assert_eq!(r.sessions.relecture.as_deref(), Some("sess-relecture"));
    assert_eq!(r.sessions.bilan.as_deref(), Some("sess-bilan"));
    assert_eq!(r.resume, "On publie la release lundi si la recette passe vendredi.");
    assert_eq!(r.actions_principales, ["Alexandre — publier la release — lundi", "Khalid — lancer la recette — vendredi"]);

    // Transcription structurée : mots, tours, répliques, relecture, passages à vérifier.
    assert_eq!(r.mots.len(), 19);
    assert_eq!(r.mots[11].texte, "recète");
    assert_eq!(r.mots[11].confiance, 0.42);
    assert_eq!(r.tours.len(), 3);
    let repliques: Vec<(usize, f64, f64, &str)> = r.repliques.iter().map(|x| (r.locuteur_affiche(x.locuteur), x.debut, x.fin, x.texte.as_str())).collect();
    assert_eq!(
        repliques,
        [
            (0, 0.0, 2.0, "Bonjour Khalid, on publie la release lundi ?"),
            (1, 2.0, 3.5, "Oui, si la recette passe vendredi."),
            (0, 5.0, 6.25, "Je préviens le support demain."),
        ]
    );
    assert_eq!(r.corrections, 1);
    assert_eq!(r.passages.len(), 1);
    assert_eq!(r.passages[0].hypothese.as_deref(), Some("l'équipe support"));
    assert_eq!(r.passages[0].statut, StatutDuPassage::EnAttente);
    assert_eq!(r.repliques[2].passage, Some(0));

    // Bilan : rendu tel quel, sujets et décisions pour la timeline.
    assert!(r.bilan.starts_with("## En bref\n\nOn publie la release lundi"));
    let sujets: Vec<(f64, &str)> = r.sujets.iter().map(|s| (s.debut, s.titre.as_str())).collect();
    assert_eq!(sujets, [(0.0, "Date de la release"), (5.0, "Support")]);
    assert_eq!(r.decisions.len(), 1);
    assert_eq!(r.decisions[0].instant, 2.0);
}

#[test]
fn une_reunion_avec_son_enregistrement_et_sa_transcription_reprend_a_la_relecture() {
    let mut banc = Banc::sur_les_fixtures();
    let r = banc.reunion_nommee("2026-11-18 09h15 - Réunion");
    assert_eq!(r.statut, Statut::ATraiter);
    assert_eq!(r.etape_faite, Some(Etape::Transcription), "une transcription sans journal de relecture n'est pas relue");
    assert_eq!(r.duree, 12.0);
    assert_eq!(r.fichiers, [Role::TranscriptionStructuree, Role::Enregistrement]);
    let locuteurs: Vec<&str> = r.locuteurs.iter().map(|l| l.etiquette.as_str()).collect();
    assert_eq!(locuteurs, ["SPEAKER_00"]);
    let textes: Vec<&str> = r.repliques.iter().map(|x| x.texte.as_str()).collect();
    assert_eq!(textes, ["On commence par le point sur le banc de test.", "Il reste deux mesures à refaire."]);
    assert!(r.mots.is_empty() && r.passages.is_empty() && r.relecture.is_none() && r.bilan.is_empty());

    let id = r.id;
    banc.executer(Commande::Traiter(id));
    assert_eq!(banc.reunion(id).statut, Statut::EnAttente(Etape::Relecture));
}

#[test]
fn seuls_les_dossiers_de_reunion_comptent_les_plus_recents_d_abord() {
    let banc = Banc::sur_les_fixtures();
    let noms: Vec<&str> = banc.greffier.reunions().iter().map(|r| r.nom_du_dossier.as_str()).collect();
    assert_eq!(noms, ["2026-11-20 10h00 - Revue de release", "2026-11-19 15h30 - Réunion", "2026-11-18 09h15 - Réunion"]);
}

/// Enregistre 5 s (2 locuteurs indiqués, un repère à 2 s), traite jusqu'au bout, nomme
/// SPEAKER_00 et coche la 2e action.
fn reunion_traitee_et_retouchee(banc: &mut Banc) -> IdReunion {
    banc.executer(Commande::DemarrerEnregistrement);
    banc.executer(Commande::NombreDeLocuteursEnregistres(Some(2)));
    banc.avancer(2.0);
    banc.executer(Commande::PoserRepere);
    banc.avancer(3.0);
    let id = banc.enregistrer_la_suite();
    banc.avancer_jusqu_a(id, |s| *s == Statut::Terminee);
    banc.executer(Commande::NommerLocuteur { reunion: id, locuteur: 0, nom: "Alexandre".into() });
    banc.executer(Commande::CocherAction { reunion: id, action: 1, faite: true });
    id
}

#[test]
fn une_reunion_traitee_se_retrouve_a_l_identique_au_lancement_suivant() {
    let mut banc = Banc::new();
    let id = reunion_traitee_et_retouchee(&mut banc);
    let avant = banc.reunion(id).clone();
    assert_eq!(avant.fichiers, Role::TOUS);

    banc.rouvrir();
    let apres = banc.reunion_nommee(&avant.nom_du_dossier).clone();
    assert_eq!(Reunion { id: avant.id, ..apres }, avant);
}

#[test]
fn la_fiche_donne_les_metadonnees_le_resume_et_les_liens() {
    let mut banc = Banc::new();
    let id = reunion_traitee_et_retouchee(&mut banc);
    let nom = banc.reunion(id).nom_du_dossier.clone();
    let fiche = banc.lire(&banc.greffier.dossier().fichier(&nom, Role::Fiche));
    assert_eq!(
        fiche,
        r#"---
titre: "Point d'équipe : démo"
date: 2026-11-21
debut: "14:02"
duree: "0:00:05"
statut: terminee
etape_faite: bilan
nombre_de_locuteurs: 2
locuteurs:
  SPEAKER_00: "Alexandre"
propositions:
  SPEAKER_00:
    nom: "Alexandre"
    justification: "Khalid l'appelle Alexandre à 0:00:03."
  SPEAKER_01:
    nom: "Khalid"
    justification: "Répond à Alexandre."
reperes:
  - instant: "0:00:02"
    libelle: "Repère 1"
sessions:
  bilan: "session-bilan"
nouvelle: true
---

# Point d'équipe : démo

Alexandre salue l'équipe.

## Actions principales

- Alexandre — préparer la démo — lundi
- Khalid — relire la PR

## Fichiers

- [Bilan](<2026-11-21 14h02 - Point d'équipe - démo - bilan.md>)
- [Transcription](<2026-11-21 14h02 - Point d'équipe - démo - transcription.md>)
- [Transcription structurée](<2026-11-21 14h02 - Point d'équipe - démo - transcription.json>)
- [Enregistrement](<2026-11-21 14h02 - Point d'équipe - démo - enregistrement.mp4>)
"#
    );
}

#[test]
fn la_transcription_lisible_donne_les_noms_connus() {
    let mut banc = Banc::new();
    let id = reunion_traitee_et_retouchee(&mut banc);
    let nom = banc.reunion(id).nom_du_dossier.clone();
    let md = banc.lire(&banc.greffier.dossier().fichier(&nom, Role::TranscriptionLisible));
    assert_eq!(md, "# Point d'équipe : démo — transcription\n\nRéunion du 2026-11-21 à 14h02, 0:00:05.\n\n[0:00:00] Alexandre : Bonjour à tous.\n\n[0:00:03] SPEAKER_01 : Salut Alexandre.\n");
}

#[test]
fn un_enregistrement_coupe_par_l_arret_de_l_app_est_relu_interrompu() {
    let mut banc = Banc::new();
    banc.executer(Commande::DemarrerEnregistrement);
    banc.avancer(3.0);
    banc.rouvrir();

    let r = banc.reunion_nommee("2026-11-21 14h02 - Réunion");
    assert_eq!(r.statut, Statut::EnregistrementInterrompu);
    assert!(r.fichiers.contains(&Role::Enregistrement));
    let id = r.id;
    banc.executer(Commande::Traiter(id));
    assert_eq!(banc.reunion(id).statut, Statut::EnAttente(Etape::Transcription));
}

#[test]
fn un_traitement_coupe_par_l_arret_de_l_app_reprend_a_son_etape() {
    let mut banc = Banc::new();
    let id = banc.reunion_a_traiter();
    banc.executer(Commande::Traiter(id));
    banc.avancer_jusqu_a(id, |s| matches!(s, Statut::EnCours { etape: Etape::Relecture, .. }));
    let nom = banc.reunion(id).nom_du_dossier.clone();
    banc.rouvrir();

    let r = banc.reunion_nommee(&nom);
    assert_eq!(r.statut, Statut::EnAttente(Etape::Relecture));
    assert_eq!(r.etape_faite, Some(Etape::Transcription));
    assert_eq!(r.repliques.len(), 2);
    let id = r.id;
    assert_eq!(banc.avancer_jusqu_a(id, |s| *s == Statut::Terminee), [Etape::Relecture, Etape::Bilan]);
}

#[test]
fn une_etape_en_echec_le_reste_au_lancement_suivant() {
    let mut a = banc::adaptateurs();
    a.ressources = Box::new(FaussesRessources::absentes(1.0));
    let mut banc = Banc::avec(a);
    let id = banc.reunion_a_traiter();
    banc.executer(Commande::Traiter(id));
    banc.avancer_jusqu_a(id, |s| matches!(s, Statut::Echec { .. }));
    let avant = banc.reunion(id).statut.clone();
    let nom = banc.reunion(id).nom_du_dossier.clone();
    banc.rouvrir();
    assert_eq!(banc.reunion_nommee(&nom).statut, avant);
}
