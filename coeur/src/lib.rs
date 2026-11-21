//! Le cœur de Greffier, sans interface.
//!
//! [`Greffier`] possède l'état des réunions, de l'enregistrement et de la file de traitement. On le
//! pilote par [`Commande`]s ([`Greffier::executer`]) et on fait passer le temps avec
//! [`Greffier::avancer`] ; les deux rendent des [`Evenement`]s. L'interface ne fait que lire l'état
//! (`reunions`, `enregistrement`…) et appeler ces deux fonctions.
//!
//! Tout ce qui sort du processus passe par les [`adaptateurs`] : capture, transcripteur,
//! séparateur de locuteurs, agent, ressources (et l'horloge). Les ressources de l'app (modèles,
//! Python privé, vérification de `claude`) sont décrites dans [`ressources`].

#![forbid(unsafe_code)]

pub mod adaptateurs;
mod attribution;
pub mod bilan;
pub mod dossier;
pub mod faux;
pub mod fictif;
pub mod lexique;
pub mod mac;
pub mod ressources;
mod reunion;
mod surveillance;
mod temps;

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::path::PathBuf;

use adaptateurs::*;
pub use dossier::{DossierDeReunions, Role};
pub use reunion::*;
pub use temps::*;

/// Les réglages qui changent le comportement du cœur.
#[derive(Clone, Debug, PartialEq)]
pub struct Reglages {
    /// Mettre la réunion dans la file de traitement dès l'arrêt de l'enregistrement.
    pub traiter_a_l_arret: bool,
    pub sources: Sources,
}

impl Default for Reglages {
    fn default() -> Reglages {
        Reglages { traiter_a_l_arret: true, sources: Sources::default() }
    }
}

/// Comment l'utilisateur tranche un passage à vérifier.
#[derive(Clone, Debug, PartialEq)]
pub enum Tranche {
    AccepterHypothese,
    GarderOriginal,
    Corriger(String),
    /// Remet le passage en attente.
    Rouvrir,
}

/// Tout ce qu'on peut demander au cœur.
#[derive(Clone, Debug, PartialEq)]
pub enum Commande {
    DemarrerEnregistrement,
    ArreterEnregistrement,
    PoserRepere,
    /// Le nombre de locuteurs indiqué pendant l'enregistrement (`None` : détecté).
    NombreDeLocuteursEnregistres(Option<u8>),
    /// Le nombre de locuteurs indiqué avant le traitement (`None` : détecté).
    NombreDeLocuteurs { reunion: IdReunion, nombre: Option<u8> },
    /// Met la réunion dans la file, à partir de la première étape qui lui manque.
    Traiter(IdReunion),
    /// Remet la réunion dans la file à partir de cette étape ; les suivantes sont refaites aussi.
    Refaire { reunion: IdReunion, etape: Etape },
    /// Arrête le traitement (ou sort la réunion de la file) : elle redevient à traiter.
    ArreterTraitement(IdReunion),
    Supprimer(IdReunion),
    NommerLocuteur { reunion: IdReunion, locuteur: usize, nom: String },
    /// `locuteur` est la même voix que `dans`, qui le remplace partout.
    FusionnerLocuteurs { reunion: IdReunion, locuteur: usize, dans: usize },
    Trancher { reunion: IdReunion, passage: usize, tranche: Tranche },
    /// Coche ou décoche la `action`-ième case du bilan.
    CocherAction { reunion: IdReunion, action: usize, faite: bool },
    /// La réunion a été ouverte : elle n'est plus nouvelle.
    MarquerLue(IdReunion),
    ChangerReglages(Reglages),
    /// Télécharge, installe et vérifie les ressources qui manquent.
    InstallerRessources,
    /// Refait les vérifications des ressources (la CLI `claude`) sans rien télécharger.
    VerifierRessources,
}

/// Ce qui s'est passé dans le cœur et que l'interface peut vouloir annoncer.
#[derive(Clone, Debug, PartialEq)]
pub enum Evenement {
    ReunionEnregistree(IdReunion),
    EtapeTerminee { reunion: IdReunion, etape: Etape },
    EtapeEnEchec { reunion: IdReunion, etape: Etape, message: String },
    BilanPret { reunion: IdReunion, titre: String },
    RessourcesInstallees,
    InstallationEnEchec(String),
    /// Un fichier de la réunion n'a pas pu être écrit dans le dossier de réunions.
    EcritureImpossible { reunion: IdReunion, message: String },
    /// Un fichier de la réunion a changé hors de l'app : elle a été relue.
    ReunionRelue(IdReunion),
    /// Un dossier de réunion est apparu hors de l'app.
    ReunionApparue(IdReunion),
    /// Le dossier de la réunion a disparu hors de l'app : elle n'est plus dans la liste.
    ReunionDisparue(IdReunion),
}

