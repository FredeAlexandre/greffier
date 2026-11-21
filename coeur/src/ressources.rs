//! Les ressources de Greffier, toutes dans le dossier de l'app
//! (`~/Library/Application Support/Greffier/`) et nulle part ailleurs : les modèles, téléchargés et
//! vérifiés par sha256, l'environnement Python privé de pyannote, installé par uv, et la
//! vérification de la CLI `claude`, seule dépendance que l'app n'installe pas.
//!
//! [`RessourcesDeLApp`] est l'adaptateur [`Ressources`] de l'app : il décide quoi télécharger,
//! reprend les téléchargements interrompus, vérifie les sha256 et range les fichiers. Le travail
//! lent (télécharger, calculer une empreinte, lancer uv ou `claude`) passe par des [`Outils`] :
//! ceux du Mac (`greffier_outils::mac::OutilsDuMac`) ou les faux des tests ([`crate::faux::FauxOutils`]).
//!
//! Sur le disque, un fichier en cours de téléchargement s'appelle `<nom>.partiel` ; il ne prend
//! son nom qu'une fois son sha256 vérifié. Un fichier présent à la bonne taille est donc prêt.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use sha2::{Digest, Sha256};

use crate::adaptateurs::{Avancement, EtatDeRessource, Ressource, Ressources, TacheDe};
use crate::reunion::Etape;

pub use crate::reglages::dossier_de_l_app;

/// Le modèle de Whisper, dans le dossier de l'app.
pub const MODELE_WHISPER: &str = "modeles/ggml-large-v3.bin";
/// Le modèle du VAD Silero, dans le dossier de l'app.
pub const MODELE_VAD: &str = "modeles/ggml-silero-v5.1.2.bin";
/// Le dossier du modèle de pyannote, dans le dossier de l'app.
pub const MODELE_PYANNOTE: &str = "modeles/pyannote-community-1";

/// Un fichier à télécharger.
#[derive(Clone, Debug, PartialEq)]
pub struct Fichier {
    /// Relatif au dossier de l'app.
    pub chemin: String,
    pub url: String,
    pub taille: u64,
    /// En hexadécimal minuscule.
    pub sha256: String,
}

