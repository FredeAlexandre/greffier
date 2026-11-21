//! Nommer et fusionner les locuteurs : les propositions du bilan, confirmer ou corriger un nom,
//! fusionner deux locuteurs qui sont la même voix (et défaire la fusion), et les fichiers qui en
//! résultent (fiche, transcription lisible, transcription structurée).

mod banc;

use banc::{Banc, mot, tour};
use greffier_coeur::adaptateurs::{Adaptateurs, Participant, ResultatDeRelecture, ResultatDuBilan};
use greffier_coeur::faux::{FauxAgent, FauxSeparateur, FauxTranscripteur};
use greffier_coeur::{Commande, IdReunion, Role, Statut};

/// Trois répliques : SPEAKER_00 salue, SPEAKER_01 répond, SPEAKER_02 (en fait la voix de
/// SPEAKER_00) relance. L'agent du bilan reconnaît Alexandre et Khalid, voit que SPEAKER_02 est
/// sans doute Alexandre, et ne sait pas qui est SPEAKER_03, qui ne dit qu'un mot.
fn adaptateurs() -> Adaptateurs {
    let mots = vec![
        mot(0.0, 0.5, "Bonjour"),
        mot(0.5, 0.8, "à"),
        mot(0.8, 1.5, "tous."),
        mot(3.0, 3.4, "Salut"),
        mot(3.4, 4.0, "Alexandre."),
        mot(6.0, 6.5, "On"),
        mot(6.5, 7.0, "commence ?"),
        mot(9.0, 10.0, "Oui."),
    ];
    let tours = vec![tour(0.0, 1.6, "SPEAKER_00"), tour(2.9, 4.1, "SPEAKER_01"), tour(5.9, 7.1, "SPEAKER_02"), tour(8.9, 10.1, "SPEAKER_03")];
    let participants = vec![
        Participant { locuteur: "SPEAKER_00".into(), nom_propose: "Alexandre".into(), justification: "Khalid l'appelle Alexandre à 0:00:03.".into(), meme_voix_que: None },
        Participant { locuteur: "SPEAKER_01".into(), nom_propose: "Khalid".into(), justification: "Répond à Alexandre à 0:00:03.".into(), meme_voix_que: None },
        Participant { locuteur: "SPEAKER_02".into(), nom_propose: "Alexandre".into(), justification: "Même voix qu'Alexandre, qui relance à 0:00:06.".into(), meme_voix_que: Some("SPEAKER_00".into()) },
        Participant { locuteur: "SPEAKER_03".into(), nom_propose: String::new(), justification: "Ne dit que « Oui » à 0:00:09 ; personne ne le nomme.".into(), meme_voix_que: None },
    ];
    let mut a = banc::adaptateurs();
    a.transcripteur = Box::new(FauxTranscripteur::fixe(banc::DUREE_D_ETAPE, mots));
    a.separateur = Box::new(FauxSeparateur::fixe(banc::DUREE_D_ETAPE, tours));
    a.agent = Box::new(FauxAgent::new(
        banc::DUREE_D_ETAPE,
        |_| Ok(ResultatDeRelecture::default()),
        banc::DUREE_D_ETAPE,
        move |_| Ok((banc::BILAN.to_string(), ResultatDuBilan { participants: participants.clone(), ..banc::resultat_du_bilan() })),
    ));
    a
}

fn reunion_terminee() -> (Banc, IdReunion) {
    let mut banc = Banc::avec(adaptateurs());
    let id = banc.reunion_a_traiter();
    banc.executer(Commande::Traiter(id));
    banc.avancer_jusqu_a(id, |s| *s == Statut::Terminee);
    (banc, id)
}

fn fichier(banc: &Banc, id: IdReunion, role: Role) -> String {
    let nom = banc.reunion(id).nom_du_dossier.clone();
    banc.lire(&banc.greffier.dossier().fichier(&nom, role))
}

/// Une table du frontmatter de la fiche : la ligne `cle:` et ses lignes en retrait.
fn bloc(fiche: &str, cle: &str) -> String {
    let mut lignes = fiche.lines().skip_while(|l| *l != format!("{cle}:"));
    let premiere = lignes.next().unwrap_or_else(|| panic!("pas de {cle} dans la fiche :\n{fiche}"));
    std::iter::once(premiere).chain(lignes.take_while(|l| l.starts_with("  "))).map(|l| format!("{l}\n")).collect()
}

