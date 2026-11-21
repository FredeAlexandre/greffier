//! Les ressources téléchargées au premier lancement, vues par le cœur : modèles vérifiés par
//! sha256, téléchargements qui reprennent, Python privé et vérification de la CLI `claude`. Les
//! outils (téléchargement, empreinte, uv, `claude`) sont faux ; tout le reste est celui de l'app.

mod banc;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use banc::Banc;
use greffier_coeur::adaptateurs::{EtatDeRessource, Ressource};
use greffier_coeur::faux::FauxOutils;
use greffier_coeur::ressources::{Catalogue, Element, EtatDeClaude, Fichier, RessourcesDeLApp};
use greffier_coeur::{Commande, Etape, Evenement, Statut};

const GRAND: &str = "poids du grand modèle, trente-deux octets";
const SHA_GRAND: &str = "97325706e5b8a9bc0cd158ac197ee74e20e19781defb99affe5a375866fca53a";
const URL_GRAND: &str = "https://exemple.org/grand.bin";
const PETIT: &str = "petit modèle";
const SHA_PETIT: &str = "f043ba3eb2682d2f4c03e0f655434803aedd1506facdc371a0544ae0ae568684";
const URL_PETIT: &str = "https://exemple.org/petit.bin";
const PYPROJECT: &str = "[project]\nname = \"locuteurs\"\n";
const VERROU: &str = "version = 1\n";

/// Le dossier de l'app, dans un répertoire temporaire supprimé à la fin du test.
struct DossierDeLApp(PathBuf);

