//! La relecture par la CLI `claude` (claude::relecture) : la première passe, la réécoute des
//! passages douteux, la deuxième passe dans la même session, et la traduction de leurs réponses
//! en résultat pour le cœur. La CLI et la réécoute sont remplacées par des faux.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;

use greffier_coeur::DateHeure;
use greffier_coeur::adaptateurs::{ChangementDeLocuteur, CorrectionSure, DemandeDeRelecture, PassageCorrige, PassageSignale, RepliquePourLAgent};
use greffier_coeur::claude::relecture::{self, Reecoute};
use greffier_coeur::claude::{self, AppelAClaude, Claude, Session};
use greffier_coeur::lexique::AjoutAuLexique;
use serde_json::{Value, json};

#[derive(Default)]
struct FauxClaude {
    reponses: Mutex<Vec<Result<Value, String>>>,
    appels: Mutex<Vec<AppelAClaude>>,
}

impl FauxClaude {
    fn repondant(reponses: Vec<Result<Value, String>>) -> FauxClaude {
        FauxClaude { reponses: Mutex::new(reponses), appels: Mutex::default() }
    }
    fn appels(&self) -> Vec<AppelAClaude> {
        self.appels.lock().unwrap().clone()
    }
}

impl Claude for FauxClaude {
    fn demander(&self, appel: &AppelAClaude, _: &AtomicBool) -> Result<Value, String> {
        self.appels.lock().unwrap().push(appel.clone());
        let mut r = self.reponses.lock().unwrap();
        if r.is_empty() { Err("appel de trop".into()) } else { r.remove(0) }
    }
}

/// Une demande de réécoute : l'enregistrement, les extraits, le vocabulaire.
type DemandeDeReecoute = (PathBuf, Vec<(f64, f64)>, String);

#[derive(Default)]
struct FausseReecoute {
    textes: Vec<String>,
    echec: Option<String>,
    demandes: Mutex<Vec<DemandeDeReecoute>>,
}

impl Reecoute for FausseReecoute {
    fn reecouter(&self, enregistrement: &Path, extraits: &[(f64, f64)], vocabulaire: &str, _: &AtomicBool) -> Result<Vec<String>, String> {
        self.demandes.lock().unwrap().push((enregistrement.to_path_buf(), extraits.to_vec(), vocabulaire.to_string()));
        match &self.echec {
            Some(e) => Err(e.clone()),
            None => Ok(self.textes.iter().take(extraits.len()).cloned().collect()),
        }
    }
}

fn replique(numero: usize, locuteur: &str, debut: f64, fin: f64, texte: &str) -> RepliquePourLAgent {
    RepliquePourLAgent { numero, locuteur: locuteur.into(), nom: None, debut, fin, texte: texte.into() }
}

fn demande() -> DemandeDeRelecture {
    DemandeDeRelecture {
        enregistrement: "/reunions/2026-11-21 14h02 - Point/2026-11-21 14h02 - Point - enregistrement.mp4".into(),
        dossier_de_reunions: "/reunions".into(),
        nom_du_dossier: "2026-11-21 14h02 - Point".into(),
        titre: "Point".into(),
        debut: DateHeure::new(2026, 11, 21, 14, 2),
        nombre_de_locuteurs: Some(2),
        repliques: vec![
            replique(0, "SPEAKER_00", 0.5, 2.5, "Bonjour, on parle de Zorglube."),
            replique(1, "SPEAKER_01", 5.0, 7.0, "La recète est prête ?"),
            replique(2, "SPEAKER_01", 69.2, 111.0, "Pas encore, il faut attendre que l'équipe de recette ait fini, jeudi peut-être, ou alors la semaine prochaine si tout va bien."),
        ],
        lexique: "---\ntype: lexique\n---\n\n# Lexique\n\n## Erreurs connues\n\n| Entendu | Correct |\n|---|---|\n| Zorglube | Zorglub |\n".into(),
        vocabulaire: "Réunion sur Zorglub.".into(),
    }
}