#[test]
fn le_bilan_propose_un_nom_et_une_justification_pour_chaque_locuteur() {
    let (banc, id) = reunion_terminee();
    let r = banc.reunion(id);
    let vus: Vec<(String, Option<String>, String)> = r.locuteurs.iter().map(|l| (l.etiquette.clone(), l.proposition.clone(), l.justification.clone())).collect();
    assert_eq!(
        vus,
        [
            ("SPEAKER_00".into(), Some("Alexandre".into()), "Khalid l'appelle Alexandre à 0:00:03.".into()),
            ("SPEAKER_01".into(), Some("Khalid".into()), "Répond à Alexandre à 0:00:03.".into()),
            ("SPEAKER_02".into(), Some("Alexandre".into()), "Même voix qu'Alexandre, qui relance à 0:00:06.".into()),
            ("SPEAKER_03".into(), None, "Ne dit que « Oui » à 0:00:09 ; personne ne le nomme.".into()),
        ]
    );
    assert_eq!(r.locuteurs[2].fusion_proposee, Some(0));
    assert!(r.locuteurs.iter().all(|l| l.nom.is_none()), "une proposition n'est pas un nom confirmé");

    let fiche = fichier(&banc, id, Role::Fiche);
    assert_eq!(
        bloc(&fiche, "propositions"),
        "propositions:\n  SPEAKER_00:\n    nom: \"Alexandre\"\n    justification: \"Khalid l'appelle Alexandre à 0:00:03.\"\n  SPEAKER_01:\n    nom: \"Khalid\"\n    justification: \"Répond à Alexandre à 0:00:03.\"\n  SPEAKER_02:\n    nom: \"Alexandre\"\n    justification: \"Même voix qu'Alexandre, qui relance à 0:00:06.\"\n    meme_voix_que: SPEAKER_00\n  SPEAKER_03:\n    nom: \"\"\n    justification: \"Ne dit que « Oui » à 0:00:09 ; personne ne le nomme.\"\n",
    );
}

/// Les lignes de répliques de la transcription lisible.
fn repliques_lisibles(md: &str) -> Vec<String> {
    md.lines().filter(|l| l.starts_with('[')).map(String::from).collect()
}

fn json(banc: &Banc, id: IdReunion) -> serde_json::Value {
    serde_json::from_str(&fichier(banc, id, Role::TranscriptionStructuree)).expect("transcription structurée")
}

#[test]
fn confirmer_puis_corriger_un_nom_l_ecrit_dans_la_fiche_et_la_transcription_lisible() {
    let (mut banc, id) = reunion_terminee();

    banc.executer(Commande::NommerLocuteur { reunion: id, locuteur: 0, nom: "Alexandre".into() });
    banc.executer(Commande::NommerLocuteur { reunion: id, locuteur: 1, nom: "Khalid".into() });
    assert_eq!(bloc(&fichier(&banc, id, Role::Fiche), "locuteurs"), "locuteurs:\n  SPEAKER_00: \"Alexandre\"\n  SPEAKER_01: \"Khalid\"\n");

    // Corriger : un autre nom que celui proposé, espaces autour retirés.
    banc.executer(Commande::NommerLocuteur { reunion: id, locuteur: 1, nom: "  Jérôme ".into() });
    let r = banc.reunion(id);
    assert_eq!(r.locuteurs[1].nom.as_deref(), Some("Jérôme"));
    assert_eq!(r.locuteurs[1].affiche(), "Jérôme");
    assert_eq!(bloc(&fichier(&banc, id, Role::Fiche), "locuteurs"), "locuteurs:\n  SPEAKER_00: \"Alexandre\"\n  SPEAKER_01: \"Jérôme\"\n");
    assert_eq!(
        repliques_lisibles(&fichier(&banc, id, Role::TranscriptionLisible)),
        ["[0:00:00] Alexandre : Bonjour à tous.", "[0:00:03] Jérôme : Salut Alexandre.", "[0:00:06] SPEAKER_02 : On commence ?", "[0:00:09] SPEAKER_03 : Oui."]
    );

    // La transcription structurée garde les étiquettes, et donne la correspondance.
    let j = json(&banc, id);
    assert_eq!(j["repliques"][1]["locuteur"], "SPEAKER_01");
    assert_eq!(j["mots"][3]["locuteur"], "SPEAKER_01");
    assert_eq!(j["noms"], serde_json::json!({ "SPEAKER_00": "Alexandre", "SPEAKER_01": "Jérôme" }));

    let refus = banc.greffier.executer(Commande::NommerLocuteur { reunion: id, locuteur: 1, nom: "  ".into() });
    assert!(refus.is_err(), "un nom vide");
    assert!(banc.greffier.executer(Commande::NommerLocuteur { reunion: id, locuteur: 9, nom: "Odile".into() }).is_err(), "locuteur inconnu");
}

