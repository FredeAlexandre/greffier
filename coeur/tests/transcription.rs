//! L'étape Transcription : le vocabulaire du lexique donné au transcripteur, les bruits entre
//! crochets retirés, un seul locuteur tant que la séparation n'est pas branchée, le découpage en
//! répliques, les fichiers écrits, la file (rang, avancement, temps restant), l'arrêt, l'échec et
//! le nouvel essai. Les noms sont inventés.

mod banc;

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use banc::{Banc, mot};
use greffier_coeur::adaptateurs::{Adaptateurs, UnSeulLocuteur};
use greffier_coeur::dossier::Import;
use greffier_coeur::faux::{FauxSeparateur, FauxTranscripteur};
use greffier_coeur::{Commande, DateHeure, Etape, Evenement, IdReunion, Mot, Reglages, Role, Statut};

/// La vidéo des fixtures (12 s).
fn video() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/dossier-de-reunions/2026-11-19 15h30 - Réunion/2026-11-19 15h30 - Réunion - enregistrement.mp4")
}

/// Les adaptateurs du banc, avec ce transcripteur et sans séparation des locuteurs.
fn avec_transcripteur(t: FauxTranscripteur) -> Adaptateurs {
    let mut a = banc::adaptateurs();
    a.transcripteur = Box::new(t);
    a.separateur = Box::new(UnSeulLocuteur);
    a
}

fn importer_un_media(banc: &mut Banc) -> IdReunion {
    let import = Import { enregistrement: video(), transcription: None, bilan: None, debut: DateHeure::new(2026, 10, 2, 9, 30), titre: "Point Ormeau".into() };
    let ev = banc.executer(Commande::Importer(import));
    ev.iter().find_map(|e| if let Evenement::ReunionImportee(id) = e { Some(*id) } else { None }).expect("réunion importée")
}

fn textes(banc: &Banc, id: IdReunion) -> Vec<String> {
    banc.reunion(id).repliques.iter().map(|r| r.texte.clone()).collect()
}

fn fichier(banc: &Banc, id: IdReunion, role: Role) -> String {
    let r = banc.reunion(id);
    banc.lire(&banc.greffier.dossier().fichier(&r.nom_du_dossier, role))
}

fn ecrire_le_lexique(banc: &Banc, vocabulaire: &str) -> PathBuf {
    let app = banc.dossier.with_extension("app");
    std::fs::create_dir_all(&app).expect("dossier de l'app");
    let chemin = app.join("lexique.md");
    let md = format!("# Lexique\n\n## Personnes\n\n- Bérénice Ormeau : cheffe de projet\n\n## Mots justes\n\nLumion\n\n## Erreurs connues\n\n| Entendu | Correct |\n|---|---|\n| lumi on | Lumion |\n\n## Vocabulaire\n\n{vocabulaire}\n");
    std::fs::write(&chemin, md).expect("lexique");
    chemin
}

fn traiter_la_transcription(banc: &mut Banc, id: IdReunion) {
    banc.executer(Commande::Traiter(id));
    banc.avancer_jusqu_a(id, |s| matches!(s, Statut::EnCours { etape: Etape::Relecture, .. } | Statut::Echec { .. } | Statut::Terminee));
}

// --- Vocabulaire ---------------------------------------------------------------------------

#[test]
fn la_section_vocabulaire_du_lexique_est_donnee_au_transcripteur_a_chaque_transcription() {
    let vus = Rc::new(RefCell::new(Vec::<String>::new()));
    let v = vus.clone();
    let mut banc = Banc::avec(avec_transcripteur(FauxTranscripteur::new(1.0, move |d| {
        v.borrow_mut().push(d.vocabulaire.clone());
        Ok(banc::mots())
    })));
    let lexique = ecrire_le_lexique(&banc, "Réunion Ormeau avec Bérénice\net Gaspard.  Projet Lumion.");
    banc.executer(Commande::ChangerReglages(Reglages { lexique: Some(lexique.clone()), ..banc.greffier.reglages().clone() }));
    let id = importer_un_media(&mut banc);

    traiter_la_transcription(&mut banc, id);
    assert_eq!(*vus.borrow(), ["Réunion Ormeau avec Bérénice et Gaspard. Projet Lumion."]);

    // Le lexique est relu : une modification vaut pour la transcription suivante.
    banc.avancer_jusqu_a(id, |s| *s == Statut::Terminee);
    ecrire_le_lexique(&banc, "Réunion Ormeau avec Gaspard.");
    banc.executer(Commande::Refaire { reunion: id, etape: Etape::Transcription });
    banc.avancer_jusqu_a(id, |s| matches!(s, Statut::EnCours { etape: Etape::Relecture, .. }));
    assert_eq!(vus.borrow()[1], "Réunion Ormeau avec Gaspard.");
}