impl Fichier {
    pub fn new(chemin: &str, url: &str, taille: u64, sha256: &str) -> Fichier {
        Fichier { chemin: chemin.into(), url: url.into(), taille, sha256: sha256.into() }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Contenu {
    Fichiers(Vec<Fichier>),
    /// L'environnement Python privé, installé par uv à partir de son projet et de son verrou.
    Python { pyproject: String, verrou: String },
    /// La CLI `claude`, qu'on ne fait que vérifier.
    Claude,
}

/// Une ligne du catalogue, c'est-à-dire de l'écran du premier lancement.
#[derive(Clone, Debug, PartialEq)]
pub struct Element {
    pub nom: String,
    pub detail: String,
    pub etapes: Vec<Etape>,
    pub contenu: Contenu,
}

impl Element {
    pub fn fichiers(nom: &str, detail: &str, etapes: &[Etape], fichiers: Vec<Fichier>) -> Element {
        Element { nom: nom.into(), detail: detail.into(), etapes: etapes.to_vec(), contenu: Contenu::Fichiers(fichiers) }
    }
    pub fn python(nom: &str, detail: &str, pyproject: &str, verrou: &str) -> Element {
        Element { nom: nom.into(), detail: detail.into(), etapes: vec![Etape::Transcription], contenu: Contenu::Python { pyproject: pyproject.into(), verrou: verrou.into() } }
    }
    pub fn claude(nom: &str) -> Element {
        Element { nom: nom.into(), detail: "CLI d'Anthropic".into(), etapes: vec![Etape::Relecture, Etape::Bilan], contenu: Contenu::Claude }
    }
}

/// Tout ce dont le traitement a besoin.
#[derive(Clone, Debug, PartialEq)]
pub struct Catalogue(pub Vec<Element>);

const RELEASE_PYANNOTE: &str = "https://github.com/FredeAlexandre/greffier/releases/download/pyannote-community-1";

impl Catalogue {
    /// Les ressources de Greffier, aux sha256 figés (ceux du vault pour whisper et Silero ; ceux de
    /// la révision 3533c8c de pyannote, republiée en release du dépôt par `scripts/publier-pyannote`).
    pub fn de_greffier() -> Catalogue {
        let pyannote = |chemin: &str, asset: &str, taille: u64, sha: &str| Fichier::new(&format!("{MODELE_PYANNOTE}/{chemin}"), &format!("{RELEASE_PYANNOTE}/{asset}"), taille, sha);
        Catalogue(vec![
            Element::fichiers(
                "Whisper large-v3",
                "huggingface.co/ggerganov/whisper.cpp",
                &[Etape::Transcription],
                vec![Fichier::new(
                    MODELE_WHISPER,
                    "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3.bin",
                    3_095_033_483,
                    "64d182b440b98d5203c4f9bd541544d84c605196c4f7b845dfa11fb23594d1e2",
                )],
            ),
            Element::fichiers(
                "Silero VAD v5.1.2",
                "huggingface.co/ggml-org/whisper-vad",
                &[Etape::Transcription],
                vec![Fichier::new(
                    MODELE_VAD,
                    "https://huggingface.co/ggml-org/whisper-vad/resolve/main/ggml-silero-v5.1.2.bin",
                    885_098,
                    "29940d98d42b91fbd05ce489f3ecf7c72f0a42f027e4875919a28fb4c04ea2cf",
                )],
            ),
            Element::fichiers(
                "pyannote community-1",
                "release GitHub de Greffier · CC-BY-4.0",
                &[Etape::Transcription],
                vec![
                    pyannote("config.yaml", "config.yaml", 444, "5ce2bfa9a938dc132cec1172592d65173cbb8f444ea1e4133f10f9391de155be"),
                    pyannote("segmentation/pytorch_model.bin", "segmentation-pytorch_model.bin", 5_906_507, "7ad24338d844fb95985486eb1a464e32d229f6d7a03c9abe60f978bacf3f816e"),
                    pyannote("embedding/pytorch_model.bin", "embedding-pytorch_model.bin", 26_646_242, "6f10ff60898a1d185fa22e1d11e0bfa8a92efec811f11bca48cb8cafebefd929"),
                    pyannote("plda/plda.npz", "plda-plda.npz", 133_852, "9b77bcd840692710dd3496f62ecfeed8d8e5f002fd991b785079b244eab7d255"),
                    pyannote("plda/xvec_transform.npz", "plda-xvec_transform.npz", 134_376, "325f1ce8e48f7e55e9c8aa47e05d2766b7c48c4b25b8de8dd751e7a4cc5fbe8f"),
                ],
            ),
            Element::python("Python privé", "pyannote.audio 4.0.7 · installé par uv", PYPROJECT, VERROU),
            Element::claude("Claude Code"),
        ])
    }

    /// Ce qu'il reste à télécharger, en octets.
    pub fn a_telecharger(&self, racine: &Path) -> u64 {
        self.0
            .iter()
            .flat_map(|e| match &e.contenu {
                Contenu::Fichiers(f) => f.as_slice(),
                _ => &[],
            })
            .filter(|f| !pret(racine, f))
            .map(|f| f.taille - std::fs::metadata(partiel(racine, f)).map_or(0, |m| m.len()).min(f.taille))
            .sum()
    }
}

/// Le projet Python de pyannote, repris du vault (dépendances figées par `uv.lock`).
pub const PYPROJECT: &str = include_str!("../ressources/python/pyproject.toml");
pub const VERROU: &str = include_str!("../ressources/python/uv.lock");

/// Ce que la vérification de la CLI `claude` a trouvé.
#[derive(Clone, Debug, PartialEq)]
pub enum EtatDeClaude {
    Connectee { version: String, compte: String },
    NonConnectee { version: String },
    Absente,
    Echec(String),
}

/// Le travail lent des ressources, hors du cœur.
pub trait Outils {
    /// Télécharge `url` dans `partiel`, en reprenant à la fin de ce qu'il contient déjà.
    fn telecharger(&mut self, url: &str, partiel: &Path, taille: u64) -> TacheDe<()>;
    /// Le sha256 du fichier, en hexadécimal minuscule.
    fn empreinte(&mut self, fichier: &Path) -> TacheDe<String>;
    /// Installe l'environnement Python décrit par `projet` (`pyproject.toml`, `uv.lock`) dans
    /// `dossier`, sans rien écrire ailleurs, puis vérifie qu'`import pyannote.audio` y réussit.
    fn installer_python(&mut self, projet: &Path, dossier: &Path) -> TacheDe<()>;
    fn verifier_claude(&mut self) -> TacheDe<EtatDeClaude>;
}

/// Le sha256 d'octets, en hexadécimal minuscule.
pub fn sha256(octets: &[u8]) -> String {
    hex(&Sha256::digest(octets))
}

pub fn hex(octets: &[u8]) -> String {
    octets.iter().map(|o| format!("{o:02x}")).collect()
}

fn final_(racine: &Path, f: &Fichier) -> PathBuf {
    racine.join(&f.chemin)
}

fn partiel(racine: &Path, f: &Fichier) -> PathBuf {
    racine.join(format!("{}.partiel", f.chemin))
}

fn taille_de(chemin: &Path) -> u64 {
    std::fs::metadata(chemin).map_or(0, |m| m.len())
}

/// Un fichier est prêt quand il porte son nom à la bonne taille : il n'est renommé qu'une fois
/// son sha256 vérifié.
fn pret(racine: &Path, f: &Fichier) -> bool {
    std::fs::metadata(final_(racine, f)).is_ok_and(|m| m.is_file() && m.len() == f.taille)
}

/// La place prise par un dossier, sans suivre les liens.
fn place(dossier: &Path) -> u64 {
    let Ok(entrees) = std::fs::read_dir(dossier) else { return 0 };
    entrees
        .flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => place(&e.path()),
            Ok(t) if t.is_file() => e.metadata().map_or(0, |m| m.len()),
            _ => 0,
        })
        .sum()
}