#[test]
fn fusionner_deux_locuteurs_les_reunit_partout() {
    let (mut banc, id) = reunion_terminee();
    banc.executer(Commande::NommerLocuteur { reunion: id, locuteur: 0, nom: "Alexandre".into() });
    let (alexandre, speaker_02) = (banc.reunion(id).temps_de_parole(0), banc.reunion(id).temps_de_parole(2));
    assert!(alexandre > 0.0 && speaker_02 > 0.0);

    banc.executer(Commande::FusionnerLocuteurs { reunion: id, locuteur: 2, dans: 0 });
    let r = banc.reunion(id);
    // Timeline et liste des locuteurs : une piste de moins, la réplique de SPEAKER_02 va à Alexandre.
    assert_eq!(r.locuteurs_actifs().collect::<Vec<_>>(), [0, 1, 3]);
    let affiches: Vec<usize> = r.repliques.iter().map(|x| r.locuteur_affiche(x.locuteur)).collect();
    assert_eq!(affiches, [0, 1, 0, 3]);
    // Temps de parole : celui de SPEAKER_02 s'ajoute à celui d'Alexandre.
    assert!((r.temps_de_parole(0) - (alexandre + speaker_02)).abs() < 1e-9, "le temps de parole de SPEAKER_02 va à Alexandre");
    let total: f64 = r.locuteurs_actifs().map(|i| r.temps_de_parole(i)).sum();
    assert!((total - 1.0).abs() < 1e-9, "les parts des locuteurs restants font 100 % ({total})");
    assert_eq!(r.locuteurs[2].fusion_proposee, None, "la fusion proposée est faite");

    assert_eq!(
        repliques_lisibles(&fichier(&banc, id, Role::TranscriptionLisible)),
        ["[0:00:00] Alexandre : Bonjour à tous.", "[0:00:03] SPEAKER_01 : Salut Alexandre.", "[0:00:06] Alexandre : On commence ?", "[0:00:09] SPEAKER_03 : Oui."]
    );
    let fiche = fichier(&banc, id, Role::Fiche);
    assert_eq!(bloc(&fiche, "locuteurs"), "locuteurs:\n  SPEAKER_00: \"Alexandre\"\n  SPEAKER_02: \"Alexandre\"\n", "les noms, fusions comprises");
    assert_eq!(bloc(&fiche, "fusions"), "fusions:\n  SPEAKER_02: SPEAKER_00\n");
    let j = json(&banc, id);
    assert_eq!(j["repliques"][2]["locuteur"], "SPEAKER_02", "la transcription structurée garde l'étiquette d'origine");
    assert_eq!(j["locuteurs"], serde_json::json!(["SPEAKER_00", "SPEAKER_01", "SPEAKER_02", "SPEAKER_03"]));
    assert_eq!(j["fusions"], serde_json::json!({ "SPEAKER_02": "SPEAKER_00" }));
    assert_eq!(j["noms"], serde_json::json!({ "SPEAKER_00": "Alexandre", "SPEAKER_02": "Alexandre" }));
}

#[test]
fn un_locuteur_nomme_fusionne_dans_un_locuteur_sans_nom_lui_donne_son_nom() {
    let (mut banc, id) = reunion_terminee();
    banc.executer(Commande::NommerLocuteur { reunion: id, locuteur: 0, nom: "Alexandre".into() });
    banc.executer(Commande::FusionnerLocuteurs { reunion: id, locuteur: 0, dans: 2 });
    let r = banc.reunion(id);
    assert_eq!(r.locuteurs_actifs().collect::<Vec<_>>(), [1, 2, 3]);
    assert_eq!(r.locuteurs[2].nom.as_deref(), Some("Alexandre"));
    assert_eq!(r.locuteurs[0].nom, None, "le nom va à la voix qui reste");
    assert_eq!(repliques_lisibles(&fichier(&banc, id, Role::TranscriptionLisible))[0], "[0:00:00] Alexandre : Bonjour à tous.");

    // Nommer un locuteur fusionné nomme celui qui le remplace.
    banc.executer(Commande::NommerLocuteur { reunion: id, locuteur: 0, nom: "Alex".into() });
    assert_eq!(banc.reunion(id).locuteurs[2].nom.as_deref(), Some("Alex"));
    assert_eq!(bloc(&fichier(&banc, id, Role::Fiche), "locuteurs"), "locuteurs:\n  SPEAKER_00: \"Alex\"\n  SPEAKER_02: \"Alex\"\n");
}

