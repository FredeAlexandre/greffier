//! Les faux adaptateurs : ils avancent avec le temps qu'on leur donne et rendent des résultats
//! écrits à la main. Ils servent aux tests du cœur et au prototype (avec les données fictives de
//! [`crate::fictif`]).

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use crate::adaptateurs::*;
use crate::reunion::{Etape, Mot, TourDeParole};
use crate::ressources::{EtatDeClaude, Outils, sha256};
use crate::temps::DateHeure;

/// Une tâche qui dure `duree` secondes de temps écoulé, puis rend son résultat.
pub struct FausseTache<T> {
    duree: f64,
    progres: f64,
    resultat: Option<Result<T, String>>,
}

impl<T> FausseTache<T> {
    pub fn new(duree: f64, resultat: Result<T, String>) -> FausseTache<T> {
        FausseTache { duree, progres: 0.0, resultat: Some(resultat) }
    }

    pub fn boite(duree: f64, resultat: Result<T, String>) -> TacheDe<T>
    where
        T: 'static,
    {
        Box::new(FausseTache::new(duree, resultat))
    }
}

impl<T> Tache<T> for FausseTache<T> {
    fn sonder(&mut self, ecoule: f64) -> Avancement<T> {
        if self.duree > 0.0 {
            self.progres += ecoule / self.duree;
        }
        if self.duree > 0.0 && self.progres < 1.0 {
            return Avancement::EnCours(self.progres);
        }
        match self.resultat.take() {
            Some(Ok(r)) => Avancement::Fini(r),
            Some(Err(e)) => Avancement::Echec(e),
            None => Avancement::Echec("tâche déjà rendue".into()),
        }
    }
}

/// Une capture dont le temps va `vitesse` fois plus vite que le temps écoulé. Elle écrit un
/// petit fichier à la place de l'enregistrement ; sa durée finale vaut au moins `duree_minimale`.
pub struct FausseCapture {
    vitesse: f64,
    duree_minimale: f64,
    duree: f64,
    fichier: Option<PathBuf>,
}

impl FausseCapture {
    pub fn new(vitesse: f64, duree_minimale: f64) -> FausseCapture {
        FausseCapture { vitesse, duree_minimale, duree: 0.0, fichier: None }
    }
}

impl Capture for FausseCapture {
    fn demarrer(&mut self, demande: DemandeDeCapture) -> Result<(), String> {
        if let Some(parent) = demande.fichier.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(&demande.fichier, b"faux enregistrement\n").map_err(|e| e.to_string())?;
        self.duree = 0.0;
        self.fichier = Some(demande.fichier);
        Ok(())
    }

    fn sonder(&mut self, ecoule: f64) -> EtatDeCapture {
        if self.fichier.is_some() {
            self.duree += ecoule * self.vitesse;
        }
        EtatDeCapture { duree: self.duree, niveaux: Niveaux { micro: 0.5, son_du_mac: 0.5 } }
    }

    fn arreter(&mut self) -> Result<f64, String> {
        self.fichier.take().ok_or("aucun enregistrement en cours")?;
        Ok(self.duree.max(self.duree_minimale))
    }
}

type Production<D, T> = Box<dyn FnMut(&D) -> Result<T, String>>;

/// Rend, après `duree` secondes, les mots que produit sa fonction.
pub struct FauxTranscripteur {
    duree: f64,
    produire: Production<DemandeDeTranscription, Vec<Mot>>,
}

impl FauxTranscripteur {
    pub fn new(duree: f64, produire: impl FnMut(&DemandeDeTranscription) -> Result<Vec<Mot>, String> + 'static) -> FauxTranscripteur {
        FauxTranscripteur { duree, produire: Box::new(produire) }
    }
    pub fn fixe(duree: f64, mots: Vec<Mot>) -> FauxTranscripteur {
        FauxTranscripteur::new(duree, move |_| Ok(mots.clone()))
    }
}

impl Transcripteur for FauxTranscripteur {
    fn transcrire(&mut self, demande: DemandeDeTranscription) -> TacheDe<Vec<Mot>> {
        FausseTache::boite(self.duree, (self.produire)(&demande))
    }
}

/// Rend, après `duree` secondes, les tours de parole que produit sa fonction.
pub struct FauxSeparateur {
    duree: f64,
    produire: Production<DemandeDeSeparation, Vec<TourDeParole>>,
}

impl FauxSeparateur {
    pub fn new(duree: f64, produire: impl FnMut(&DemandeDeSeparation) -> Result<Vec<TourDeParole>, String> + 'static) -> FauxSeparateur {
        FauxSeparateur { duree, produire: Box::new(produire) }
    }
    pub fn fixe(duree: f64, tours: Vec<TourDeParole>) -> FauxSeparateur {
        FauxSeparateur::new(duree, move |_| Ok(tours.clone()))
    }
}

