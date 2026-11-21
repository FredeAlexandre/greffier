//! La relecture par l'agent : ce que Greffier lui envoie, et comment il applique lui-même son
//! résultat (corrections sûres, changements de locuteur, passages corrigés après réécoute ou à
//! vérifier, lexique enrichi).

mod banc;

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use banc::{Banc, mot, tour};
use greffier_coeur::adaptateurs::{Adaptateurs, ChangementDeLocuteur, CorrectionSure, DemandeDeRelecture, PassageCorrige, PassageSignale, ResultatDeRelecture};
use greffier_coeur::faux::{FauxAgent, FauxSeparateur, FauxTranscripteur};
use greffier_coeur::lexique::AjoutAuLexique;
use greffier_coeur::{Commande, Etape, IdReunion, Reglages, Statut};

const LEXIQUE: &str = "---\ntype: lexique\n---\n\n# Lexique\n\n## Personnes\n\n- Alice : cheffe de projet\n\n## Mots justes\n\nZorglub.\n\n## Erreurs connues\n\n| Entendu | Correct |\n|---|---|\n| Zorglube | Zorglub |\n\n## Vocabulaire\n\nRéunion sur Zorglub.\n";

/// Trois répliques : Alice ouvre, Bob répond, Alice reprend (mais la séparation l'a donnée à Bob).
fn adaptateurs(relire: impl FnMut(&DemandeDeRelecture) -> Result<ResultatDeRelecture, String> + 'static) -> Adaptateurs {
    let mots = vec![
        mot(0.0, 0.5, "Bonjour,"),
        mot(0.5, 1.0, "on"),
        mot(1.0, 1.4, "parle"),
        mot(1.4, 1.8, "de"),
        mot(1.8, 2.5, "Zorglube."),
        mot(5.0, 5.4, "La"),
        mot(5.4, 6.0, "recète"),
        mot(6.0, 6.5, "est"),
        mot(6.5, 7.0, "prête ?"),
        mot(9.2, 9.6, "Pas"),
        mot(9.6, 10.0, "encore,"),
        mot(10.0, 10.4, "jeudi"),
        mot(10.4, 11.0, "peut-être."),
    ];
    let tours = vec![tour(0.0, 2.6, "SPEAKER_00"), tour(4.9, 11.1, "SPEAKER_01")];
    let mut a = banc::adaptateurs();
    a.transcripteur = Box::new(FauxTranscripteur::fixe(banc::DUREE_D_ETAPE, mots));
    a.separateur = Box::new(FauxSeparateur::fixe(banc::DUREE_D_ETAPE, tours));
    a.agent = Box::new(FauxAgent::new(banc::DUREE_D_ETAPE, relire, banc::DUREE_D_ETAPE, |_| Ok((banc::BILAN.to_string(), banc::resultat_du_bilan()))));
    a
}

/// Un lexique dans un fichier temporaire, branché dans les réglages.
fn avec_lexique(banc: &mut Banc, md: &str) -> PathBuf {
    let chemin = banc.dossier.with_extension("lexique.md");
    std::fs::write(&chemin, md).expect("lexique");
    banc.executer(Commande::ChangerReglages(Reglages { lexique: Some(chemin.clone()), ..banc.greffier.reglages().clone() }));
    chemin
}

/// Traite une réunion jusqu'à la fin de la relecture.
fn relue(banc: &mut Banc) -> IdReunion {
    let id = banc.reunion_a_traiter();
    banc.executer(Commande::NombreDeLocuteurs { reunion: id, nombre: Some(2) });
    banc.executer(Commande::Traiter(id));
    banc.avancer_jusqu_a(id, |s| matches!(s, Statut::EnCours { etape: Etape::Bilan, .. } | Statut::Echec { .. } | Statut::Terminee));
    id
}

#[test]
fn l_agent_recoit_les_repliques_le_lexique_et_le_contexte() {
    let recue = Rc::new(RefCell::new(None));
    let r = recue.clone();
    let mut banc = Banc::avec(adaptateurs(move |d| {
        *r.borrow_mut() = Some(d.clone());
        Ok(ResultatDeRelecture::default())
    }));
    avec_lexique(&mut banc, LEXIQUE);
    let id = relue(&mut banc);

    let d = recue.borrow().clone().expect("relecture demandée");
    let textes: Vec<(&str, &str)> = d.repliques.iter().map(|x| (x.locuteur.as_str(), x.texte.as_str())).collect();
    assert_eq!(textes, [("SPEAKER_00", "Bonjour, on parle de Zorglube."), ("SPEAKER_01", "La recète est prête ?"), ("SPEAKER_01", "Pas encore, jeudi peut-être.")]);
    assert_eq!(d.lexique, LEXIQUE);
    assert_eq!(d.vocabulaire, "Réunion sur Zorglub.");
    assert_eq!(d.nombre_de_locuteurs, Some(2));
    assert_eq!(d.dossier_de_reunions, banc.dossier);
    assert_eq!(d.nom_du_dossier, banc.reunion(id).nom_du_dossier);
    assert_eq!(d.debut, banc.reunion(id).debut);
    assert!(d.enregistrement.ends_with("2026-11-21 14h02 - Réunion - enregistrement.mp4"), "{}", d.enregistrement.display());
}

