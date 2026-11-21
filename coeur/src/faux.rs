//! Les faux adaptateurs : ils avancent avec le temps qu'on leur donne et rendent des résultats
//! écrits à la main. Ils servent aux tests du cœur et au prototype (avec les données fictives de
//! [`crate::fictif`]).

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;

use crate::adaptateurs::*;
use crate::reunion::{Mot, TourDeParole};
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

/// Des ressources prêtes, ou absentes jusqu'à ce qu'une installation de `duree` secondes aboutisse.
pub struct FaussesRessources {
    pretes: Rc<Cell<bool>>,
    duree_installation: f64,
    echec: Option<String>,
}

impl FaussesRessources {
    pub fn pretes() -> FaussesRessources {
        FaussesRessources { pretes: Rc::new(Cell::new(true)), duree_installation: 0.0, echec: None }
    }
    pub fn absentes(duree_installation: f64) -> FaussesRessources {
        FaussesRessources { pretes: Rc::new(Cell::new(false)), duree_installation, echec: None }
    }
    /// L'installation échoue avec ce message.
    pub fn en_echec(mut self, message: &str) -> FaussesRessources {
        self.echec = Some(message.into());
        self
    }
}

const NOMS_DES_RESSOURCES: [&str; 4] = ["whisper large-v3", "Silero VAD", "pyannote community-1", "Python privé"];

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
        let etat = if self.pretes.get() { EtatDeRessource::Prete } else { EtatDeRessource::Absente };
        NOMS_DES_RESSOURCES.iter().map(|n| Ressource { nom: (*n).into(), etat: etat.clone() }).collect()
    }

    fn installer(&mut self) -> TacheDe<()> {
        let resultat = match &self.echec {
            Some(e) => Err(e.clone()),
            None => Ok(()),
        };
        Box::new(Installation { tache: FausseTache::new(self.duree_installation, resultat), pretes: self.pretes.clone() })
    }
}

/// Une horloge arrêtée.
pub struct FausseHorloge(pub DateHeure);

impl Horloge for FausseHorloge {
    fn maintenant(&self) -> DateHeure {
        self.0
    }
}