impl Separateur for FauxSeparateur {
    fn separer(&mut self, demande: DemandeDeSeparation) -> TacheDe<Vec<TourDeParole>> {
        FausseTache::boite(self.duree, (self.produire)(&demande))
    }
}

/// Un agent dont la relecture et le bilan sont donnés par deux fonctions. Le bilan (Markdown)
/// est écrit dans le fichier demandé dès l'appel, comme le ferait l'agent.
pub struct FauxAgent {
    duree_relecture: f64,
    relire: Production<DemandeDeRelecture, ResultatDeRelecture>,
    duree_bilan: f64,
    bilan: Production<DemandeDeBilan, (String, ResultatDuBilan)>,
}

impl FauxAgent {
    pub fn new(
        duree_relecture: f64,
        relire: impl FnMut(&DemandeDeRelecture) -> Result<ResultatDeRelecture, String> + 'static,
        duree_bilan: f64,
        bilan: impl FnMut(&DemandeDeBilan) -> Result<(String, ResultatDuBilan), String> + 'static,
    ) -> FauxAgent {
        FauxAgent { duree_relecture, relire: Box::new(relire), duree_bilan, bilan: Box::new(bilan) }
    }
}

impl Agent for FauxAgent {
    fn relire(&mut self, demande: DemandeDeRelecture) -> TacheDe<ResultatDeRelecture> {
        FausseTache::boite(self.duree_relecture, (self.relire)(&demande))
    }

    fn faire_le_bilan(&mut self, demande: DemandeDeBilan) -> TacheDe<ResultatDuBilan> {
        let resultat = (self.bilan)(&demande).and_then(|(md, resultat)| {
            std::fs::write(&demande.fichier_du_bilan, md).map_err(|e| format!("{} : {e}", demande.fichier_du_bilan.display()))?;
            Ok(resultat)
        });
        FausseTache::boite(self.duree_bilan, resultat)
    }
}

/// Des ressources prêtes, ou absentes jusqu'à ce qu'une installation de `duree` secondes aboutisse,
/// ou figées dans les états donnés (les scènes de l'interface).
pub struct FaussesRessources {
    pretes: Rc<Cell<bool>>,
    duree_installation: f64,
    echec: Option<String>,
    fixes: Option<Vec<Ressource>>,
}

impl FaussesRessources {
    pub fn pretes() -> FaussesRessources {
        FaussesRessources { pretes: Rc::new(Cell::new(true)), duree_installation: 0.0, echec: None, fixes: None }
    }
    pub fn absentes(duree_installation: f64) -> FaussesRessources {
        FaussesRessources { pretes: Rc::new(Cell::new(false)), duree_installation, echec: None, fixes: None }
    }
    /// Toujours ces ressources ; installer et vérifier n'y changent rien.
    pub fn fixes(ressources: Vec<Ressource>) -> FaussesRessources {
        FaussesRessources { fixes: Some(ressources), ..FaussesRessources::pretes() }
    }
    /// L'installation échoue avec ce message.
    pub fn en_echec(mut self, message: &str) -> FaussesRessources {
        self.echec = Some(message.into());
        self
    }
}

const NOMS_DES_RESSOURCES: [&str; 4] = ["Whisper large-v3", "Silero VAD v5.1.2", "pyannote community-1", "Python privé"];

struct Installation {
    tache: FausseTache<()>,
    pretes: Rc<Cell<bool>>,
}

impl Tache<()> for Installation {
    fn sonder(&mut self, ecoule: f64) -> Avancement<()> {
        let a = self.tache.sonder(ecoule);
        if a == Avancement::Fini(()) {
            self.pretes.set(true);
        }
        a
    }
}

impl Ressources for FaussesRessources {
    fn etat(&mut self) -> Vec<Ressource> {
        if let Some(fixes) = &self.fixes {
            return fixes.clone();
        }
        let etat = if self.pretes.get() { EtatDeRessource::Prete } else { EtatDeRessource::Absente };
        let ressource = |nom: &str, etapes: Vec<Etape>, etat: EtatDeRessource| Ressource { nom: nom.into(), detail: String::new(), etat, taille: 0, total: 0, aide: None, etapes };
        NOMS_DES_RESSOURCES
            .iter()
            .map(|n| ressource(n, vec![Etape::Transcription], etat.clone()))
            .chain([ressource("Claude Code", vec![Etape::Relecture, Etape::Bilan], EtatDeRessource::Prete)])
            .collect()
    }

    fn installer(&mut self) -> TacheDe<()> {
        let resultat = match &self.echec {
            Some(e) => Err(e.clone()),
            None => Ok(()),
        };
        Box::new(Installation { tache: FausseTache::new(self.duree_installation, resultat), pretes: self.pretes.clone() })
    }

    fn verifier(&mut self) -> TacheDe<()> {
        FausseTache::boite(0.0, Ok(()))
    }
}