fn correction(replique: usize, avant: &str, apres: &str) -> CorrectionSure {
    CorrectionSure { replique, avant: avant.into(), apres: apres.into() }
}

fn changement(replique: usize, locuteur: &str) -> ChangementDeLocuteur {
    ChangementDeLocuteur { replique, locuteur: locuteur.into() }
}

#[test]
fn les_corrections_sures_et_les_changements_de_locuteur_prouves_sont_appliques() {
    let mut banc = Banc::avec(adaptateurs(|_| {
        Ok(ResultatDeRelecture {
            session: Some("relecture-1".into()),
            corrections: vec![correction(1, "recète", "recette"), correction(0, "", "rien"), correction(0, "Bonjour", "Bonjour"), correction(2, "absent", "x"), correction(9, "La", "Le"), correction(0, "Zorglube", "Zorglub")],
            changements_de_locuteur: vec![changement(2, "SPEAKER_00"), changement(1, "SPEAKER_07"), changement(0, "SPEAKER_00"), changement(5, "SPEAKER_01")],
            ..Default::default()
        })
    }));
    let id = relue(&mut banc);
    let r = banc.reunion(id);

    let textes: Vec<(&str, &str)> = r.repliques.iter().map(|x| (r.locuteurs[x.locuteur].etiquette.as_str(), x.texte.as_str())).collect();
    assert_eq!(textes, [("SPEAKER_00", "Bonjour, on parle de Zorglub."), ("SPEAKER_01", "La recette est prête ?"), ("SPEAKER_00", "Pas encore, jeudi peut-être.")]);
    assert_eq!(r.locuteurs.len(), 2, "un locuteur inconnu n'est pas créé");
    let journal = r.relecture.as_ref().expect("journal de la relecture");
    assert_eq!(journal.corrections, [correction(1, "recète", "recette"), correction(0, "Zorglube", "Zorglub")], "seules les corrections appliquées");
    assert_eq!(journal.changements_de_locuteur, [changement(2, "SPEAKER_00")]);
    assert_eq!(r.corrections, 2);
    let brute: Vec<(usize, &str)> = r.brute.iter().map(|x| (x.locuteur, x.texte.as_str())).collect();
    assert_eq!(brute, [(0, "Bonjour, on parle de Zorglube."), (1, "La recète est prête ?"), (1, "Pas encore, jeudi peut-être.")]);
    assert_eq!(r.sessions.relecture.as_deref(), Some("relecture-1"));
}

fn ajout(entendu: &str, correct: &str) -> AjoutAuLexique {
    AjoutAuLexique { entendu: entendu.into(), correct: correct.into() }
}

/// Une relecture qui corrige un passage après réécoute et en laisse deux à vérifier.
fn relecture_avec_passages(_: &DemandeDeRelecture) -> Result<ResultatDeRelecture, String> {
    Ok(ResultatDeRelecture {
        session: Some("relecture-2".into()),
        corrections: vec![correction(1, "recète", "recette")],
        passages_corriges: vec![
            PassageCorrige { replique: 2, extrait: "jeudi".into(), apres: "mardi".into(), reecoute: "pas encore mardi peut-être".into() },
            // Confirmé tel quel : rien à changer.
            PassageCorrige { replique: 0, extrait: "on parle".into(), apres: "on parle".into(), reecoute: "on parle".into() },
            // L'extrait a disparu sous une correction sûre : il reste à vérifier.
            PassageCorrige { replique: 1, extrait: "recète".into(), apres: "requête".into(), reecoute: "la requête est prête".into() },
        ],
        passages_a_verifier: vec![PassageSignale { replique: 0, extrait: "Zorglube".into(), hypothese: Some("?".into()), reecoute: "on parle de Zorglub".into() }],
        ajouts_au_lexique: vec![ajout("recète", "recette"), ajout("Zorglube", "Zorglub")],
        ..Default::default()
    })
}

#[test]
fn les_passages_douteux_sont_corriges_apres_reecoute_ou_laisses_a_verifier() {
    let mut banc = Banc::avec(adaptateurs(relecture_avec_passages));
    let id = relue(&mut banc);
    let r = banc.reunion(id);

    let textes: Vec<&str> = r.repliques.iter().map(|x| x.texte.as_str()).collect();
    assert_eq!(textes, ["Bonjour, on parle de Zorglube.", "La recette est prête ?", "Pas encore, mardi peut-être."]);
    let journal = r.relecture.as_ref().expect("journal");
    assert_eq!(journal.passages_corriges, [PassageCorrige { replique: 2, extrait: "jeudi".into(), apres: "mardi".into(), reecoute: "pas encore mardi peut-être".into() }]);
    assert_eq!(r.corrections, 2, "une correction sûre et un passage corrigé");

    let passages: Vec<(usize, &str, Option<&str>, &str)> = r.passages.iter().map(|p| (p.replique, p.extrait.as_str(), p.hypothese.as_deref(), p.reecoute.as_str())).collect();
    assert_eq!(passages, [(0, "Zorglube", None, "on parle de Zorglub"), (1, "recète", Some("requête"), "la requête est prête")]);
    assert_eq!(r.passages_en_attente(), 2);
    assert_eq!((r.repliques[0].passage, r.repliques[1].passage, r.repliques[2].passage), (Some(0), Some(1), None));
}