const DOSSIER_PYTHON: &str = "python";

/// L'empreinte du projet Python : le témoin d'installation la retient, un nouveau verrou
/// réinstalle l'environnement.
fn empreinte_du_projet(pyproject: &str, verrou: &str) -> String {
    sha256(format!("{pyproject}\0{verrou}").as_bytes())
}

/// Ce qui se passe pour un élément pendant une installation.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Phase {
    Telechargement,
    Verification,
    Installation,
}

struct Partage {
    racine: PathBuf,
    catalogue: Catalogue,
    outils: Box<dyn Outils>,
    phases: HashMap<usize, Phase>,
    echecs: HashMap<usize, String>,
    claude: Option<EtatDeClaude>,
    /// La place prise par le Python, calculée une fois (le dossier compte des dizaines de
    /// milliers de fichiers).
    place_du_python: Option<u64>,
}

impl Partage {
    fn dossier_python(&self) -> PathBuf {
        self.racine.join(DOSSIER_PYTHON)
    }

    fn python_pret(&self, pyproject: &str, verrou: &str) -> bool {
        std::fs::read_to_string(self.dossier_python().join("pret")).is_ok_and(|t| t.trim() == empreinte_du_projet(pyproject, verrou))
    }

    fn ressource(&mut self, i: usize) -> Ressource {
        let e = &self.catalogue.0[i];
        let mut r = Ressource { nom: e.nom.clone(), detail: e.detail.clone(), etat: EtatDeRessource::Absente, taille: 0, total: 0, aide: None, etapes: e.etapes.clone() };
        let phase = self.phases.get(&i).copied();
        match &e.contenu {
            Contenu::Fichiers(fichiers) => {
                let total: u64 = fichiers.iter().map(|f| f.taille).sum();
                let recu: u64 = fichiers.iter().map(|f| taille_de(&final_(&self.racine, f)) + taille_de(&partiel(&self.racine, f))).sum();
                r.taille = recu;
                r.total = total;
                r.etat = match phase {
                    Some(Phase::Verification) => EtatDeRessource::EnVerification,
                    Some(_) => EtatDeRessource::EnTelechargement { recu: recu.min(total), total },
                    None if fichiers.iter().all(|f| pret(&self.racine, f)) => EtatDeRessource::Prete,
                    None => match self.echecs.get(&i) {
                        Some(m) => EtatDeRessource::Echec(m.clone()),
                        None if recu > 0 => EtatDeRessource::Interrompue { recu: recu.min(total), total },
                        None => EtatDeRessource::Absente,
                    },
                };
            }
            Contenu::Python { pyproject, verrou } => {
                let pret = self.python_pret(pyproject, verrou);
                r.etat = match (phase, self.echecs.get(&i)) {
                    (Some(_), _) => EtatDeRessource::EnInstallation,
                    (None, Some(m)) => EtatDeRessource::Echec(m.clone()),
                    (None, None) if pret => EtatDeRessource::Prete,
                    (None, None) => EtatDeRessource::Absente,
                };
                if phase.is_none() {
                    let dossier = self.dossier_python();
                    r.taille = *self.place_du_python.get_or_insert_with(|| place(&dossier));
                }
            }
            Contenu::Claude => {
                (r.etat, r.aide) = match (phase, &self.claude) {
                    (Some(_), _) | (None, None) => (EtatDeRessource::EnVerification, None),
                    (None, Some(EtatDeClaude::Connectee { version, compte })) => {
                        r.detail = format!("{version} · connectée ({compte})");
                        (EtatDeRessource::Prete, None)
                    }
                    (None, Some(EtatDeClaude::NonConnectee { version })) => {
                        r.detail = format!("{version} · pas connectée");
                        (
                            EtatDeRessource::Echec("La CLI claude est installée mais n'est pas connectée.".into()),
                            Some("Dans un terminal, lancer `claude` puis `/login`, puis cliquer sur « Vérifier à nouveau ».".into()),
                        )
                    }
                    (None, Some(EtatDeClaude::Absente)) => (
                        EtatDeRessource::Absente,
                        Some("La relecture et le bilan passent par Claude Code. L'installer (https://claude.com/claude-code), lancer `claude` une fois pour se connecter, puis cliquer sur « Vérifier à nouveau ».".into()),
                    ),
                    (None, Some(EtatDeClaude::Echec(m))) => (EtatDeRessource::Echec(m.clone()), Some("Vérifier que `claude` se lance dans un terminal, puis cliquer sur « Vérifier à nouveau ».".into())),
                };
            }
        }
        r
    }
}

