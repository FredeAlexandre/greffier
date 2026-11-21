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

/// Une capture dont le temps va `vitesse` fois plus vite que le temps écoulé. Elle écrit au fil
/// de l'eau un petit MP4 fragmenté sans image ni son (une piste, un fragment par sondage), dont
/// la durée se relit comme celle d'un vrai enregistrement, même coupé. Sa durée finale vaut au
/// moins `duree_minimale`.
pub struct FausseCapture {
    vitesse: f64,
    duree_minimale: f64,
    duree: f64,
    fichier: Option<PathBuf>,
    /// Millisecondes déjà écrites dans les fragments, et numéro du prochain fragment.
    ecrit: u64,
    fragment: u32,
    autorisations: Autorisations,
    /// La capture s'arrête d'elle-même à cet instant, avec ce message.
    panne: Option<(f64, String)>,
    /// `arreter` échoue avec ce message (le fichier garde les fragments déjà écrits).
    echec_a_l_arret: Option<String>,
}

impl FausseCapture {
    pub fn new(vitesse: f64, duree_minimale: f64) -> FausseCapture {
        FausseCapture { vitesse, duree_minimale, duree: 0.0, fichier: None, ecrit: 0, fragment: 1, autorisations: Autorisations::ACCORDEES, panne: None, echec_a_l_arret: None }
    }

    /// La capture s'arrête d'elle-même après `duree` secondes enregistrées, comme quand macOS
    /// coupe le flux : elle n'avance plus et signale `message`.
    pub fn en_panne_a(mut self, duree: f64, message: &str) -> FausseCapture {
        self.panne = Some((duree, message.into()));
        self
    }

    /// L'arrêt échoue avec `message`, comme un fichier qui ne peut pas être fini.
    pub fn en_echec_a_l_arret(mut self, message: &str) -> FausseCapture {
        self.echec_a_l_arret = Some(message.into());
        self
    }

    /// Les autorisations de départ. Une demande est toujours acceptée (« Autoriser ») ; une
    /// autorisation refusée le reste, comme sous macOS.
    pub fn avec_autorisations(mut self, autorisations: Autorisations) -> FausseCapture {
        self.autorisations = autorisations;
        self
    }

    /// Ajoute un fragment jusqu'à la durée actuelle.
    fn ecrire_jusqu_ici(&mut self) -> Result<(), String> {
        let Some(fichier) = &self.fichier else { return Ok(()) };
        let ms = (self.duree * 1000.0).round() as u64;
        if ms <= self.ecrit {
            return Ok(());
        }
        let octets = mp4::fragment(self.fragment, self.ecrit, (ms - self.ecrit) as u32);
        use std::io::Write;
        std::fs::OpenOptions::new().append(true).open(fichier).and_then(|mut f| f.write_all(&octets)).map_err(|e| e.to_string())?;
        self.ecrit = ms;
        self.fragment += 1;
        Ok(())
    }
}

impl Capture for FausseCapture {
    fn autorisations(&self) -> Autorisations {
        self.autorisations
    }

    fn demander_autorisation(&mut self, autorisation: Autorisation) {
        let etat = self.autorisations.etat_mut(autorisation);
        if *etat == EtatDAutorisation::ADemander {
            *etat = EtatDAutorisation::Accordee;
        }
    }

    fn demarrer(&mut self, demande: DemandeDeCapture) -> Result<(), String> {
        if let Some(parent) = demande.fichier.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(&demande.fichier, mp4::debut()).map_err(|e| e.to_string())?;
        self.duree = 0.0;
        self.ecrit = 0;
        self.fragment = 1;
        self.fichier = Some(demande.fichier);
        Ok(())
    }

    fn sonder(&mut self, ecoule: f64) -> EtatDeCapture {
        let mut erreur = None;
        if self.fichier.is_some() {
            self.duree += ecoule * self.vitesse;
            if let Some((fin, message)) = &self.panne
                && self.duree >= *fin
            {
                self.duree = *fin;
                erreur = Some(message.clone());
            }
            let _ = self.ecrire_jusqu_ici();
        }
        EtatDeCapture { duree: self.duree, niveaux: Niveaux { micro: 0.5, son_du_mac: 0.5 }, erreur }
    }

    fn arreter(&mut self) -> Result<f64, String> {
        if let Some(message) = &self.echec_a_l_arret {
            self.fichier = None;
            return Err(message.clone());
        }
        self.duree = self.duree.max(self.duree_minimale);
        self.ecrire_jusqu_ici()?;
        self.fichier.take().ok_or("aucun enregistrement en cours")?;
        Ok(self.duree)
    }
}

/// Les boîtes du MP4 fragmenté de la fausse capture : une piste au millième de seconde.
mod mp4 {
    fn boite(nom: &[u8; 4], contenu: &[u8]) -> Vec<u8> {
        let mut b = ((contenu.len() + 8) as u32).to_be_bytes().to_vec();
        b.extend_from_slice(nom);
        b.extend_from_slice(contenu);
        b
    }

    fn entiers(v: &[u32]) -> Vec<u8> {
        v.iter().flat_map(|x| x.to_be_bytes()).collect()
    }