#[test]
fn sans_lexique_le_transcripteur_n_a_pas_de_vocabulaire() {
    let vus = Rc::new(RefCell::new(Vec::<String>::new()));
    let v = vus.clone();
    let mut banc = Banc::avec(avec_transcripteur(FauxTranscripteur::new(1.0, move |d| {
        v.borrow_mut().push(d.vocabulaire.clone());
        Ok(banc::mots())
    })));
    let absent = banc.dossier.with_extension("app").join("lexique.md");
    banc.executer(Commande::ChangerReglages(Reglages { lexique: Some(absent), ..banc.greffier.reglages().clone() }));
    let id = importer_un_media(&mut banc);
    traiter_la_transcription(&mut banc, id);
    assert_eq!(*vus.borrow(), [""]);
}

#[test]
fn le_transcripteur_recoit_le_media_importe_et_sa_duree() {
    let vus = Rc::new(RefCell::new(Vec::new()));
    let v = vus.clone();
    let mut banc = Banc::avec(avec_transcripteur(FauxTranscripteur::new(1.0, move |d| {
        v.borrow_mut().push((d.audio.clone(), d.duree));
        Ok(banc::mots())
    })));
    let id = importer_un_media(&mut banc);
    let nom = banc.reunion(id).nom_du_dossier.clone();
    traiter_la_transcription(&mut banc, id);
    let (audio, duree) = vus.borrow()[0].clone();
    assert_eq!(audio, banc.greffier.dossier().enregistrement(&nom).expect("enregistrement importé"));
    assert!((duree - 12.0).abs() < 0.1, "{duree}");
}

// --- Mots et répliques ---------------------------------------------------------------------

#[test]
fn les_bruits_entre_crochets_et_les_mots_vides_sont_retires() {
    let mots = vec![
        mot(0.0, 1.0, "[Musique]"),
        mot(1.0, 1.4, "Bonjour"),
        mot(1.4, 1.6, "(rires)"),
        mot(1.6, 1.8, "Gaspard."),
        mot(1.8, 1.9, "  "),
        mot(2.0, 2.2, "[bruit"),
        mot(2.2, 2.4, "de"),
        mot(2.4, 2.6, "fond]"),
        mot(2.6, 2.9, "On"),
        mot(2.9, 3.3, "commence."),
        // Une parenthèse qui ne se ferme pas dans les 7 mots n'est pas un bruit.
        mot(3.4, 3.6, "(enfin"),
        mot(3.6, 3.8, "si"),
        mot(3.8, 4.0, "tout"),
        mot(4.0, 4.2, "le"),
        mot(4.2, 4.4, "monde"),
        mot(4.4, 4.6, "est"),
        mot(4.6, 4.8, "là."),
        // Pas plus qu'une parenthèse ouverte à la fin.
        mot(4.9, 5.0, "[applaudissements"),
    ];
    let mut banc = Banc::avec(avec_transcripteur(FauxTranscripteur::fixe(1.0, mots)));
    let id = importer_un_media(&mut banc);
    traiter_la_transcription(&mut banc, id);

    let r = banc.reunion(id);
    let gardes: Vec<&str> = r.mots.iter().map(|m| m.texte.as_str()).collect();
    assert_eq!(gardes, ["Bonjour", "Gaspard.", "On", "commence.", "(enfin", "si", "tout", "le", "monde", "est", "là.", "[applaudissements"]);
    assert_eq!(textes(&banc, id), ["Bonjour Gaspard. On commence. (enfin si tout le monde est là. [applaudissements"]);
}