/// Les outils des ressources, sans réseau ni uv : des téléchargements servis depuis la mémoire à
/// `octets_par_seconde` (écrits peu à peu dans le fichier partiel, comme curl), des empreintes
/// vraies, un Python et une CLI `claude` au résultat donné.
pub struct FauxOutils {
    octets_par_seconde: f64,
    contenus: HashMap<String, Vec<u8>>,
    coupures: HashMap<String, u64>,
    corrompus: HashSet<String>,
    duree_d_empreinte: f64,
    python: (f64, Result<(), String>),
    claude: EtatDeClaude,
}

impl FauxOutils {
    pub fn new(octets_par_seconde: f64) -> FauxOutils {
        FauxOutils {
            octets_par_seconde,
            contenus: HashMap::new(),
            coupures: HashMap::new(),
            corrompus: HashSet::new(),
            duree_d_empreinte: 0.0,
            python: (0.0, Ok(())),
            claude: EtatDeClaude::Connectee { version: "2.1.0".into(), compte: "claude.ai".into() },
        }
    }
    pub fn servir(mut self, url: &str, contenu: &[u8]) -> FauxOutils {
        self.contenus.insert(url.into(), contenu.to_vec());
        self
    }
    /// Le prochain téléchargement de `url` s'interrompt une fois le fichier partiel à `octets`.
    pub fn couper_une_fois(mut self, url: &str, octets: u64) -> FauxOutils {
        self.coupures.insert(url.into(), octets);
        self
    }
    /// Le prochain téléchargement de `url` reçoit des octets faux.
    pub fn corrompre_une_fois(mut self, url: &str) -> FauxOutils {
        self.corrompus.insert(url.into());
        self
    }
    pub fn duree_d_empreinte(mut self, secondes: f64) -> FauxOutils {
        self.duree_d_empreinte = secondes;
        self
    }
    pub fn python(mut self, duree: f64, resultat: Result<(), String>) -> FauxOutils {
        self.python = (duree, resultat);
        self
    }
    pub fn claude(mut self, etat: EtatDeClaude) -> FauxOutils {
        self.claude = etat;
        self
    }
}

struct FauxTelechargement {
    contenu: Vec<u8>,
    partiel: PathBuf,
    octets_par_seconde: f64,
    position: f64,
    ecrit: u64,
    coupure: Option<u64>,
    corrompu: bool,
}

impl Tache<()> for FauxTelechargement {
    fn sonder(&mut self, ecoule: f64) -> Avancement<()> {
        let total = self.contenu.len() as u64;
        self.position += ecoule * self.octets_par_seconde;
        let cible = (self.position as u64).min(total).min(self.coupure.unwrap_or(u64::MAX)).max(self.ecrit);
        let mut octets = self.contenu[self.ecrit as usize..cible as usize].to_vec();
        if self.corrompu {
            octets.iter_mut().for_each(|o| *o = !*o);
        }
        let ecrit = std::fs::OpenOptions::new().create(true).append(true).open(&self.partiel).and_then(|mut f| std::io::Write::write_all(&mut f, &octets));
        if let Err(e) = ecrit {
            return Avancement::Echec(format!("{} : {e}", self.partiel.display()));
        }
        self.ecrit = cible;
        if self.coupure.is_some_and(|c| self.ecrit >= c) {
            return Avancement::Echec("connexion perdue".into());
        }
        if self.ecrit >= total {
            return Avancement::Fini(());
        }
        Avancement::EnCours(self.ecrit as f64 / total.max(1) as f64)
    }
}

impl Outils for FauxOutils {
    fn telecharger(&mut self, url: &str, partiel: &Path, _taille: u64) -> TacheDe<()> {
        let Some(contenu) = self.contenus.get(url).cloned() else { return FausseTache::boite(0.0, Err(format!("{url} : 404"))) };
        let deja = std::fs::metadata(partiel).map_or(0, |m| m.len()).min(contenu.len() as u64);
        Box::new(FauxTelechargement {
            contenu,
            partiel: partiel.to_path_buf(),
            octets_par_seconde: self.octets_par_seconde,
            position: deja as f64,
            ecrit: deja,
            coupure: self.coupures.remove(url),
            corrompu: self.corrompus.remove(url),
        })
    }

    fn empreinte(&mut self, fichier: &Path) -> TacheDe<String> {
        let resultat = std::fs::read(fichier).map(|o| sha256(&o)).map_err(|e| format!("{} : {e}", fichier.display()));
        FausseTache::boite(self.duree_d_empreinte, resultat)
    }

    fn installer_python(&mut self, _projet: &Path, _dossier: &Path) -> TacheDe<()> {
        FausseTache::boite(self.python.0, self.python.1.clone())
    }

    fn verifier_claude(&mut self) -> TacheDe<EtatDeClaude> {
        FausseTache::boite(0.0, Ok(self.claude.clone()))
    }
}

/// Une horloge arrêtée.
pub struct FausseHorloge(pub DateHeure);

impl Horloge for FausseHorloge {
    fn maintenant(&self) -> DateHeure {
        self.0
    }
}