    /// `ftyp` et `moov` : durée inconnue, une piste (identifiant 1, échelle 1000).
    pub fn debut() -> Vec<u8> {
        let matrice = [0x0001_0000, 0, 0, 0, 0x0001_0000, 0, 0, 0, 0x4000_0000];
        let mut mvhd = entiers(&[0, 0, 0, 1000, 0, 0x0001_0000, 0x0100_0000, 0, 0]);
        mvhd.extend(entiers(&matrice));
        mvhd.extend(entiers(&[0, 0, 0, 0, 0, 0, 2]));
        let mut tkhd = entiers(&[3, 0, 0, 1, 0, 0, 0, 0, 0, 0]);
        tkhd.extend(entiers(&matrice));
        tkhd.extend(entiers(&[0, 0]));
        let mdhd = entiers(&[0, 0, 0, 1000, 0, 0x55c4_0000]);
        let hdlr = [entiers(&[0, 0]), b"soun".to_vec(), entiers(&[0, 0, 0]), b"Greffier\0".to_vec()].concat();
        let mdia = [boite(b"mdhd", &mdhd), boite(b"hdlr", &hdlr)].concat();
        let trak = [boite(b"tkhd", &tkhd), boite(b"mdia", &mdia)].concat();
        let mvex = boite(b"trex", &entiers(&[0, 1, 1, 0, 0, 0]));
        let moov = [boite(b"mvhd", &mvhd), boite(b"trak", &trak), boite(b"mvex", &mvex)].concat();
        [boite(b"ftyp", &[b"iso5".to_vec(), entiers(&[0]), b"iso5iso6mp41".to_vec()].concat()), boite(b"moov", &moov)].concat()
    }

    /// Un fragment d'un échantillon de `duree` ms commençant à `debut` ms, et son `mdat` vide.
    pub fn fragment(numero: u32, debut: u64, duree: u32) -> Vec<u8> {
        let tfhd = entiers(&[0x0002_0000, 1]);
        let tfdt = [entiers(&[0x0100_0000]), debut.to_be_bytes().to_vec()].concat();
        let trun = entiers(&[0x0000_0100, 1, duree]);
        let traf = [boite(b"tfhd", &tfhd), boite(b"tfdt", &tfdt), boite(b"trun", &trun)].concat();
        let moof = [boite(b"mfhd", &entiers(&[0, numero])), boite(b"traf", &traf)].concat();
        [boite(b"moof", &moof), boite(b"mdat", &[])].concat()
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

/// Une corbeille dans un dossier temporaire (effacé avec la dernière copie) : les dossiers jetés
/// y sont déplacés et retenus. Les copies partagent la même corbeille, pour regarder dedans après
/// l'avoir branchée.
#[derive(Clone)]
pub struct FausseCorbeille(std::sync::Arc<Poubelle>);

struct Poubelle {
    dossier: PathBuf,
    panne: Option<String>,
    jetes: std::sync::Mutex<Vec<PathBuf>>,
}

impl Drop for Poubelle {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dossier);
    }
}

impl FausseCorbeille {
    pub fn new() -> FausseCorbeille {
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dossier = std::env::temp_dir().join(format!("greffier-corbeille-{}-{n}", std::process::id()));
        FausseCorbeille(std::sync::Arc::new(Poubelle { dossier, panne: None, jetes: Default::default() }))
    }

    /// Une corbeille qui refuse tout, avec ce message.
    pub fn en_panne(self, message: &str) -> FausseCorbeille {
        let dossier = self.0.dossier.clone();
        FausseCorbeille(std::sync::Arc::new(Poubelle { dossier, panne: Some(message.into()), jetes: Default::default() }))
    }

    /// Où sont les dossiers jetés (sous leur nom).
    pub fn dossier(&self) -> &Path {
        &self.0.dossier
    }

    /// Les chemins jetés, dans l'ordre.
    pub fn jetes(&self) -> Vec<PathBuf> {
        self.0.jetes.lock().expect("corbeille").clone()
    }
}

impl Default for FausseCorbeille {
    fn default() -> FausseCorbeille {
        FausseCorbeille::new()
    }
}

impl Corbeille for FausseCorbeille {
    fn jeter(&self, chemin: &Path) -> Result<(), String> {
        if let Some(m) = &self.0.panne {
            return Err(m.clone());
        }
        let nom = chemin.file_name().ok_or("rien à jeter")?;
        std::fs::create_dir_all(&self.0.dossier).map_err(|e| e.to_string())?;
        std::fs::rename(chemin, self.0.dossier.join(nom)).map_err(|e| format!("{} : {e}", chemin.display()))?;
        self.0.jetes.lock().expect("corbeille").push(chemin.to_path_buf());
        Ok(())
    }
}

/// Une fausse mise en MP4 : un fichier qui commence comme un MP4 (boîte `ftyp`) est copié tel
/// quel, le reste est refusé.
#[derive(Clone, Copy, Debug, Default)]
pub struct FausseMiseEnMp4;

impl MiseEnMp4 for FausseMiseEnMp4 {
    fn en_mp4(&self, de: &Path, vers: &Path) -> Result<(), String> {
        let octets = std::fs::read(de).map_err(|e| format!("{} : {e}", de.display()))?;
        if octets.get(4..8) != Some(b"ftyp".as_slice()) {
            return Err(format!("{} : pas un conteneur MP4", de.display()));
        }
        std::fs::write(vers, octets).map_err(|e| format!("{} : {e}", vers.display()))
    }
}