#[test]
fn sans_separation_des_locuteurs_tous_les_mots_vont_a_un_seul_locuteur() {
    let mots = vec![mot(0.0, 0.5, "Bonjour"), mot(0.5, 1.0, "Bérénice."), mot(1.2, 1.6, "Salut"), mot(1.6, 2.0, "Gaspard.")];
    let mut banc = Banc::avec(avec_transcripteur(FauxTranscripteur::fixe(1.0, mots)));
    let id = importer_un_media(&mut banc);
    traiter_la_transcription(&mut banc, id);

    let r = banc.reunion(id);
    let etiquettes: Vec<&str> = r.locuteurs.iter().map(|l| l.etiquette.as_str()).collect();
    assert_eq!(etiquettes, ["SPEAKER_00"]);
    assert!(r.repliques.iter().all(|x| x.locuteur == 0));
    assert!(r.tours.is_empty());
}

#[test]
fn une_replique_commence_apres_une_pause_de_2_s() {
    let mots = vec![
        mot(0.0, 0.5, "On"),
        mot(0.5, 1.0, "démarre."),
        mot(2.9, 3.2, "Lumion"), // 1,9 s de pause : même réplique
        mot(3.2, 3.5, "avance."),
        mot(5.5, 6.0, "Ensuite"), // 2 s de pause : nouvelle réplique
        mot(6.0, 6.5, "Ormeau."),
    ];
    let mut banc = Banc::avec(avec_transcripteur(FauxTranscripteur::fixe(1.0, mots)));
    let id = importer_un_media(&mut banc);
    traiter_la_transcription(&mut banc, id);

    assert_eq!(textes(&banc, id), ["On démarre. Lumion avance.", "Ensuite Ormeau."]);
    let r = banc.reunion(id);
    assert_eq!((r.repliques[0].debut, r.repliques[0].fin), (0.0, 3.5));
    assert_eq!((r.repliques[1].debut, r.repliques[1].fin), (5.5, 6.5));
}

#[test]
fn une_replique_s_arrete_a_la_premiere_fin_de_phrase_passe_220_caracteres() {
    // « Bérénice » + 25 × « explique » = 233 caractères, sans fin de phrase.
    let mut mots = vec![mot(0.0, 0.2, "Bérénice")];
    let mut t = 0.2;
    let mut dire = |texte: &str, mots: &mut Vec<Mot>| {
        mots.push(mot(t, t + 0.2, texte));
        t += 0.2;
    };
    for _ in 0..25 {
        dire("explique", &mut mots);
    }
    dire("tout.", &mut mots); // fin de phrase après 220 caractères : la réplique s'arrête après
    dire("Gaspard", &mut mots);
    dire("répond.", &mut mots); // fin de phrase, mais réplique courte : on continue
    dire("Puis", &mut mots);
    dire("Lumion.", &mut mots);
    let mut banc = Banc::avec(avec_transcripteur(FauxTranscripteur::fixe(1.0, mots)));
    let id = importer_un_media(&mut banc);
    traiter_la_transcription(&mut banc, id);

    let t = textes(&banc, id);
    assert_eq!(t.len(), 2, "{t:?}");
    assert!(t[0].starts_with("Bérénice explique") && t[0].ends_with("explique tout."), "{}", t[0]);
    assert_eq!(t[1], "Gaspard répond. Puis Lumion.");
}

#[test]
fn une_ponctuation_seule_reste_dans_la_replique_precedente() {
    let mots = vec![mot(0.0, 0.4, "Ça"), mot(0.4, 0.8, "va"), mot(0.8, 0.9, "?"), mot(1.5, 1.9, "Oui.")];
    let tours = vec![banc::tour(0.0, 0.85, "SPEAKER_00"), banc::tour(0.85, 2.0, "SPEAKER_01")];
    let mut a = avec_transcripteur(FauxTranscripteur::fixe(1.0, mots));
    a.separateur = Box::new(FauxSeparateur::fixe(1.0, tours));
    let mut banc = Banc::avec(a);
    let id = importer_un_media(&mut banc);
    traiter_la_transcription(&mut banc, id);
    assert_eq!(textes(&banc, id), ["Ça va ?", "Oui."]);
}