fn premiere_passe() -> Value {
    json!({
        "corrections": [{ "ligne": 2, "avant": "recète", "apres": "recette", "raison": "homophone" }],
        "locuteurs": [{ "ligne": 3, "locuteur": "SPEAKER_00", "raison": "répond à la question" }],
        "doutes": [
            { "ligne": 3, "extrait": "jeudi", "hypothese": "mardi", "raison": "son couvert" },
            { "ligne": 1, "extrait": "Zorglube", "hypothese": "?", "raison": "nom inconnu" },
            { "ligne": 3, "extrait": "jeudi", "hypothese": "mardi", "raison": "en double" },
            { "ligne": 9, "extrait": "hors", "hypothese": "?", "raison": "ligne inconnue" },
            { "ligne": 2, "extrait": " ", "hypothese": "?", "raison": "extrait vide" }
        ],
        "lexique": [{ "entendu": "recète", "correct": "recette" }]
    })
}

fn deuxieme_passe() -> Value {
    json!({
        "decisions": [
            { "ligne": 3, "extrait": "jeudi", "statut": "corrige", "apres": "mardi", "raison": "la réécoute dit mardi" },
            { "ligne": 1, "extrait": "Zorglube", "statut": "a_verifier", "apres": "Zorglub", "raison": "pas clair" }
        ],
        "lexique": [{ "entendu": "Zorglube", "correct": "Zorglub" }]
    })
}

fn relire(claude: &FauxClaude, reecoute: &FausseReecoute, demande: DemandeDeRelecture) -> Result<greffier_coeur::adaptateurs::ResultatDeRelecture, String> {
    relecture::relire(claude, reecoute, &demande, &AtomicBool::new(false), &mut |_| {})
}

#[test]
fn la_premiere_passe_recoit_les_repliques_numerotees_le_lexique_et_le_contexte() {
    let claude = FauxClaude::repondant(vec![Ok(json!({ "corrections": [], "locuteurs": [], "doutes": [], "lexique": [] }))]);
    relire(&claude, &FausseReecoute::default(), demande()).expect("relecture");

    let appels = claude.appels();
    assert_eq!(appels.len(), 1, "sans passage douteux, ni réécoute ni deuxième passe");
    let a = &appels[0];
    assert_eq!(a.entree, "#1 [0:00:00 → 0:00:02] SPEAKER_00 : Bonjour, on parle de Zorglube.\n#2 [0:00:05 → 0:00:07] SPEAKER_01 : La recète est prête ?\n#3 [0:01:09 → 0:01:51] SPEAKER_01 : Pas encore, il faut attendre que l'équipe de recette ait fini, jeudi peut-être, ou alors la semaine prochaine si tout va bien.");
    assert!(matches!(&a.session, Session::Nouvelle(id) if id.len() == 36), "{:?}", a.session);
    assert_eq!(a.repertoire, PathBuf::from("/reunions"));
    assert!(a.prompt.contains("Réunion du 2026-11-21"), "{}", a.prompt);
    assert!(a.prompt.contains("`2026-11-21 14h02 - Point`"), "{}", a.prompt);
    assert!(a.prompt.contains("2 personnes dans la réunion."), "{}", a.prompt);
    assert!(a.prompt.contains("## Erreurs connues\n\n| Entendu | Correct |\n|---|---|\n| Zorglube | Zorglub |"), "{}", a.prompt);
    assert!(!a.prompt.contains("type: lexique") && !a.prompt.contains("\n# Lexique") && !a.prompt.contains("{{"), "{}", a.prompt);
    assert!(!a.prompt.contains("Deuxième passe"), "la deuxième partie du prompt attend la réécoute");
    for outil in ["Write", "Edit", "Bash", "WebFetch", "WebSearch", "Task"] {
        assert!(a.outils_interdits.iter().any(|o| o == outil) && !a.outils_permis.iter().any(|o| o == outil), "{outil}");
    }
    assert_eq!(a.outils_permis, ["Read", "Glob", "Grep"]);
    let schema: Value = serde_json::from_str(&a.schema).expect("schéma JSON");
    assert_eq!(schema["required"], json!(["corrections", "locuteurs", "doutes", "lexique"]));
}

