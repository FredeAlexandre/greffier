//! Le bilan par l'agent : la fiche reçoit titre, résumé, actions et session ; le titre renomme
//! le dossier et tous ses fichiers, liens compris ; changer le titre dans l'app fait de même.

mod banc;

use banc::Banc;
use greffier_coeur::adaptateurs::{ResultatDeRelecture, ResultatDuBilan};
use greffier_coeur::faux::FauxAgent;
use greffier_coeur::{Commande, Etape, IdReunion, Role, Statut};

/// Un agent dont le bilan porte ce titre.
fn agent_titre(titre: &'static str) -> FauxAgent {
    FauxAgent::new(
        banc::DUREE_D_ETAPE,
        |_| Ok(ResultatDeRelecture::default()),
        banc::DUREE_D_ETAPE,
        move |_| Ok((banc::BILAN.to_string(), ResultatDuBilan { titre: titre.into(), ..banc::resultat_du_bilan() })),
    )
}

fn banc_titre(titre: &'static str) -> Banc {
    let mut a = banc::adaptateurs();
    a.agent = Box::new(agent_titre(titre));
    Banc::avec(a)
}

fn reunion_terminee(banc: &mut Banc) -> IdReunion {
    let id = banc.reunion_a_traiter();
    banc.executer(Commande::Traiter(id));
    banc.avancer_jusqu_a(id, |s| *s == Statut::Terminee);
    id
}

/// Les noms des fichiers d'un dossier, triés.
fn fichiers(banc: &Banc, nom: &str) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(banc.dossier.join(nom)).expect("dossier de la réunion").flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    v.sort();
    v
}

#[test]
fn le_titre_du_bilan_renomme_le_dossier_et_ses_fichiers() {
    let mut banc = banc_titre("Revue : budget/planning ?");
    let id = reunion_terminee(&mut banc);

    let nom = "2026-11-21 14h02 - Revue - budget-planning -";
    assert_eq!(banc.reunion(id).titre, "Revue : budget/planning ?");
    assert_eq!(banc.reunion(id).nom_du_dossier, nom);
    assert!(!banc.dossier.join("2026-11-21 14h02 - Réunion").exists());
    assert_eq!(
        fichiers(&banc, nom),
        [
            format!("{nom} - bilan.md"),
            format!("{nom} - enregistrement.mp4"),
            format!("{nom} - réunion.md"),
            format!("{nom} - transcription.json"),
            format!("{nom} - transcription.md"),
        ]
    );
    assert_eq!(banc.lire(&banc.greffier.dossier().fichier(nom, Role::Bilan)), banc::BILAN);
}

#[test]
fn apres_le_renommage_les_liens_menent_aux_nouveaux_fichiers() {
    // Le bilan de l'agent renvoie vers la fiche par son nom (lien Obsidian).
    let bilan: &'static str = "---\nréunion: \"[[2026-11-21 14h02 - Réunion - réunion]]\"\n---\n\nVoir [la transcription](<2026-11-21 14h02 - Réunion - transcription.md>).\n\n## Sujets abordés\n\n### Budget (0:00:00)\n\n## Décisions\n\n- (0:00:03) On garde le budget.\n\n## Actions\n\n- [ ] Odile — envoyer le devis\n";
    let mut a = banc::adaptateurs();
    a.agent = Box::new(FauxAgent::new(
        banc::DUREE_D_ETAPE,
        |_| Ok(ResultatDeRelecture::default()),
        banc::DUREE_D_ETAPE,
        move |_| Ok((bilan.to_string(), ResultatDuBilan { titre: "Budget".into(), ..banc::resultat_du_bilan() })),
    ));
    let mut banc = Banc::avec(a);
    let id = reunion_terminee(&mut banc);

    let nom = "2026-11-21 14h02 - Budget";
    let dossier = banc.greffier.dossier().clone();
    let fiche = banc.lire(&dossier.fichier(nom, Role::Fiche));
    for role in [Role::Bilan, Role::TranscriptionLisible, Role::TranscriptionStructuree, Role::Enregistrement] {
        let lien = format!("(<{nom} - {}>)", role.suffixe());
        assert!(fiche.contains(&lien), "{lien} absent de la fiche :\n{fiche}");
        assert!(dossier.fichier(nom, role).is_file(), "{role:?}");
    }
    let md = banc.lire(&dossier.fichier(nom, Role::Bilan));
    assert!(md.contains("[[2026-11-21 14h02 - Budget - réunion]]"), "{md}");
    assert!(md.contains("(<2026-11-21 14h02 - Budget - transcription.md>)"), "{md}");
    assert_eq!(banc.reunion(id).bilan, md);

    // Greffier a renommé lui-même : rien à relire, la réunion garde son identité.
    assert_eq!(banc.greffier.relire_le_dossier(), []);
    banc.rouvrir();
    assert_eq!(banc.reunion_nommee(nom).titre, "Budget");
    assert_eq!(banc.reunion_nommee(nom).statut, Statut::Terminee);
}