// --- Fichiers ------------------------------------------------------------------------------

#[test]
fn un_media_importe_seul_produit_les_deux_transcriptions_et_la_fiche_dit_ou_il_en_est() {
    let mots = vec![Mot { debut: 0.5, fin: 0.9, texte: "Bonjour".into(), confiance: 0.873 }, mot(0.9, 1.4, "Bérénice."), mot(4.0, 4.5, "Lumion"), mot(4.5, 5.0, "démarre.")];
    let mut banc = Banc::avec(avec_transcripteur(FauxTranscripteur::fixe(1.0, mots)));
    let id = importer_un_media(&mut banc);
    traiter_la_transcription(&mut banc, id);

    let json: serde_json::Value = serde_json::from_str(&fichier(&banc, id, Role::TranscriptionStructuree)).expect("JSON");
    assert_eq!(json["locuteurs"], serde_json::json!(["SPEAKER_00"]));
    let premier = &json["mots"][0];
    assert_eq!((premier["debut"].as_f64(), premier["fin"].as_f64(), premier["texte"].as_str()), (Some(0.5), Some(0.9), Some("Bonjour")));
    assert!((premier["confiance"].as_f64().unwrap_or(0.0) - 0.873).abs() < 1e-6, "{premier}");
    assert_eq!(premier["locuteur"], "SPEAKER_00");
    assert_eq!(json["mots"].as_array().map(Vec::len), Some(4));
    assert_eq!(json["repliques"][0]["texte"], "Bonjour Bérénice.");
    assert_eq!(json["repliques"][1]["texte"], "Lumion démarre.");
    assert!(json.get("relecture").is_none(), "pas encore relue");

    let md = fichier(&banc, id, Role::TranscriptionLisible);
    assert!(md.contains("[0:00:00] SPEAKER_00 : Bonjour Bérénice."), "{md}");
    assert!(md.contains("[0:00:04] SPEAKER_00 : Lumion démarre."), "{md}");

    let fiche = fichier(&banc, id, Role::Fiche);
    assert!(fiche.contains("statut: en_cours\n"), "{fiche}");
    assert!(fiche.contains("etape: relecture\n"), "{fiche}");
    assert!(fiche.contains("etape_faite: transcription\n"), "{fiche}");
}

#[test]
fn pendant_la_transcription_la_fiche_dit_transcription_en_cours() {
    let mut banc = Banc::avec(avec_transcripteur(FauxTranscripteur::fixe(3.0, banc::mots())));
    let id = importer_un_media(&mut banc);
    banc.executer(Commande::Traiter(id));
    banc.avancer(1.0);
    let fiche = fichier(&banc, id, Role::Fiche);
    assert!(fiche.contains("statut: en_cours\netape: transcription\n"), "{fiche}");
}

// --- File, arrêt, échec --------------------------------------------------------------------

#[test]
fn l_avancement_de_la_transcription_est_celui_de_la_moitie_la_plus_lente() {
    let mut a = avec_transcripteur(FauxTranscripteur::fixe(4.0, banc::mots()));
    a.separateur = Box::new(FauxSeparateur::fixe(1.0, banc::tours()));
    let mut banc = Banc::avec(a);
    let id = importer_un_media(&mut banc);
    banc.executer(Commande::Traiter(id));
    banc.avancer(2.0);
    let p = banc.reunion(id).statut.progres();
    assert!((p - 0.5).abs() < 0.05, "{p}");
}

#[test]
fn le_temps_restant_compte_les_reunions_qui_passent_avant() {
    let mut banc = Banc::avec(avec_transcripteur(FauxTranscripteur::fixe(1.0, banc::mots())));
    let premiere = importer_un_media(&mut banc);
    let seconde = importer_un_media(&mut banc);
    banc.executer(Commande::Traiter(premiere));
    banc.executer(Commande::Traiter(seconde));
    banc.avancer(0.1);

    assert_eq!((banc.greffier.rang(premiere), banc.greffier.rang(seconde)), (Some(0), Some(1)));
    let propre = |id| banc.reunion(id).temps_restant_estime().expect("dans la file");
    let (a, b) = (banc.greffier.temps_restant(premiere).expect("en cours"), banc.greffier.temps_restant(seconde).expect("en attente"));
    assert_eq!(a, propre(premiere));
    assert!((b - (a + propre(seconde))).abs() < 1e-9, "{b} ≠ {a} + {}", propre(seconde));
}