#[test]
fn les_passages_douteux_sont_reecoutes_puis_tranches_dans_la_meme_session() {
    let claude = FauxClaude::repondant(vec![Ok(premiere_passe()), Ok(deuxieme_passe())]);
    let reecoute = FausseReecoute { textes: vec!["pas encore mardi peut-être".into(), "".into()], ..Default::default() };
    let r = relire(&claude, &reecoute, demande()).expect("relecture");

    // ±1,5 s autour de la réplique, 28 s au plus : la réplique 3 (42 s) est centrée sur l'extrait.
    let demandes = reecoute.demandes.lock().unwrap().clone();
    assert_eq!(demandes.len(), 1);
    let (enregistrement, extraits, vocabulaire) = &demandes[0];
    assert_eq!(enregistrement, &demande().enregistrement);
    assert_eq!(vocabulaire, "Réunion sur Zorglub.");
    let centre = 69.2 + 63.0 / 126.0 * 41.8;
    let attendus = [(centre - 12.0, centre + 14.0), (0.0, 4.0)];
    assert_eq!(extraits.len(), 2);
    for ((a, b), (c, d)) in extraits.iter().zip(attendus) {
        assert!((a - c).abs() < 1e-6 && (b - d).abs() < 1e-6, "{extraits:?} ≠ {attendus:?}");
    }

    let appels = claude.appels();
    assert_eq!(appels.len(), 2);
    let Session::Nouvelle(id) = &appels[0].session else { panic!("{:?}", appels[0].session) };
    assert_eq!(appels[1].session, Session::Reprise(id.clone()));
    assert_eq!(appels[1].entree, "");
    assert!(appels[1].prompt.starts_with("## Deuxième passe"), "{}", appels[1].prompt);
    assert!(
        appels[1].prompt.ends_with("- ligne #3 [0:01:09] · extrait : « jeudi » · ton hypothèse : « mardi »\n  réécoute : « pas encore mardi peut-être »\n- ligne #1 [0:00:00] · extrait : « Zorglube » · ton hypothèse : « ? »\n  réécoute : « (rien d'exploitable) »"),
        "{}",
        appels[1].prompt
    );
    let schema: Value = serde_json::from_str(&appels[1].schema).expect("schéma JSON");
    assert_eq!(schema["required"], json!(["decisions", "lexique"]));

    assert_eq!(r.session.as_deref(), Some(id.as_str()));
    assert_eq!(r.corrections, [CorrectionSure { replique: 1, avant: "recète".into(), apres: "recette".into() }]);
    assert_eq!(r.changements_de_locuteur, [ChangementDeLocuteur { replique: 2, locuteur: "SPEAKER_00".into() }]);
    assert_eq!(r.passages_corriges, [PassageCorrige { replique: 2, extrait: "jeudi".into(), apres: "mardi".into(), reecoute: "pas encore mardi peut-être".into() }]);
    assert_eq!(r.passages_a_verifier, [PassageSignale { replique: 0, extrait: "Zorglube".into(), hypothese: Some("Zorglub".into()), reecoute: "".into() }]);
    assert_eq!(r.ajouts_au_lexique, [AjoutAuLexique { entendu: "recète".into(), correct: "recette".into() }, AjoutAuLexique { entendu: "Zorglube".into(), correct: "Zorglub".into() }]);
}