#[test]
fn la_relecture_ecrit_la_brute_le_journal_les_passages_et_la_session() {
    let mut banc = Banc::avec(adaptateurs(relecture_avec_passages));
    let id = relue(&mut banc);
    let nom = banc.reunion(id).nom_du_dossier.clone();
    let fichier = |role| banc.greffier.dossier().fichier(&nom, role);

    let json: serde_json::Value = serde_json::from_str(&banc.lire(&fichier(greffier_coeur::Role::TranscriptionStructuree))).expect("json");
    assert_eq!(json["brute"][1]["texte"], "La recète est prête ?");
    assert_eq!(json["repliques"][1]["texte"], "La recette est prête ?");
    assert_eq!(json["relecture"]["corrections"][0]["apres"], "recette");
    assert_eq!(json["relecture"]["passages_corriges"][0]["apres"], "mardi");
    assert_eq!(json["passages"][1]["extrait"], "recète");
    assert_eq!(json["passages"][1]["hypothese"], "requête");
    assert_eq!(json["passages"][1]["reecoute"], "la requête est prête");
    assert_eq!(json["passages"][1]["statut"], "en_attente");

    let lisible = banc.lire(&fichier(greffier_coeur::Role::TranscriptionLisible));
    assert!(lisible.contains("SPEAKER_01 : La recette est prête ?\n> À vérifier : « recète » (hypothèse : « requête »)\n"), "{lisible}");
    let fiche = banc.lire(&fichier(greffier_coeur::Role::Fiche));
    assert!(fiche.contains("relecture-2"), "{fiche}");

    // Relue au lancement suivant, la réunion est la même.
    let avant = banc.reunion(id).clone();
    banc.rouvrir();
    let r = banc.reunion_nommee(&nom);
    assert_eq!((&r.repliques, &r.brute, &r.passages, &r.relecture, r.corrections), (&avant.repliques, &avant.brute, &avant.passages, &avant.relecture, avant.corrections));
    assert_eq!(r.sessions.relecture.as_deref(), Some("relecture-2"));
}

#[test]
fn le_lexique_s_enrichit_des_erreurs_apprises_qu_il_ne_connait_pas() {
    let mut banc = Banc::avec(adaptateurs(relecture_avec_passages));
    let lexique = avec_lexique(&mut banc, LEXIQUE);
    let id = relue(&mut banc);

    let md = banc.lire(&lexique);
    assert!(md.contains("| Entendu | Correct |\n|---|---|\n| Zorglube | Zorglub |\n| recète | recette |\n\n## Vocabulaire"), "{md}");
    assert_eq!(banc.reunion(id).relecture.as_ref().expect("journal").ajouts_au_lexique, [ajout("recète", "recette")], "seul le nouvel ajout est au journal");
}

#[test]
fn refaire_la_relecture_repart_de_la_transcription_brute() {
    let passe = Rc::new(RefCell::new(0));
    let p = passe.clone();
    let mut banc = Banc::avec(adaptateurs(move |d| {
        *p.borrow_mut() += 1;
        // La seconde relecture voit la transcription brute, pas la première relecture.
        assert_eq!(d.repliques[1].texte, "La recète est prête ?");
        let apres = if *p.borrow() == 1 { "recette" } else { "requête" };
        Ok(ResultatDeRelecture { corrections: vec![correction(1, "recète", apres)], ..Default::default() })
    }));
    let id = relue(&mut banc);
    banc.avancer_jusqu_a(id, |s| *s == Statut::Terminee);
    assert_eq!(banc.reunion(id).repliques[1].texte, "La recette est prête ?");

    banc.executer(Commande::Refaire { reunion: id, etape: Etape::Relecture });
    banc.avancer_jusqu_a(id, |s| *s == Statut::Terminee);
    let r = banc.reunion(id);
    assert_eq!(*passe.borrow(), 2);
    assert_eq!(r.repliques[1].texte, "La requête est prête ?");
    assert_eq!(r.brute[1].texte, "La recète est prête ?");
    assert_eq!(r.corrections, 1);
}

#[test]
fn si_l_agent_echoue_l_etape_est_en_echec_avec_son_message() {
    let mut banc = Banc::avec(adaptateurs(|_| Err("La CLI `claude` est introuvable : installer Claude Code.".into())));
    let id = relue(&mut banc);
    let r = banc.reunion(id);
    assert_eq!(r.statut, Statut::Echec { etape: Etape::Relecture, message: "La CLI `claude` est introuvable : installer Claude Code.".into() });
    assert_eq!(r.repliques[1].texte, "La recète est prête ?", "la transcription reste celle de Whisper");
    assert!(r.relecture.is_none());
}