/// L'adaptateur de ressources de l'app, sur un dossier de l'app et un catalogue.
pub struct RessourcesDeLApp {
    partage: Rc<RefCell<Partage>>,
}

impl RessourcesDeLApp {
    pub fn new(racine: impl Into<PathBuf>, catalogue: Catalogue, outils: impl Outils + 'static) -> RessourcesDeLApp {
        let partage = Partage { racine: racine.into(), catalogue, outils: Box::new(outils), phases: HashMap::new(), echecs: HashMap::new(), claude: None, place_du_python: None };
        RessourcesDeLApp { partage: Rc::new(RefCell::new(partage)) }
    }
}

impl Ressources for RessourcesDeLApp {
    fn etat(&mut self) -> Vec<Ressource> {
        let mut p = self.partage.borrow_mut();
        (0..p.catalogue.0.len()).map(|i| p.ressource(i)).collect()
    }

    fn installer(&mut self) -> TacheDe<()> {
        Box::new(Installation::demarrer(self.partage.clone(), true))
    }

    fn verifier(&mut self) -> TacheDe<()> {
        Box::new(Installation::demarrer(self.partage.clone(), false))
    }
}

enum Operation {
    Telechargement(TacheDe<()>),
    Verification(TacheDe<String>),
}

struct FichierEnCours {
    element: usize,
    fichier: Fichier,
    operation: Operation,
}

/// Une installation : les fichiers un par un (les plus petits d'abord), le Python en même temps,
/// et la vérification de `claude`.
struct Installation {
    partage: Rc<RefCell<Partage>>,
    a_faire: VecDeque<(usize, Fichier)>,
    en_cours: Option<FichierEnCours>,
    python: Option<(usize, TacheDe<()>)>,
    claude: Option<(usize, TacheDe<EtatDeClaude>)>,
    echecs: Vec<String>,
}