#[test]
fn changer_le_titre_dans_l_app_renomme_aussi_le_dossier() {
    let mut banc = Banc::new();
    let id = reunion_terminee(&mut banc);
    let ancien = banc.reunion(id).nom_du_dossier.clone();

    banc.executer(Commande::ChangerTitre { reunion: id, titre: "  Rétro * sprint 12  ".into() });

    let nom = "2026-11-21 14h02 - Rétro - sprint 12";
    let r = banc.reunion(id);
    assert_eq!(r.titre, "Rétro * sprint 12");
    assert_eq!(r.nom_du_dossier, nom);
    assert!(!banc.dossier.join(&ancien).exists());
    let fiche = banc.lire(&banc.greffier.dossier().fichier(nom, Role::Fiche));
    assert!(fiche.starts_with("---\ntitre: \"Rétro * sprint 12\"\n"), "{fiche}");
    assert!(fiche.contains(&format!("- [Bilan](<{nom} - bilan.md>)")), "{fiche}");
    assert_eq!(banc.greffier.relire_le_dossier(), []);
}

#[test]
fn un_titre_deja_pris_le_meme_jour_recoit_un_numero() {
    let mut banc = Banc::new();
    let a = banc.reunion_a_traiter();
    let b = banc.reunion_a_traiter();
    banc.executer(Commande::ChangerTitre { reunion: a, titre: "Daily".into() });
    banc.executer(Commande::ChangerTitre { reunion: b, titre: "Daily".into() });
    assert_eq!(banc.reunion(a).nom_du_dossier, "2026-11-21 14h02 - Daily");
    assert_eq!(banc.reunion(b).nom_du_dossier, "2026-11-21 14h02 - Daily (2)");

    // Remettre le même titre ne change rien, même avec son numéro.
    banc.executer(Commande::ChangerTitre { reunion: b, titre: "Daily".into() });
    assert_eq!(banc.reunion(b).nom_du_dossier, "2026-11-21 14h02 - Daily (2)");
    // Une réunion à traiter sans transcription a quand même sa fiche et son enregistrement.
    assert!(banc.greffier.dossier().fichier("2026-11-21 14h02 - Daily (2)", Role::Enregistrement).is_file());
}

#[test]
fn le_titre_ne_change_pas_pendant_une_etape_ni_vide() {
    let mut banc = Banc::new();
    let id = banc.reunion_a_traiter();
    assert!(banc.greffier.executer(Commande::ChangerTitre { reunion: id, titre: "   ".into() }).is_err());

    banc.executer(Commande::Traiter(id));
    banc.avancer(0.2);
    assert!(matches!(banc.reunion(id).statut, Statut::EnCours { etape: Etape::Transcription, .. }));
    assert!(banc.greffier.executer(Commande::ChangerTitre { reunion: id, titre: "Plus tard".into() }).is_err());
    assert_eq!(banc.reunion(id).titre, "Réunion");
}

#[test]
fn l_agent_recoit_les_dossiers_de_contexte_et_les_fichiers_de_la_reunion() {
    let demandes = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let vues = demandes.clone();
    let mut a = banc::adaptateurs();
    a.agent = Box::new(FauxAgent::new(
        banc::DUREE_D_ETAPE,
        |_| Ok(ResultatDeRelecture::default()),
        banc::DUREE_D_ETAPE,
        move |d| {
            // Les fichiers de la réunion existent quand l'agent démarre.
            assert!(d.transcription.is_file() && d.fiche.is_file(), "{d:?}");
            vues.borrow_mut().push(d.clone());
            Ok((banc::BILAN.to_string(), banc::resultat_du_bilan()))
        },
    ));
    let mut banc = Banc::avec(a);
    let contexte = vec![std::path::PathBuf::from("/Users/odile/Notes/Projets"), std::path::PathBuf::from("/Users/odile/Clients")];
    let reglages = greffier_coeur::Reglages { dossiers_de_contexte: contexte.clone(), ..banc.greffier.reglages().clone() };
    banc.executer(Commande::ChangerReglages(reglages));
    reunion_terminee(&mut banc);

    let d = demandes.borrow()[0].clone();
    let nom = "2026-11-21 14h02 - Réunion";
    let dossier = banc.greffier.dossier();
    assert_eq!(d.dossiers_de_contexte, contexte);
    assert_eq!(d.dossier_de_reunions, banc.dossier);
    assert_eq!(d.fichier_du_bilan, dossier.fichier(nom, Role::Bilan));
    assert_eq!(d.fiche, dossier.fichier(nom, Role::Fiche));
    assert_eq!(d.transcription, dossier.fichier(nom, Role::TranscriptionLisible));
}