#[test]
fn un_passage_sans_decision_ou_corrige_en_point_d_interrogation_reste_a_verifier() {
    let deuxieme = json!({ "decisions": [{ "ligne": 1, "extrait": "autre chose", "statut": "corrige", "apres": "? (un nom de client, peut-être)", "raison": "?" }], "lexique": [] });
    let claude = FauxClaude::repondant(vec![Ok(premiere_passe()), Ok(deuxieme)]);
    let reecoute = FausseReecoute { textes: vec!["a".into(), "b".into()], ..Default::default() };
    let r = relire(&claude, &reecoute, demande()).expect("relecture");
    assert!(r.passages_corriges.is_empty());
    let passages: Vec<(usize, &str, Option<&str>, &str)> = r.passages_a_verifier.iter().map(|p| (p.replique, p.extrait.as_str(), p.hypothese.as_deref(), p.reecoute.as_str())).collect();
    assert_eq!(passages, [(2, "jeudi", Some("mardi"), "a"), (0, "Zorglube", None, "b")]);
}

#[test]
fn trente_passages_douteux_au_plus() {
    let doutes: Vec<Value> = (0..40).map(|i| json!({ "ligne": 3, "extrait": format!("mot {i}"), "hypothese": "?", "raison": "" })).collect();
    let claude = FauxClaude::repondant(vec![Ok(json!({ "corrections": [], "locuteurs": [], "doutes": doutes, "lexique": [] })), Ok(json!({ "decisions": [], "lexique": [] }))]);
    let r = relire(&claude, &FausseReecoute::default(), demande()).expect("relecture");
    assert_eq!(r.passages_a_verifier.len(), 30);
}

#[test]
fn une_reecoute_impossible_n_empeche_pas_la_deuxieme_passe() {
    let claude = FauxClaude::repondant(vec![Ok(premiere_passe()), Ok(deuxieme_passe())]);
    let reecoute = FausseReecoute { echec: Some("modèle illisible".into()), ..Default::default() };
    let r = relire(&claude, &reecoute, demande()).expect("relecture");
    assert!(claude.appels()[1].prompt.contains("réécoute : « (réécoute impossible : modèle illisible) »"), "{}", claude.appels()[1].prompt);
    assert_eq!(r.passages_corriges.len(), 1);
}

#[test]
fn si_claude_echoue_la_relecture_echoue_avec_son_message() {
    let claude = FauxClaude::repondant(vec![Err("Claude : Credit balance is too low".into())]);
    assert_eq!(relire(&claude, &FausseReecoute::default(), demande()), Err("Claude : Credit balance is too low".into()));

    let claude = FauxClaude::repondant(vec![Ok(premiere_passe()), Err("Claude : délai dépassé".into())]);
    assert_eq!(relire(&claude, &FausseReecoute::default(), demande()), Err("Claude : délai dépassé".into()));

    let claude = FauxClaude::repondant(vec![Ok(json!({ "corrections": "pas une liste" }))]);
    let e = relire(&claude, &FausseReecoute::default(), demande()).expect_err("réponse illisible");
    assert!(e.starts_with("Claude a rendu une réponse illisible"), "{e}");
}

#[test]
fn la_reponse_de_la_cli_donne_la_sortie_structuree_ou_un_message_clair() {
    let ok = r#"{"type":"result","is_error":false,"result":"","session_id":"abc","structured_output":{"x":1}}"#;
    assert_eq!(claude::lire_la_reponse(ok, "", true), Ok(json!({ "x": 1 })));

    let erreur = r#"{"type":"result","is_error":true,"result":"Credit balance is too low"}"#;
    assert_eq!(claude::lire_la_reponse(erreur, "", false), Err("Claude : Credit balance is too low".into()));

    let sans = r#"{"type":"result","is_error":false,"result":"Je n'ai pas pu."}"#;
    assert_eq!(claude::lire_la_reponse(sans, "", true), Err("Claude n'a pas rendu de réponse structurée : Je n'ai pas pu.".into()));

    assert_eq!(claude::lire_la_reponse("", "ligne 1\nligne 2\n\nError: not logged in\n", false), Err("Claude : ligne 2 / Error: not logged in".into()));
    assert_eq!(claude::lire_la_reponse("", "", false), Err("Claude s'est arrêté sans rien dire.".into()));
}