#[test]
fn une_fusion_ne_fait_jamais_de_boucle() {
    let (mut banc, id) = reunion_terminee();
    banc.executer(Commande::FusionnerLocuteurs { reunion: id, locuteur: 1, dans: 2 });
    banc.executer(Commande::FusionnerLocuteurs { reunion: id, locuteur: 3, dans: 1 });
    let r = banc.reunion(id);
    assert_eq!(r.locuteurs_actifs().collect::<Vec<_>>(), [0, 2]);
    assert_eq!(r.locuteur_affiche(3), 2, "fusionné dans un locuteur déjà fusionné : va au bout");

    for (locuteur, dans) in [(1, 3), (2, 3), (3, 3), (1, 9)] {
        let refus = banc.greffier.executer(Commande::FusionnerLocuteurs { reunion: id, locuteur, dans });
        assert!(refus.is_err(), "{locuteur} dans {dans} : déjà la même voix ou inconnu");
    }
    let r = banc.reunion(id);
    assert_eq!(r.repliques.iter().map(|x| r.locuteur_affiche(x.locuteur)).collect::<Vec<_>>(), [0, 2, 2, 2]);
}

/// Nomme Alexandre et Khalid, fusionne SPEAKER_02 dans Alexandre.
fn reunion_nommee_et_fusionnee() -> (Banc, IdReunion) {
    let (mut banc, id) = reunion_terminee();
    banc.executer(Commande::NommerLocuteur { reunion: id, locuteur: 0, nom: "Alexandre".into() });
    banc.executer(Commande::NommerLocuteur { reunion: id, locuteur: 1, nom: "Khalid".into() });
    banc.executer(Commande::FusionnerLocuteurs { reunion: id, locuteur: 2, dans: 0 });
    (banc, id)
}

#[test]
fn noms_fusions_et_propositions_se_retrouvent_au_lancement_suivant() {
    let (mut banc, id) = reunion_nommee_et_fusionnee();
    let avant = banc.reunion(id).locuteurs.clone();
    let nom = banc.reunion(id).nom_du_dossier.clone();

    banc.rouvrir();
    assert_eq!(banc.reunion_nommee(&nom).locuteurs, avant);
}

#[test]
fn sans_fiche_la_transcription_structuree_suffit_a_retrouver_noms_et_fusions() {
    let (mut banc, id) = reunion_nommee_et_fusionnee();
    let nom = banc.reunion(id).nom_du_dossier.clone();
    std::fs::remove_file(banc.greffier.dossier().fichier(&nom, Role::Fiche)).unwrap();

    banc.rouvrir();
    let r = banc.reunion_nommee(&nom);
    let noms: Vec<Option<&str>> = r.locuteurs.iter().map(|l| l.nom.as_deref()).collect();
    assert_eq!(noms, [Some("Alexandre"), Some("Khalid"), None, None]);
    assert_eq!(r.locuteurs_actifs().collect::<Vec<_>>(), [0, 1, 3]);
    assert_eq!(r.nom_du_locuteur(2), Some("Alexandre"));
}

#[test]
fn defaire_une_fusion_rend_au_locuteur_ses_repliques() {
    let (mut banc, id) = reunion_nommee_et_fusionnee();
    banc.executer(Commande::SeparerLocuteur { reunion: id, locuteur: 2 });
    let r = banc.reunion(id);
    assert_eq!(r.locuteurs_actifs().collect::<Vec<_>>(), [0, 1, 2, 3]);
    assert_eq!(r.locuteurs[2].nom, None);
    assert_eq!(r.locuteurs[2].proposition.as_deref(), Some("Alexandre"), "la proposition de l'agent reste");
    assert_eq!(repliques_lisibles(&fichier(&banc, id, Role::TranscriptionLisible))[2], "[0:00:06] SPEAKER_02 : On commence ?");
    assert!(!fichier(&banc, id, Role::Fiche).contains("fusions:"));

    assert!(banc.greffier.executer(Commande::SeparerLocuteur { reunion: id, locuteur: 2 }).is_err(), "il n'est plus fusionné");
}