impl Installation {
    fn demarrer(partage: Rc<RefCell<Partage>>, installer: bool) -> Installation {
        let mut a_faire = Vec::new();
        let mut python = None;
        let mut claude = None;
        let mut echecs = Vec::new();
        {
            let mut p = partage.borrow_mut();
            let p = &mut *p;
            for (i, e) in p.catalogue.0.iter().enumerate() {
                match &e.contenu {
                    Contenu::Fichiers(fichiers) if installer => {
                        p.echecs.remove(&i);
                        let manquants: Vec<Fichier> = fichiers.iter().filter(|f| !pret(&p.racine, f)).cloned().collect();
                        if !manquants.is_empty() {
                            let total: u64 = manquants.iter().map(|f| f.taille).sum();
                            a_faire.push((total, i, manquants));
                        }
                    }
                    Contenu::Python { pyproject, verrou } if installer => {
                        p.echecs.remove(&i);
                        if !p.python_pret(pyproject, verrou) {
                            let dossier = p.racine.join(DOSSIER_PYTHON);
                            let projet = dossier.join("projet");
                            let ecrit = std::fs::create_dir_all(&projet)
                                .and_then(|()| std::fs::remove_file(dossier.join("pret")).or_else(|e| if e.kind() == std::io::ErrorKind::NotFound { Ok(()) } else { Err(e) }))
                                .and_then(|()| std::fs::write(projet.join("pyproject.toml"), pyproject))
                                .and_then(|()| std::fs::write(projet.join("uv.lock"), verrou));
                            match ecrit {
                                Ok(()) => {
                                    p.phases.insert(i, Phase::Installation);
                                    python = Some((i, p.outils.installer_python(&projet, &dossier)));
                                }
                                Err(err) => {
                                    let m = format!("{} : {err}", projet.display());
                                    echecs.push(format!("{} : {m}", e.nom));
                                    p.echecs.insert(i, m);
                                }
                            }
                        }
                    }
                    Contenu::Claude => {
                        p.phases.insert(i, Phase::Verification);
                        claude = Some((i, p.outils.verifier_claude()));
                    }
                    _ => {}
                }
            }
        }
        a_faire.sort_by_key(|(total, i, _)| (*total, *i));
        let a_faire = a_faire.into_iter().flat_map(|(_, i, fichiers)| fichiers.into_iter().map(move |f| (i, f))).collect();
        let mut installation = Installation { partage, a_faire, en_cours: None, python, claude, echecs };
        installation.suivant();
        installation
    }

    fn echec(&mut self, element: usize, message: String) {
        let mut p = self.partage.borrow_mut();
        p.phases.remove(&element);
        self.echecs.push(format!("{} : {message}", p.catalogue.0[element].nom));
        p.echecs.insert(element, message);
        self.a_faire.retain(|(e, _)| *e != element);
    }

    /// Lance le fichier suivant de la file, s'il n'y en a pas déjà un en cours.
    fn suivant(&mut self) {
        while self.en_cours.is_none() {
            let Some((element, fichier)) = self.a_faire.pop_front() else { return };
            let mut p = self.partage.borrow_mut();
            let (fin, part) = (final_(&p.racine, &fichier), partiel(&p.racine, &fichier));
            if pret(&p.racine, &fichier) {
                continue;
            }
            // Un fichier à la mauvaise taille sous son nom final n'a pas été vérifié : on le jette.
            let _ = std::fs::remove_file(&fin);
            if taille_de(&part) > fichier.taille {
                let _ = std::fs::remove_file(&part);
            }
            if let Some(parent) = part.parent()
                && let Err(e) = std::fs::create_dir_all(parent)
            {
                drop(p);
                self.echec(element, format!("{} : {e}", parent.display()));
                continue;
            }
            let operation = if taille_de(&part) == fichier.taille {
                p.phases.insert(element, Phase::Verification);
                Operation::Verification(p.outils.empreinte(&part))
            } else {
                p.phases.insert(element, Phase::Telechargement);
                Operation::Telechargement(p.outils.telecharger(&fichier.url, &part, fichier.taille))
            };
            self.en_cours = Some(FichierEnCours { element, fichier, operation });
        }
    }

