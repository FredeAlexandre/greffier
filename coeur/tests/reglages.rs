//! Les réglages retenus d'un lancement à l'autre (`reglages.json` du dossier de l'app) et le
//! lexique, lus et écrits par le cœur : l'interface ne touche pas à ces fichiers.

mod banc;

use std::path::PathBuf;

use banc::Banc;
use greffier_coeur::adaptateurs::Sources;
use greffier_coeur::reglages::{self, RACCOURCI_PAR_DEFAUT, ReglagesRetenus};
use greffier_coeur::{Commande, Reglages, lexique};

/// Un dossier de l'app temporaire, effacé à la fin.
struct DossierDeLApp(PathBuf);

impl DossierDeLApp {
    fn new(nom: &str) -> DossierDeLApp {
        let d = std::env::temp_dir().join(format!("greffier-app-{nom}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        DossierDeLApp(d)
    }
    fn reglages_json(&self) -> serde_json::Value {
        serde_json::from_str(&std::fs::read_to_string(self.0.join("reglages.json")).expect("reglages.json")).expect("JSON")
    }
}

impl Drop for DossierDeLApp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn sans_fichier_les_reglages_sont_ceux_par_defaut() {
    let app = DossierDeLApp::new("defauts");
    let r = ReglagesRetenus::lire(&app.0);
    assert_eq!(r, ReglagesRetenus::default());
    assert_eq!(r.dossier_de_reunions, None);
    assert_eq!(r.raccourci, RACCOURCI_PAR_DEFAUT);
    assert!(r.traiter_a_l_arret);
    assert_eq!(r.sources, Sources::default());
    assert!(!app.0.exists(), "lire n'écrit rien");
}

#[test]
fn les_reglages_retenus_se_retrouvent_au_lancement_suivant() {
    let app = DossierDeLApp::new("aller-retour");
    let r = ReglagesRetenus {
        dossier_de_reunions: Some(PathBuf::from("/Users/odile/Réunions")),
        raccourci: "cmd+alt+e".into(),
        traiter_a_l_arret: false,
        sources: Sources { ecran: false, son_du_mac: true, micro: true },
        dossiers_de_contexte: vec![PathBuf::from("/Users/odile/Notes")],
    };
    r.retenir(&app.0).expect("écrit");
    assert_eq!(ReglagesRetenus::lire(&app.0), r);
    let json = app.reglages_json();
    assert_eq!(json["dossier_de_reunions"], "/Users/odile/Réunions");
    assert_eq!(json["sources"]["ecran"], false);
}

#[test]
fn les_cles_inconnues_du_fichier_sont_gardees() {
    let app = DossierDeLApp::new("inconnues");
    std::fs::create_dir_all(&app.0).unwrap();
    std::fs::write(app.0.join("reglages.json"), r#"{"futur": {"a": 1}, "traiter_a_l_arret": false}"#).unwrap();
    let mut r = ReglagesRetenus::lire(&app.0);
    assert!(!r.traiter_a_l_arret);
    r.raccourci = String::new();
    r.retenir(&app.0).unwrap();
    let json = app.reglages_json();
    assert_eq!(json["futur"]["a"], 1);
    assert_eq!(json["raccourci"], "");
    assert_eq!(ReglagesRetenus::lire(&app.0).raccourci, "", "vide : aucun raccourci");
}

#[test]
fn un_fichier_illisible_donne_les_reglages_par_defaut() {
    let app = DossierDeLApp::new("illisible");
    std::fs::create_dir_all(&app.0).unwrap();
    std::fs::write(app.0.join("reglages.json"), "pas du JSON").unwrap();
    assert_eq!(ReglagesRetenus::lire(&app.0), ReglagesRetenus::default());
}

#[test]
fn choisir_le_dossier_de_reunions_le_cree_et_le_retient() {
    let app = DossierDeLApp::new("choix");
    let dossier = app.0.with_extension("reunions");
    let _ = std::fs::remove_dir_all(&dossier);
    reglages::choisir_dossier_de_reunions(&app.0, &dossier).expect("choisi");
    assert!(dossier.is_dir());
    assert_eq!(ReglagesRetenus::lire(&app.0).dossier_de_reunions, Some(dossier.clone()));
    let _ = std::fs::remove_dir_all(&dossier);
}

#[test]
fn les_reglages_retenus_s_appliquent_aux_reglages_du_coeur_sans_toucher_lexique_ni_cache() {
    let r = ReglagesRetenus { traiter_a_l_arret: false, dossiers_de_contexte: vec![PathBuf::from("/Users/odile/Notes")], ..ReglagesRetenus::default() };
    let avant = Reglages { lexique: Some(PathBuf::from("/app/lexique.md")), cache_de_recherche: Some(PathBuf::from("/app/recherche")), ..Reglages::default() };
    let apres = r.appliquer(avant.clone());
    assert_eq!(apres, Reglages { traiter_a_l_arret: false, dossiers_de_contexte: vec![PathBuf::from("/Users/odile/Notes")], ..avant });
}

#[test]
fn le_coeur_lit_le_lexique_et_ses_personnes() {
    let mut banc = Banc::new();
    // Sans lexique : le lexique livré.
    assert_eq!(banc.greffier.lexique(), lexique::apercu(lexique::LEXIQUE_INITIAL));
    assert_eq!(banc.greffier.personnes(), lexique::personnes(lexique::LEXIQUE_INITIAL));

    let chemin = banc.dossier.with_extension("lexique.md");
    std::fs::write(&chemin, "# Lexique\n\n## Personnes\n\n- Odile : cheffe de projet\n- Bastien\n\n## Mots justes\n\nZorglub, Ormeau.\n\n## Erreurs connues\n\n| Entendu | Correct |\n|---|---|\n| Zorglube | Zorglub |\n").unwrap();
    banc.executer(Commande::ChangerReglages(Reglages { lexique: Some(chemin.clone()), ..banc.greffier.reglages().clone() }));
    let a = banc.greffier.lexique();
    assert_eq!((a.personnes, a.mots_justes, a.erreurs_connues), (2, 2, 1));
    assert_eq!(banc.greffier.personnes(), ["Odile", "Bastien"]);
    let _ = std::fs::remove_file(&chemin);
}

#[test]
fn le_dossier_de_l_app_est_dans_la_bibliotheque_de_l_utilisateur() {
    assert_eq!(reglages::dossier_de_l_app(), reglages::maison().join("Library/Application Support/Greffier"));
    assert_eq!(reglages::dossier_propose(), reglages::maison().join("Documents/Réunions"));
}