/// Pourquoi une commande n'a pas été exécutée. L'état n'a pas changé.
#[derive(Clone, Debug, PartialEq)]
pub enum Refus {
    ReunionInconnue(IdReunion),
    Impossible(String),
}

impl fmt::Display for Refus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refus::ReunionInconnue(id) => write!(f, "réunion inconnue ({id})"),
            Refus::Impossible(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for Refus {}

fn impossible<T>(message: impl Into<String>) -> Result<T, Refus> {
    Err(Refus::Impossible(message.into()))
}

/// L'enregistrement en cours.
#[derive(Clone, Debug, PartialEq)]
pub struct Enregistrement {
    pub debut: DateHeure,
    /// Durée enregistrée, en secondes.
    pub duree: f64,
    pub niveaux: Niveaux,
    /// Instants des repères posés.
    pub reperes: Vec<f64>,
    pub nombre_de_locuteurs: Option<u8>,
    pub nom_du_dossier: String,
}

enum Partie<T> {
    EnCours(TacheDe<T>),
    Fini(T),
}

impl<T> Partie<T> {
    fn sonder(&mut self, ecoule: f64) -> Result<f64, String> {
        let a = match self {
            Partie::EnCours(t) => t.sonder(ecoule),
            Partie::Fini(_) => return Ok(1.0),
        };
        match a {
            Avancement::EnCours(p) => Ok(p),
            Avancement::Fini(v) => {
                *self = Partie::Fini(v);
                Ok(1.0)
            }
            Avancement::Echec(e) => Err(e),
        }
    }
    fn arreter(&mut self) {
        if let Partie::EnCours(t) = self {
            t.arreter();
        }
    }
    fn fini(&self) -> bool {
        matches!(self, Partie::Fini(_))
    }
    fn prendre(self) -> Option<T> {
        match self {
            Partie::Fini(v) => Some(v),
            Partie::EnCours(_) => None,
        }
    }
}

enum Travail {
    /// La transcription et la séparation des locuteurs tournent en parallèle.
    Transcription { mots: Partie<Vec<Mot>>, tours: Partie<Vec<TourDeParole>> },
    Relecture(Partie<ResultatDeRelecture>),
    Bilan { resultat: Partie<ResultatDuBilan>, fichier: PathBuf },
}

impl Travail {
    fn etape(&self) -> Etape {
        match self {
            Travail::Transcription { .. } => Etape::Transcription,
            Travail::Relecture(_) => Etape::Relecture,
            Travail::Bilan { .. } => Etape::Bilan,
        }
    }
    fn arreter(&mut self) {
        match self {
            Travail::Transcription { mots, tours } => {
                mots.arreter();
                tours.arreter();
            }
            Travail::Relecture(p) => p.arreter(),
            Travail::Bilan { resultat, .. } => resultat.arreter(),
        }
    }
}

struct Traitement {
    reunion: IdReunion,
    travail: Travail,
}

enum Produit {
    Transcription(Vec<Mot>, Vec<TourDeParole>),
    Relecture(ResultatDeRelecture),
    Bilan(ResultatDuBilan, PathBuf),
}

/// Le cœur de Greffier.
pub struct Greffier {
    dossier: DossierDeReunions,
    adaptateurs: Adaptateurs,
    reglages: Reglages,
    /// Les plus récentes d'abord.
    reunions: Vec<Reunion>,
    prochain_id: IdReunion,
    enregistrement: Option<Enregistrement>,
    /// Les réunions en attente, dans l'ordre où elles seront traitées.
    file: VecDeque<IdReunion>,
    traitement: Option<Traitement>,
    installation: Option<TacheDe<()>>,
    verification: Option<TacheDe<()>>,
    /// L'état des ressources, relu à chaque avancée.
    ressources: Vec<Ressource>,
    /// L'empreinte de chaque dossier de réunion au dernier passage du cœur (voir
    /// [`Greffier::relire_le_dossier`]).
    empreintes: HashMap<String, dossier::surveillance::Empreintes>,
}

impl Greffier {
    /// Le cœur sur un dossier de réunions, avec les réunions qu'il contient déjà.
    pub fn ouvrir(dossier: impl Into<PathBuf>, adaptateurs: Adaptateurs) -> Greffier {
        let dossier = DossierDeReunions::new(dossier);
        let reunions = dossier.lire_tout(1);
        Greffier::avec_reunions(dossier.racine(), adaptateurs, reunions)
    }

    /// Le cœur avec des réunions déjà connues (les données fictives du prototype).
    pub fn avec_reunions(dossier: impl Into<PathBuf>, adaptateurs: Adaptateurs, mut reunions: Vec<Reunion>) -> Greffier {
        let mut file = VecDeque::new();
        for r in &mut reunions {
            if let Statut::EnCours { etape, .. } = r.statut {
                r.statut = Statut::EnAttente(etape);
            }
            if let Statut::EnAttente(_) = r.statut {
                file.push_back(r.id);
            }
        }
        let prochain_id = reunions.iter().map(|r| r.id + 1).max().unwrap_or(1);
        let mut adaptateurs = adaptateurs;
        let ressources = adaptateurs.ressources.etat();
        let dossier = DossierDeReunions::new(dossier);
        let empreintes = dossier.toutes_les_empreintes();
        Greffier {
            dossier,
            adaptateurs,
            reglages: Reglages::default(),
            reunions,
            prochain_id,
            enregistrement: None,
            file,
            traitement: None,
            installation: None,
            verification: None,
            ressources,
            empreintes,
        }
    }

    // --- Lecture de l'état ---------------------------------------------------------------

    pub fn reunions(&self) -> &[Reunion] {
        &self.reunions
    }

    pub fn reunion(&self, id: IdReunion) -> Option<&Reunion> {
        self.reunions.iter().find(|r| r.id == id)
    }

    pub fn enregistrement(&self) -> Option<&Enregistrement> {
        self.enregistrement.as_ref()
    }

    pub fn reglages(&self) -> &Reglages {
        &self.reglages
    }

    pub fn dossier(&self) -> &DossierDeReunions {
        &self.dossier
    }

    /// Le rang d'une réunion dans la file : 0 pour celle en cours de traitement.
    pub fn rang(&self, id: IdReunion) -> Option<usize> {
        let en_cours = self.traitement.as_ref().map(|t| t.reunion);
        en_cours.into_iter().chain(self.file.iter().copied()).position(|x| x == id)
    }

    pub fn aujourd_hui(&self) -> DateHeure {
        self.adaptateurs.horloge.maintenant()
    }

    /// Les modèles, l'environnement Python et la vérification de `claude`, avec leur état et la
    /// place qu'ils prennent.
    pub fn ressources(&self) -> &[Ressource] {
        &self.ressources
    }

    pub fn installation_en_cours(&self) -> bool {
        self.installation.is_some()
    }

    pub fn verification_en_cours(&self) -> bool {
        self.verification.is_some()
    }

    /// Vrai tant que le temps compte : enregistrement, traitement, file, installation ou
    /// vérification des ressources.
    pub fn occupe(&self) -> bool {
        self.enregistrement.is_some() || self.traitement.is_some() || !self.file.is_empty() || self.installation.is_some() || self.verification.is_some()
    }

    // --- Commandes -----------------------------------------------------------------------

    pub fn executer(&mut self, commande: Commande) -> Result<Vec<Evenement>, Refus> {
        let mut ev = Vec::new();
        let touchee = match &commande {
            Commande::NombreDeLocuteurs { reunion, .. }
            | Commande::Refaire { reunion, .. }
            | Commande::NommerLocuteur { reunion, .. }
            | Commande::FusionnerLocuteurs { reunion, .. }
            | Commande::Trancher { reunion, .. }
            | Commande::CocherAction { reunion, .. } => Some(*reunion),
            Commande::Traiter(id) | Commande::ArreterTraitement(id) | Commande::MarquerLue(id) => Some(*id),
            _ => None,
        };
        match commande {
            Commande::DemarrerEnregistrement => self.demarrer_enregistrement()?,
            Commande::ArreterEnregistrement => {
                let e = self.arreter_enregistrement()?;
                if let Evenement::ReunionEnregistree(id) = e {
                    self.persister(id, &mut ev);
                }
                ev.insert(0, e);
            }
            Commande::PoserRepere => {
                let Some(e) = &mut self.enregistrement else { return impossible("aucun enregistrement en cours") };
                e.reperes.push(e.duree);
            }
            Commande::NombreDeLocuteursEnregistres(n) => {
                let Some(e) = &mut self.enregistrement else { return impossible("aucun enregistrement en cours") };
                e.nombre_de_locuteurs = n;
            }
            Commande::NombreDeLocuteurs { reunion, nombre } => self.reunion_mut(reunion)?.nombre_de_locuteurs = nombre,
            Commande::Traiter(id) => {
                let r = self.reunion_mut(id)?;
                let etape = match &r.statut {
                    Statut::Echec { etape, .. } => *etape,
                    Statut::ATraiter | Statut::EnregistrementInterrompu => match r.etape_faite {
                        None => Etape::Transcription,
                        Some(e) => match e.suivante() {
                            Some(s) => s,
                            None => return impossible("la réunion est déjà traitée"),
                        },
                    },
                    s => return impossible(format!("la réunion ne peut pas être traitée ({s:?})")),
                };
                self.mettre_en_file(id, etape);
            }
            Commande::Refaire { reunion, etape } => {
                let r = self.reunion_mut(reunion)?;
                if r.statut.dans_la_file() || r.statut == Statut::EnregistrementEnCours {
                    return impossible("la réunion est déjà dans la file");
                }
                if etape.precedente().is_some_and(|p| r.etape_faite < Some(p)) {
                    return impossible("les étapes précédentes ne sont pas faites");
                }
                r.etape_faite = etape.precedente();
                self.mettre_en_file(reunion, etape);
            }
            Commande::ArreterTraitement(id) => {
                let r = self.reunion_mut(id)?;
                if !r.statut.dans_la_file() {
                    return impossible("la réunion n'est pas en traitement");
                }
                r.statut = Statut::ATraiter;
                self.sortir_de_la_file(id);
            }
            Commande::Supprimer(id) => {
                self.reunion_mut(id)?;
                self.sortir_de_la_file(id);
                self.reunions.retain(|r| r.id != id);
            }
            Commande::NommerLocuteur { reunion, locuteur, nom } => {
                let l = locuteur_mut(self.reunion_mut(reunion)?, locuteur)?;
                l.nom = Some(nom);
                l.fusion_proposee = None;
            }
            Commande::FusionnerLocuteurs { reunion, locuteur, dans } => {
                let r = self.reunion_mut(reunion)?;
                locuteur_mut(r, dans)?;
                if r.locuteur_affiche(dans) == locuteur_mut(r, locuteur).map(|_| locuteur)? {
                    return impossible("un locuteur ne peut pas être fusionné dans lui-même");
                }
                let l = &mut r.locuteurs[locuteur];
                l.fusionne_dans = Some(dans);
                l.fusion_proposee = None;
            }
            Commande::Trancher { reunion, passage, tranche } => {
                let r = self.reunion_mut(reunion)?;
                let Some(p) = r.passages.get_mut(passage) else { return impossible("passage inconnu") };
                p.statut = match tranche {
                    Tranche::AccepterHypothese if p.hypothese.is_none() => return impossible("ce passage n'a pas d'hypothèse"),
                    Tranche::AccepterHypothese => StatutDuPassage::HypotheseAcceptee,
                    Tranche::GarderOriginal => StatutDuPassage::OriginalGarde,
                    Tranche::Corriger(texte) => StatutDuPassage::CorrigeALaMain(texte),
                    Tranche::Rouvrir => StatutDuPassage::EnAttente,
                };
            }
            Commande::CocherAction { reunion, action, faite } => {
                let r = self.reunion(reunion).ok_or(Refus::ReunionInconnue(reunion))?;
                if action >= bilan::nombre_de_cases(&r.bilan) {
                    return impossible("action inconnue");
                }
                // Le fichier du bilan est celui de l'agent : on n'y change que la case, et il fait foi.
                let nom = r.nom_du_dossier.clone();
                let fichier = self.dossier.cocher_dans_le_bilan(&nom, action, faite).map_err(Refus::Impossible)?;
                let r = self.reunion_mut(reunion)?;
                match fichier {
                    Some(md) => r.poser_bilan(md),
                    None => _ = bilan::cocher(&mut r.bilan, action, faite),
                }
                self.noter(&nom, &[Role::Bilan]);
            }
            Commande::MarquerLue(id) => self.reunion_mut(id)?.nouvelle = false,
            Commande::ChangerReglages(r) => self.reglages = r,
            Commande::InstallerRessources => {
                if self.installation.is_none() {
                    // L'installation refait aussi les vérifications.
                    if let Some(mut v) = self.verification.take() {
                        v.arreter();
                    }
                    self.installation = Some(self.adaptateurs.ressources.installer());
                    self.ressources = self.adaptateurs.ressources.etat();
                }
            }
            Commande::VerifierRessources => {
                if self.installation.is_none() && self.verification.is_none() {
                    self.verification = Some(self.adaptateurs.ressources.verifier());
                    self.ressources = self.adaptateurs.ressources.etat();
                }
            }
        }
        if let Some(id) = touchee {
            self.persister(id, &mut ev);
        }
        Ok(ev)
    }

    /// Écrit les fichiers de la réunion (fiche, transcriptions) et note ceux qui sont présents.
    fn persister(&mut self, id: IdReunion, ev: &mut Vec<Evenement>) {
        let Some(r) = self.reunion(id) else { return };
        let nom = r.nom_du_dossier.clone();
        let resultat = self.dossier.ecrire_les_fichiers(r);
        let fichiers = self.dossier.roles_presents(&nom);
        if let Ok(r) = self.reunion_mut(id) {
            r.fichiers = fichiers;
        }
        self.noter(&nom, resultat.as_deref().unwrap_or_default());
        if let Err(message) = resultat {
            ev.push(Evenement::EcritureImpossible { reunion: id, message });
        }
    }

    fn reunion_mut(&mut self, id: IdReunion) -> Result<&mut Reunion, Refus> {
        self.reunions.iter_mut().find(|r| r.id == id).ok_or(Refus::ReunionInconnue(id))
    }

    fn demarrer_enregistrement(&mut self) -> Result<(), Refus> {
        if self.enregistrement.is_some() {
            return impossible("un enregistrement est déjà en cours");
        }
        let debut = self.adaptateurs.horloge.maintenant();
        let nom = self.dossier.nom_libre(debut, "Réunion");
        std::fs::create_dir_all(self.dossier.dossier(&nom)).map_err(|e| Refus::Impossible(format!("dossier de la réunion : {e}")))?;
        let demande = DemandeDeCapture { fichier: self.dossier.fichier(&nom, Role::Enregistrement), sources: self.reglages.sources };
        self.adaptateurs.capture.demarrer(demande).map_err(Refus::Impossible)?;
        // La fiche dit qu'un enregistrement est en cours : si l'app s'arrête, la réunion sera
        // relue « enregistrement interrompu ».
        let mut r = Reunion::new(0, nom.clone(), debut, 0.0);
        r.statut = Statut::EnregistrementEnCours;
        let _ = self.dossier.ecrire(&r);
        self.enregistrement = Some(Enregistrement { debut, duree: 0.0, niveaux: Niveaux::default(), reperes: vec![], nombre_de_locuteurs: None, nom_du_dossier: nom });
        Ok(())
    }

    fn arreter_enregistrement(&mut self) -> Result<Evenement, Refus> {
        let Some(e) = self.enregistrement.take() else { return impossible("aucun enregistrement en cours") };
        let duree = self.adaptateurs.capture.arreter().map_err(Refus::Impossible)?;
        let id = self.prochain_id;
        self.prochain_id += 1;
        // Au centième de seconde, comme dans la fiche.
        let centieme = |t: f64| (t * 100.0).round() / 100.0;
        let mut r = Reunion::new(id, e.nom_du_dossier, e.debut, centieme(duree));
        r.nombre_de_locuteurs = e.nombre_de_locuteurs;
        r.reperes = e.reperes.iter().enumerate().map(|(i, t)| Repere { instant: centieme(*t), libelle: format!("Repère {}", i + 1) }).collect();
        self.reunions.insert(0, r);
        if self.reglages.traiter_a_l_arret {
            self.mettre_en_file(id, Etape::Transcription);
        }
        Ok(Evenement::ReunionEnregistree(id))
    }

    fn mettre_en_file(&mut self, id: IdReunion, etape: Etape) {
        if let Ok(r) = self.reunion_mut(id) {
            r.statut = Statut::EnAttente(etape);
            self.file.push_back(id);
        }
    }

    /// Arrête le traitement de la réunion s'il tourne, et la retire de la file.
    fn sortir_de_la_file(&mut self, id: IdReunion) {
        if let Some(mut t) = self.traitement.take_if(|t| t.reunion == id) {
            t.travail.arreter();
        }
        self.file.retain(|x| *x != id);
    }

    // --- Le temps ------------------------------------------------------------------------

    /// Fait passer `ecoule` secondes : la capture, l'installation des ressources et la file de
    /// traitement avancent. Une seule réunion est traitée à la fois ; pendant un enregistrement,
    /// aucune étape ne démarre (celle qui tourne va jusqu'au bout).
    pub fn avancer(&mut self, ecoule: f64) -> Vec<Evenement> {
        let mut ev = Vec::new();
        if let Some(e) = &mut self.enregistrement {
            let etat = self.adaptateurs.capture.sonder(ecoule);
            e.duree = etat.duree;
            e.niveaux = etat.niveaux;
        }
        if let Some(t) = &mut self.installation {
            match t.sonder(ecoule) {
                Avancement::EnCours(_) => {}
                Avancement::Fini(()) => {
                    self.installation = None;
                    ev.push(Evenement::RessourcesInstallees);
                }
                Avancement::Echec(m) => {
                    self.installation = None;
                    ev.push(Evenement::InstallationEnEchec(m));
                }
            }
        }
        if let Some(t) = &mut self.verification
            && !matches!(t.sonder(ecoule), Avancement::EnCours(_))
        {
            self.verification = None;
        }
        self.ressources = self.adaptateurs.ressources.etat();
        if self.traitement.is_none()
            && self.enregistrement.is_none()
            && let Some(id) = self.file.pop_front()
            && let Some(Statut::EnAttente(etape)) = self.reunion(id).map(|r| r.statut.clone())
        {
            self.demarrer_etape(id, etape, &mut ev);
        }
        self.avancer_traitement(ecoule, &mut ev);
        ev
    }

    fn demarrer_etape(&mut self, id: IdReunion, etape: Etape, ev: &mut Vec<Evenement>) {
        match self.preparer(id, etape) {
            Ok(travail) => {
                if let Ok(r) = self.reunion_mut(id) {
                    r.statut = Statut::EnCours { etape, progres: 0.0 };
                }
                self.traitement = Some(Traitement { reunion: id, travail });
                self.persister(id, ev);
            }
            Err(message) => self.echec(id, etape, message, ev),
        }
    }

    fn preparer(&mut self, id: IdReunion, etape: Etape) -> Result<Travail, String> {
        let r = self.reunion(id).ok_or("réunion inconnue")?.clone();
        let audio = self.dossier.fichier(&r.nom_du_dossier, Role::Enregistrement);
        let a = &mut self.adaptateurs;
        self.ressources = a.ressources.etat();
        let manquantes: Vec<&str> = self.ressources.iter().filter(|x| x.etapes.contains(&etape) && x.etat != EtatDeRessource::Prete).map(|x| x.nom.as_str()).collect();
        if !manquantes.is_empty() {
            return Err(format!("Ressources manquantes : {}.", manquantes.join(", ")));
        }
        Ok(match etape {
            Etape::Transcription => {
                // Le vocabulaire du lexique arrive avec la relecture (#9).
                let mots = a.transcripteur.transcrire(DemandeDeTranscription { audio: audio.clone(), duree: r.duree, vocabulaire: String::new() });
                let tours = a.separateur.separer(DemandeDeSeparation { audio, duree: r.duree, nombre_de_locuteurs: r.nombre_de_locuteurs });
                Travail::Transcription { mots: Partie::EnCours(mots), tours: Partie::EnCours(tours) }
            }
            Etape::Relecture => Travail::Relecture(Partie::EnCours(a.agent.relire(DemandeDeRelecture { enregistrement: audio, repliques: pour_l_agent(&r), lexique: String::new() }))),
            Etape::Bilan => {
                let dossier = self.dossier.dossier(&r.nom_du_dossier);
                std::fs::create_dir_all(&dossier).map_err(|e| format!("{} : {e}", dossier.display()))?;
                let fichier = self.dossier.fichier(&r.nom_du_dossier, Role::Bilan);
                let demande = DemandeDeBilan {
                    dossier_de_reunions: self.dossier.racine().to_path_buf(),
                    fichier_du_bilan: fichier.clone(),
                    titre: r.titre.clone(),
                    debut: r.debut,
                    duree: r.duree,
                    repliques: pour_l_agent(&r),
                    locuteurs: r
                        .locuteurs
                        .iter()
                        .map(|l| LocuteurPourLAgent {
                            etiquette: l.etiquette.clone(),
                            nom: l.nom.clone(),
                            proposition: l.proposition.clone(),
                            justification: l.justification.clone(),
                            meme_voix_proposee: l.fusion_proposee.map(|j| r.locuteurs[j].etiquette.clone()),
                        })
                        .collect(),
                    reperes: r.reperes.clone(),
                    bilan_actuel: (!r.bilan.is_empty()).then(|| r.bilan.clone()),
                };
                Travail::Bilan { resultat: Partie::EnCours(a.agent.faire_le_bilan(demande)), fichier }
            }
        })
    }

    fn avancer_traitement(&mut self, ecoule: f64, ev: &mut Vec<Evenement>) {
        let Some(t) = &mut self.traitement else { return };
        let (id, etape) = (t.reunion, t.travail.etape());
        let issue = |p: Result<f64, String>, fini: bool| match p {
            Err(e) => Avancement::Echec(e),
            Ok(_) if fini => Avancement::Fini(()),
            Ok(x) => Avancement::EnCours(x),
        };
        let issue = match &mut t.travail {
            // Les deux moitiés comptent autant.
            Travail::Transcription { mots, tours } => match (mots.sonder(ecoule), tours.sonder(ecoule)) {
                (Ok(a), Ok(b)) => issue(Ok((a + b) / 2.0), mots.fini() && tours.fini()),
                (Err(e), _) | (_, Err(e)) => Avancement::Echec(e),
            },
            Travail::Relecture(p) => {
                let a = p.sonder(ecoule);
                issue(a, p.fini())
            }
            Travail::Bilan { resultat, .. } => {
                let a = resultat.sonder(ecoule);
                issue(a, resultat.fini())
            }
        };
        match issue {
            Avancement::EnCours(p) => {
                if let Ok(r) = self.reunion_mut(id) {
                    r.statut = Statut::EnCours { etape, progres: p };
                }
            }
            Avancement::Echec(message) => {
                if let Some(mut t) = self.traitement.take() {
                    t.travail.arreter();
                }
                self.echec(id, etape, message, ev);
            }
            Avancement::Fini(()) => {
                let Some(t) = self.traitement.take() else { return };
                let produit = match t.travail {
                    Travail::Transcription { mots, tours } => Produit::Transcription(mots.prendre().unwrap_or_default(), tours.prendre().unwrap_or_default()),
                    Travail::Relecture(p) => Produit::Relecture(p.prendre().unwrap_or_default()),
                    Travail::Bilan { resultat, fichier } => match resultat.prendre() {
                        Some(r) => Produit::Bilan(r, fichier),
                        None => return,
                    },
                };
                self.appliquer(id, etape, produit, ev);
            }
        }
    }

    /// Applique le produit d'une étape finie, puis lance la suivante (ou la met en attente
    /// pendant un enregistrement).
    fn appliquer(&mut self, id: IdReunion, etape: Etape, produit: Produit, ev: &mut Vec<Evenement>) {
        let lu = match &produit {
            Produit::Bilan(_, fichier) => match std::fs::read_to_string(fichier) {
                Ok(md) => Some(md),
                Err(e) => return self.echec(id, etape, format!("L'agent n'a pas écrit le bilan ({e})."), ev),
            },
            _ => None,
        };
        if let Some(manque) = lu.as_deref().and_then(bilan::manques) {
            return self.echec(id, etape, manque, ev);
        }
        // Le bilan vient d'être écrit par l'agent pour Greffier : ce n'est pas une modification
        // faite hors de l'app.
        if lu.is_some()
            && let Some(nom) = self.reunion(id).map(|r| r.nom_du_dossier.clone())
        {
            self.noter(&nom, &[Role::Bilan]);
        }
        let en_enregistrement = self.enregistrement.is_some();
        let Ok(r) = self.reunion_mut(id) else { return };
        match produit {
            Produit::Transcription(mots, tours) => {
                let (locuteurs, repliques) = attribution::attribuer(&mots, &tours);
                r.mots = mots;
                r.tours = tours;
                r.locuteurs = locuteurs;
                r.repliques = repliques;
                r.passages.clear();
                r.corrections = 0;
                r.brute.clear();
                r.relecture = None;
                r.corrections_a_la_main.clear();
            }
            Produit::Relecture(resultat) => appliquer_la_relecture(r, resultat),
            Produit::Bilan(resultat, _) => {
                r.poser_bilan(lu.unwrap_or_default());
                r.titre = resultat.titre;
                r.resume = resultat.resume;
                r.actions_principales = resultat.actions.into_iter().take(3).collect();
                r.sessions.bilan = resultat.session;
                for p in resultat.participants {
                    let index = |etiquette: &str| r.locuteurs.iter().position(|l| l.etiquette == etiquette);
                    let Some(i) = index(&p.locuteur) else { continue };
                    let meme = p.meme_voix_que.as_deref().and_then(index).filter(|j| *j != i);
                    let l = &mut r.locuteurs[i];
                    l.proposition = Some(p.nom_propose);
                    l.justification = p.justification;
                    l.fusion_proposee = meme;
                }
            }
        }
        r.etape_faite = Some(etape);
        ev.push(Evenement::EtapeTerminee { reunion: id, etape });
        match etape.suivante() {
            None => {
                r.statut = Statut::Terminee;
                r.nouvelle = true;
                ev.push(Evenement::BilanPret { reunion: id, titre: r.titre.clone() });
                self.persister(id, ev);
            }
            Some(suivante) if en_enregistrement => {
                r.statut = Statut::EnAttente(suivante);
                self.file.push_front(id);
                self.persister(id, ev);
            }
            Some(suivante) => self.demarrer_etape(id, suivante, ev),
        }
    }

    fn echec(&mut self, id: IdReunion, etape: Etape, message: String, ev: &mut Vec<Evenement>) {
        if let Ok(r) = self.reunion_mut(id) {
            r.statut = Statut::Echec { etape, message: message.clone() };
        }
        ev.push(Evenement::EtapeEnEchec { reunion: id, etape, message });
        self.persister(id, ev);
    }
}

fn locuteur_mut(r: &mut Reunion, i: usize) -> Result<&mut Locuteur, Refus> {
    r.locuteurs.get_mut(i).ok_or(Refus::Impossible("locuteur inconnu".into()))
}

fn appliquer_la_relecture(r: &mut Reunion, resultat: ResultatDeRelecture) {
    r.brute = r.repliques.iter().map(|x| Replique { passage: None, ..x.clone() }).collect();
    let mut journal = JournalDeRelecture { ajouts_au_lexique: resultat.ajouts_au_lexique, ..Default::default() };
    for c in resultat.corrections {
        let Some(x) = r.repliques.get_mut(c.replique) else { continue };
        if let Some(i) = x.texte.find(&c.avant) {
            x.texte.replace_range(i..i + c.avant.len(), &c.apres);
            journal.corrections.push(c);
        }
    }
    r.corrections = journal.corrections.len() as u32;
    for c in resultat.changements_de_locuteur {
        if c.replique >= r.repliques.len() {
            continue;
        }
        journal.changements_de_locuteur.push(c.clone());
        let i = match r.locuteurs.iter().position(|l| l.etiquette == c.locuteur) {
            Some(i) => i,
            None => {
                r.locuteurs.push(Locuteur::new(c.locuteur, TEINTES[r.locuteurs.len() % TEINTES.len()]));
                r.locuteurs.len() - 1
            }
        };
        r.repliques[c.replique].locuteur = i;
    }
    r.passages.clear();
    for x in &mut r.repliques {
        x.passage = None;
    }
    for p in resultat.passages_a_verifier {
        let Some(x) = r.repliques.get_mut(p.replique) else { continue };
        if x.passage.is_some() {
            continue;
        }
        x.passage = Some(r.passages.len());
        r.passages.push(PassageAVerifier { replique: p.replique, extrait: p.extrait, hypothese: p.hypothese, reecoute: p.reecoute, statut: StatutDuPassage::EnAttente });
    }
    r.sessions.relecture = resultat.session;
    r.relecture = Some(journal);
}

fn pour_l_agent(r: &Reunion) -> Vec<RepliquePourLAgent> {
    r.repliques
        .iter()
        .enumerate()
        .map(|(numero, x)| {
            let l = &r.locuteurs[r.locuteur_affiche(x.locuteur)];
            RepliquePourLAgent { numero, locuteur: l.etiquette.clone(), nom: l.nom.clone(), debut: x.debut, fin: x.fin, texte: x.texte.clone() }
        })
        .collect()
}