    fn avancer_les_fichiers(&mut self, ecoule: f64) {
        let Some(mut c) = self.en_cours.take() else { return };
        let (fin, part) = {
            let p = self.partage.borrow();
            (final_(&p.racine, &c.fichier), partiel(&p.racine, &c.fichier))
        };
        match &mut c.operation {
            Operation::Telechargement(t) => match t.sonder(ecoule) {
                Avancement::EnCours(_) => self.en_cours = Some(c),
                Avancement::Fini(()) => {
                    let mut p = self.partage.borrow_mut();
                    p.phases.insert(c.element, Phase::Verification);
                    c.operation = Operation::Verification(p.outils.empreinte(&part));
                    self.en_cours = Some(c);
                }
                Avancement::Echec(m) => {
                    self.echec(c.element, format!("Téléchargement interrompu ({m}). Il reprendra là où il s'est arrêté."));
                }
            },
            Operation::Verification(t) => match t.sonder(ecoule) {
                Avancement::EnCours(_) => self.en_cours = Some(c),
                Avancement::Fini(empreinte) if empreinte == c.fichier.sha256 => match std::fs::rename(&part, &fin) {
                    Ok(()) => {
                        if !self.a_faire.iter().any(|(e, _)| *e == c.element) {
                            self.partage.borrow_mut().phases.remove(&c.element);
                        }
                    }
                    Err(e) => self.echec(c.element, format!("{} : {e}", fin.display())),
                },
                Avancement::Fini(empreinte) => {
                    let _ = std::fs::remove_file(&part);
                    self.echec(
                        c.element,
                        format!("sha256 incorrect pour {} (reçu {empreinte}, attendu {}). Le fichier est supprimé ; il sera retéléchargé au prochain essai.", c.fichier.chemin, c.fichier.sha256),
                    );
                }
                Avancement::Echec(m) => self.echec(c.element, format!("Vérification du sha256 impossible ({m}).")),
            },
        }
        self.suivant();
    }

    fn avancer_le_python(&mut self, ecoule: f64) {
        let Some((i, t)) = &mut self.python else { return };
        let i = *i;
        match t.sonder(ecoule) {
            Avancement::EnCours(_) => {}
            Avancement::Fini(()) => {
                self.python = None;
                let mut p = self.partage.borrow_mut();
                p.phases.remove(&i);
                p.place_du_python = None;
                if let Contenu::Python { pyproject, verrou } = &p.catalogue.0[i].contenu {
                    let temoin = empreinte_du_projet(pyproject, verrou);
                    let chemin = p.dossier_python().join("pret");
                    if let Err(e) = std::fs::write(&chemin, temoin) {
                        drop(p);
                        self.echec(i, format!("{} : {e}", chemin.display()));
                    }
                }
            }
            Avancement::Echec(m) => {
                self.python = None;
                self.partage.borrow_mut().place_du_python = None;
                self.echec(i, m);
            }
        }
    }

    fn avancer_claude(&mut self, ecoule: f64) {
        let Some((i, t)) = &mut self.claude else { return };
        let i = *i;
        let resultat = match t.sonder(ecoule) {
            Avancement::EnCours(_) => return,
            Avancement::Fini(e) => e,
            Avancement::Echec(m) => EtatDeClaude::Echec(m),
        };
        self.claude = None;
        let mut p = self.partage.borrow_mut();
        p.phases.remove(&i);
        p.claude = Some(resultat);
    }
}

impl crate::adaptateurs::Tache<()> for Installation {
    fn sonder(&mut self, ecoule: f64) -> Avancement<()> {
        self.avancer_les_fichiers(ecoule);
        self.avancer_le_python(ecoule);
        self.avancer_claude(ecoule);
        if self.en_cours.is_some() || self.python.is_some() || self.claude.is_some() {
            return Avancement::EnCours(0.0);
        }
        if self.echecs.is_empty() { Avancement::Fini(()) } else { Avancement::Echec(self.echecs.join("\n")) }
    }

    fn arreter(&mut self) {
        let mut p = self.partage.borrow_mut();
        if let Some(c) = &mut self.en_cours {
            match &mut c.operation {
                Operation::Telechargement(t) => t.arreter(),
                Operation::Verification(t) => t.arreter(),
            }
        }
        if let Some((_, t)) = &mut self.python {
            t.arreter();
        }
        if let Some((_, t)) = &mut self.claude {
            t.arreter();
        }
        p.phases.clear();
        p.place_du_python = None;
        self.en_cours = None;
        self.python = None;
        self.claude = None;
        self.a_faire.clear();
    }
}