#[test]
fn une_reunion_en_attente_d_une_etape_avancee_a_moins_a_attendre() {
    let mut banc = Banc::new();
    let faite = banc.reunion_a_traiter();
    banc.executer(Commande::Traiter(faite));
    banc.avancer_jusqu_a(faite, |s| *s == Statut::Terminee);
    let neuve = banc.reunion_a_traiter();
    // Pendant un enregistrement, la file attend : les deux restent en attente.
    banc.executer(Commande::DemarrerEnregistrement);
    banc.executer(Commande::Refaire { reunion: faite, etape: Etape::Bilan });
    banc.executer(Commande::Traiter(neuve));
    assert_eq!(banc.reunion(faite).statut, Statut::EnAttente(Etape::Bilan));
    assert_eq!(banc.reunion(neuve).statut, Statut::EnAttente(Etape::Transcription));

    let bilan_seul = banc.reunion(faite).temps_restant_estime().expect("en attente");
    let tout = banc.reunion(neuve).temps_restant_estime().expect("en attente");
    assert!(bilan_seul < tout / 2.0, "{bilan_seul} / {tout}");
}

#[test]
fn arreter_la_transcription_arrete_le_transcripteur_et_la_remet_a_traiter() {
    let mut banc = Banc::avec(avec_transcripteur(FauxTranscripteur::fixe(3.0, banc::mots())));
    let id = importer_un_media(&mut banc);
    banc.executer(Commande::Traiter(id));
    banc.avancer(1.0);
    banc.executer(Commande::ArreterTraitement(id));

    assert_eq!(banc.reunion(id).statut, Statut::ATraiter);
    assert_eq!(banc.reunion(id).etape_faite, None);
    assert!(!banc.greffier.occupe());
    let fiche = fichier(&banc, id, Role::Fiche);
    assert!(fiche.contains("statut: a_traiter\n"), "{fiche}");
    assert!(!banc.greffier.dossier().roles_presents(&banc.reunion(id).nom_du_dossier).contains(&Role::TranscriptionStructuree));
}

#[test]
fn une_transcription_en_echec_garde_son_message_et_traiter_la_reessaie() {
    let essais = Rc::new(RefCell::new(0));
    let n = essais.clone();
    let mut banc = Banc::avec(avec_transcripteur(FauxTranscripteur::new(1.0, move |_| {
        *n.borrow_mut() += 1;
        if *n.borrow() == 1 { Err("Le média n'a pas de son.".into()) } else { Ok(vec![mot(0.0, 0.5, "Lumion"), mot(0.5, 1.0, "redémarre.")]) }
    })));
    let id = importer_un_media(&mut banc);
    banc.executer(Commande::Traiter(id));
    banc.avancer_jusqu_a(id, |s| matches!(s, Statut::Echec { .. }));

    assert_eq!(banc.reunion(id).statut, Statut::Echec { etape: Etape::Transcription, message: "Le média n'a pas de son.".into() });
    let fiche = fichier(&banc, id, Role::Fiche);
    assert!(fiche.contains("statut: echec\netape: transcription\nerreur: \"Le média n'a pas de son.\"\n"), "{fiche}");

    banc.executer(Commande::Traiter(id));
    assert_eq!(banc.reunion(id).statut, Statut::EnAttente(Etape::Transcription));
    banc.avancer_jusqu_a(id, |s| matches!(s, Statut::EnCours { etape: Etape::Relecture, .. }));
    assert_eq!(*essais.borrow(), 2);
    assert_eq!(banc.reunion(id).etape_faite, Some(Etape::Transcription));
    assert_eq!(textes(&banc, id), ["Lumion redémarre."]);
}