impl DossierDeLApp {
    fn new() -> DossierDeLApp {
        static N: AtomicU32 = AtomicU32::new(0);
        let d = std::env::temp_dir().join(format!("greffier-app-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        let _ = std::fs::remove_dir_all(&d);
        DossierDeLApp(d)
    }
}

impl Drop for DossierDeLApp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn catalogue(verrou: &str) -> Catalogue {
    Catalogue(vec![
        Element::fichiers("Grand modèle", "exemple.org", &[Etape::Transcription], vec![Fichier::new("modeles/grand.bin", URL_GRAND, GRAND.len() as u64, SHA_GRAND)]),
        Element::fichiers("Petit modèle", "exemple.org", &[Etape::Transcription], vec![Fichier::new("modeles/petit/a.bin", URL_PETIT, PETIT.len() as u64, SHA_PETIT)]),
        Element::python("Python privé", "uv", PYPROJECT, verrou),
        Element::claude("Claude Code"),
    ])
}

/// 10 octets par seconde ; une empreinte prend 0,5 s, le Python 2 s, `claude` répond aussitôt.
fn outils() -> FauxOutils {
    FauxOutils::new(10.0).servir(URL_GRAND, GRAND.as_bytes()).servir(URL_PETIT, PETIT.as_bytes()).duree_d_empreinte(0.5).python(2.0, Ok(()))
}

fn banc_sur(app: &Path, outils: FauxOutils) -> Banc {
    let mut a = banc::adaptateurs();
    a.ressources = Box::new(RessourcesDeLApp::new(app, catalogue(VERROU), outils));
    Banc::avec(a)
}

fn etat(banc: &Banc, nom: &str) -> EtatDeRessource {
    ressource(banc, nom).etat
}

fn ressource(banc: &Banc, nom: &str) -> Ressource {
    banc.greffier.ressources().iter().find(|r| r.nom == nom).unwrap_or_else(|| panic!("ressource {nom}")).clone()
}

fn installer_jusqu_au_bout(banc: &mut Banc) -> Vec<Evenement> {
    banc.executer(Commande::InstallerRessources);
    let mut evenements = Vec::new();
    for _ in 0..300 {
        evenements.extend(banc.avancer(0.1));
        if !banc.greffier.occupe() {
            return evenements;
        }
    }
    panic!("installation toujours en cours après 30 s");
}

#[test]
fn le_premier_lancement_montre_l_etat_de_chaque_ressource_jusqu_a_ce_qu_elles_soient_pretes() {
    let app = DossierDeLApp::new();
    let mut banc = banc_sur(&app.0, outils());
    let noms: Vec<String> = banc.greffier.ressources().iter().map(|r| r.nom.clone()).collect();
    assert_eq!(noms, ["Grand modèle", "Petit modèle", "Python privé", "Claude Code"]);
    assert_eq!(etat(&banc, "Grand modèle"), EtatDeRessource::Absente);
    assert_eq!(etat(&banc, "Python privé"), EtatDeRessource::Absente);

    banc.executer(Commande::InstallerRessources);
    banc.avancer(0.3);
    // Les plus petits fichiers d'abord ; le Python s'installe en même temps.
    assert_eq!(etat(&banc, "Petit modèle"), EtatDeRessource::EnTelechargement { recu: 3, total: 13 });
    assert_eq!(etat(&banc, "Grand modèle"), EtatDeRessource::Absente);
    assert_eq!(etat(&banc, "Python privé"), EtatDeRessource::EnInstallation);
    assert_eq!(etat(&banc, "Claude Code"), EtatDeRessource::Prete);

    banc.avancer(1.2);
    assert_eq!(etat(&banc, "Petit modèle"), EtatDeRessource::EnVerification);
    banc.avancer(0.6);
    assert_eq!(etat(&banc, "Petit modèle"), EtatDeRessource::Prete);
    assert!(matches!(etat(&banc, "Grand modèle"), EtatDeRessource::EnTelechargement { recu, total: 42 } if recu > 0));

    let evenements = installer_jusqu_au_bout(&mut banc);
    assert!(evenements.contains(&Evenement::RessourcesInstallees), "{evenements:?}");
    for r in banc.greffier.ressources() {
        assert_eq!(r.etat, EtatDeRessource::Prete, "{}", r.nom);
    }
    assert_eq!(banc.lire(&app.0.join("modeles/grand.bin")), GRAND);
    assert_eq!(banc.lire(&app.0.join("modeles/petit/a.bin")), PETIT);
    assert_eq!(ressource(&banc, "Grand modèle").taille, 42);
}

#[test]
fn les_reglages_voient_la_place_prise_par_chaque_ressource() {
    let app = DossierDeLApp::new();
    let mut banc = banc_sur(&app.0, outils());
    installer_jusqu_au_bout(&mut banc);
    assert_eq!(ressource(&banc, "Grand modèle").taille, GRAND.len() as u64);
    assert_eq!(ressource(&banc, "Petit modèle").taille, PETIT.len() as u64);
    assert!(ressource(&banc, "Python privé").taille >= (PYPROJECT.len() + VERROU.len()) as u64);
    assert_eq!(ressource(&banc, "Claude Code").taille, 0);
}

#[test]
fn un_sha256_faux_met_la_ressource_en_echec_puis_le_fichier_est_retelecharge() {
    let app = DossierDeLApp::new();
    let mut banc = banc_sur(&app.0, outils().corrompre_une_fois(URL_GRAND));

    let evenements = installer_jusqu_au_bout(&mut banc);
    let EtatDeRessource::Echec(message) = etat(&banc, "Grand modèle") else { panic!("{:?}", etat(&banc, "Grand modèle")) };
    assert!(message.contains("sha256"), "{message}");
    assert!(evenements.iter().any(|e| matches!(e, Evenement::InstallationEnEchec(m) if m.contains("Grand modèle"))), "{evenements:?}");
    assert_eq!(etat(&banc, "Petit modèle"), EtatDeRessource::Prete);
    // Rien de faux ne reste sur le disque.
    assert_eq!(ressource(&banc, "Grand modèle").taille, 0);
    assert!(!app.0.join("modeles/grand.bin").exists());

    let evenements = installer_jusqu_au_bout(&mut banc);
    assert!(evenements.contains(&Evenement::RessourcesInstallees), "{evenements:?}");
    assert_eq!(etat(&banc, "Grand modèle"), EtatDeRessource::Prete);
    assert_eq!(banc.lire(&app.0.join("modeles/grand.bin")), GRAND);
}

#[test]
fn un_telechargement_coupe_reprend_la_ou_il_s_est_arrete_au_lancement_suivant() {
    let app = DossierDeLApp::new();
    {
        let mut banc = banc_sur(&app.0, outils().couper_une_fois(URL_GRAND, 20));
        let evenements = installer_jusqu_au_bout(&mut banc);
        assert!(evenements.iter().any(|e| matches!(e, Evenement::InstallationEnEchec(_))), "{evenements:?}");
        assert!(matches!(etat(&banc, "Grand modèle"), EtatDeRessource::Echec(m) if m.contains("reprendra")));
    }

    // L'app est relancée : le téléchargement est vu comme interrompu, et reprend à 20 octets.
    let mut banc = banc_sur(&app.0, outils());
    assert_eq!(etat(&banc, "Grand modèle"), EtatDeRessource::Interrompue { recu: 20, total: 42 });
    assert_eq!(etat(&banc, "Petit modèle"), EtatDeRessource::Prete);
    banc.executer(Commande::InstallerRessources);
    banc.avancer(0.1);
    assert_eq!(etat(&banc, "Grand modèle"), EtatDeRessource::EnTelechargement { recu: 21, total: 42 });

    installer_jusqu_au_bout(&mut banc);
    assert_eq!(etat(&banc, "Grand modèle"), EtatDeRessource::Prete);
    assert_eq!(banc.lire(&app.0.join("modeles/grand.bin")), GRAND);
}

#[test]
fn le_python_prive_s_installe_dans_le_dossier_de_l_app_et_se_reinstalle_si_le_verrou_change() {
    let app = DossierDeLApp::new();
    {
        let mut banc = banc_sur(&app.0, outils());
        installer_jusqu_au_bout(&mut banc);
        assert_eq!(etat(&banc, "Python privé"), EtatDeRessource::Prete);
        assert_eq!(banc.lire(&app.0.join("python/projet/uv.lock")), VERROU);
        assert_eq!(banc.lire(&app.0.join("python/projet/pyproject.toml")), PYPROJECT);
    }
    {
        let banc = banc_sur(&app.0, outils());
        assert_eq!(etat(&banc, "Python privé"), EtatDeRessource::Prete);
    }
    let mut a = banc::adaptateurs();
    a.ressources = Box::new(RessourcesDeLApp::new(&app.0, catalogue("version = 2\n"), outils()));
    let banc = Banc::avec(a);
    assert_eq!(etat(&banc, "Python privé"), EtatDeRessource::Absente);
}

#[test]
fn l_echec_de_l_installation_du_python_est_montre_avec_son_message() {
    let app = DossierDeLApp::new();
    let mut banc = banc_sur(&app.0, outils().python(1.0, Err("import pyannote.audio a échoué".into())));
    installer_jusqu_au_bout(&mut banc);
    assert_eq!(etat(&banc, "Python privé"), EtatDeRessource::Echec("import pyannote.audio a échoué".into()));
    assert_eq!(etat(&banc, "Grand modèle"), EtatDeRessource::Prete);
}

#[test]
fn claude_absente_ou_pas_connectee_est_signalee_avec_quoi_faire() {
    let app = DossierDeLApp::new();
    let mut banc = banc_sur(&app.0, outils().claude(EtatDeClaude::Absente));
    banc.executer(Commande::VerifierRessources);
    banc.avancer(0.1);
    let claude = ressource(&banc, "Claude Code");
    assert_eq!(claude.etat, EtatDeRessource::Absente);
    assert!(claude.aide.as_deref().is_some_and(|a| a.contains("claude")), "{claude:?}");

    let mut banc = banc_sur(&app.0, outils().claude(EtatDeClaude::NonConnectee { version: "2.1.329".into() }));
    banc.executer(Commande::VerifierRessources);
    banc.avancer(0.1);
    let claude = ressource(&banc, "Claude Code");
    assert!(matches!(&claude.etat, EtatDeRessource::Echec(m) if m.contains("connectée")), "{claude:?}");
    assert!(claude.aide.as_deref().is_some_and(|a| a.contains("/login")), "{claude:?}");

    let mut banc = banc_sur(&app.0, outils().claude(EtatDeClaude::Connectee { version: "2.1.329".into(), compte: "claude.ai".into() }));
    banc.executer(Commande::VerifierRessources);
    banc.avancer(0.1);
    let claude = ressource(&banc, "Claude Code");
    assert_eq!(claude.etat, EtatDeRessource::Prete);
    assert!(claude.detail.contains("2.1.329"), "{claude:?}");
    assert_eq!(claude.aide, None);
}

#[test]
fn sans_claude_la_transcription_passe_mais_la_relecture_attend() {
    let app = DossierDeLApp::new();
    let mut banc = banc_sur(&app.0, outils().claude(EtatDeClaude::Absente));
    installer_jusqu_au_bout(&mut banc);
    let id = banc.reunion_a_traiter();
    banc.executer(Commande::Traiter(id));
    let etapes = banc.avancer_jusqu_a(id, |s| matches!(s, Statut::Echec { .. }));
    assert_eq!(etapes, [Etape::Transcription]);
    assert_eq!(banc.reunion(id).statut, Statut::Echec { etape: Etape::Relecture, message: "Ressources manquantes : Claude Code.".into() });
}
