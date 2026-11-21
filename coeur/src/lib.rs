//! Le cœur de Greffier, sans interface.
//!
//! [`Greffier`] possède l'état des réunions, de l'enregistrement et de la file de traitement. On le
//! pilote par [`Commande`]s ([`Greffier::executer`]) et on fait passer le temps avec
//! [`Greffier::avancer`] ; les deux rendent des [`Evenement`]s. L'interface ne fait que lire l'état
//! (`reunions`, `enregistrement`…) et appeler ces deux fonctions.
//!
//! Tout ce qui sort du processus passe par les [`adaptateurs`] : capture, transcripteur,
//! séparateur de locuteurs, agent, ressources, corbeille, mise en MP4 (et l'horloge). Le cœur ne
//! connaît que ces interfaces ; les vrais adaptateurs sont dans d'autres crates
//! (`greffier-outils` : la CLI `claude`, curl, uv ; `greffier-moteurs` : whisper.cpp, pyannote ;
//! `greffier-capture` : ScreenCaptureKit, corbeille, mise en MP4). Les ressources de l'app
//! (modèles, Python privé, vérification de `claude`) sont décrites dans [`ressources`].

#![forbid(unsafe_code)]

pub mod adaptateurs;
pub mod attribution;
pub mod bilan;
pub mod dossier;
pub mod faux;
pub mod fictif;
pub mod lexique;
pub mod recherche;
pub mod reglages;
pub mod ressources;
mod reunion;
mod suppression;
mod surveillance;
mod temps;
mod titre;

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
    /// Le lexique (`lexique.md` du dossier de l'app). Sa section Vocabulaire est donnée au
    /// transcripteur ; il est relu au début de chaque transcription. Absent ou illisible : pas de
    /// vocabulaire.
    pub lexique: Option<PathBuf>,
    /// Les dossiers ouverts à l'agent du bilan en plus du dossier de réunions (notes, projets…).
    pub dossiers_de_contexte: Vec<PathBuf>,
    /// Le dossier du cache de recherche (`recherche/` du dossier de l'app) : l'index y est
    /// gardé d'un lancement à l'autre, et refait s'il manque. Sans lui, l'index reste en mémoire.
    pub cache_de_recherche: Option<PathBuf>,
}

impl Default for Reglages {
    fn default() -> Reglages {
        Reglages { traiter_a_l_arret: true, sources: Sources::default(), lexique: None, dossiers_de_contexte: Vec::new(), cache_de_recherche: None }
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
    /// Met le dossier de la réunion à la corbeille du Mac et l'oublie (refusé si la corbeille
    /// refuse).
    Supprimer(IdReunion),
    /// Confirme ou corrige le nom d'un locuteur (celui qui le remplace s'il est fusionné).
    NommerLocuteur { reunion: IdReunion, locuteur: usize, nom: String },
    /// `locuteur` est la même voix que `dans`, qui le remplace partout (fusions suivies des deux
    /// côtés) et prend son nom s'il n'en a pas.
    FusionnerLocuteurs { reunion: IdReunion, locuteur: usize, dans: usize },
    /// Défait la fusion de `locuteur` : il retrouve ses répliques, sans nom.
    SeparerLocuteur { reunion: IdReunion, locuteur: usize },
    /// Tranche un passage à vérifier : le texte de sa réplique prend l'hypothèse ou la correction
    /// (rouvrir remet l'extrait), et les deux transcriptions sont réécrites, sans le marquage.
    Trancher { reunion: IdReunion, passage: usize, tranche: Tranche },
    /// Corrige à la main un ou plusieurs mots d'une réplique, dont `texte` est le nouveau texte. Le
    /// changement, élargi aux mots entiers, est gardé comme correction à la main (et repris si la
    /// relecture est refaite). Un passage à vérifier se tranche par [`Commande::Trancher`].
    CorrigerReplique { reunion: IdReunion, replique: usize, texte: String },
    /// Coche ou décoche la `action`-ième case du bilan.
    CocherAction { reunion: IdReunion, action: usize, faite: bool },
    /// La réunion a été ouverte : elle n'est plus nouvelle.
    MarquerLue(IdReunion),
    /// Change le titre : le dossier et ses fichiers sont renommés, liens compris. Refusé pendant
    /// une étape (l'adaptateur écrit dans le dossier).
    ChangerTitre { reunion: IdReunion, titre: String },
    ChangerReglages(Reglages),
    /// Télécharge, installe et vérifie les ressources qui manquent.
    InstallerRessources,
    /// Affiche la demande de macOS pour cette autorisation.
    DemanderAutorisation(Autorisation),
    /// Refait les vérifications des ressources (la CLI `claude`) sans rien télécharger.
    VerifierRessources,
    /// Ajoute une réunion à partir de fichiers existants, copiés dans un nouveau dossier de
    /// réunion (voir [`Greffier::proposer_import`]).
    Importer(dossier::Import),
}

/// Ce qui s'est passé dans le cœur et que l'interface peut vouloir annoncer.
#[derive(Clone, Debug, PartialEq)]
pub enum Evenement {
    ReunionEnregistree(IdReunion),
    /// La capture s'est arrêtée d'elle-même, ou son arrêt a échoué : la réunion est gardée
    /// « enregistrement interrompu » avec ce qui a été capturé, et peut être traitée.
    EnregistrementInterrompu { reunion: IdReunion, message: String },
    /// L'enregistrement a duré moins de [`DUREE_MINIMALE`] : sans doute un faux départ, que
    /// l'interface propose de supprimer. Le traitement suit son cours tant qu'on la garde.
    SuppressionProposee(IdReunion),
    ReunionImportee(IdReunion),
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

/// En dessous de cette durée (en secondes), un enregistrement est proposé à la suppression.
pub const DUREE_MINIMALE: f64 = 60.0;

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

/// Ce qui doit rester éveillé, et pourquoi (voir [`Greffier::garder_le_mac_eveille`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Eveil {
    /// Le nom de l'assertion de non-veille (« Greffier enregistre une réunion »).
    pub raison: &'static str,
    /// L'écran aussi : une fois l'écran éteint, ScreenCaptureKit ne capture plus rien. Sinon,
    /// seul le Mac reste éveillé et l'écran peut s'éteindre.
    pub ecran: bool,
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
    /// Les sources enregistrées (celles des réglages au démarrage).
    pub sources: Sources,
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
    Bilan { resultat: Partie<ResultatDuBilan> },
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
    Bilan(ResultatDuBilan),
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
    /// L'index de la recherche ⌘K (voir [`Greffier::rechercher`]).
    index: recherche::Index,
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
        let mut greffier = Greffier {
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
            index: recherche::Index::default(),
        };
        greffier.indexer();
        greffier
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

    /// Le lexique des réglages, lu maintenant ; sans lexique (ou illisible), le lexique livré.
    fn texte_du_lexique(&self) -> String {
        self.reglages.lexique.as_ref().and_then(|c| std::fs::read_to_string(c).ok()).unwrap_or_else(|| lexique::LEXIQUE_INITIAL.to_string())
    }

    /// Ce que compte le lexique (personnes, mots justes, erreurs connues), pour les réglages.
    pub fn lexique(&self) -> lexique::Apercu {
        lexique::apercu(&self.texte_du_lexique())
    }

    /// Les personnes du lexique, proposées pour nommer les locuteurs.
    pub fn personnes(&self) -> Vec<String> {
        lexique::personnes(&self.texte_du_lexique())
    }

    pub fn dossier(&self) -> &DossierDeReunions {
        &self.dossier
    }

    /// Le rang d'une réunion dans la file : 0 pour celle en cours de traitement.
    pub fn rang(&self, id: IdReunion) -> Option<usize> {
        let en_cours = self.traitement.as_ref().map(|t| t.reunion);
        en_cours.into_iter().chain(self.file.iter().copied()).position(|x| x == id)
    }

    /// Le temps estimé avant la fin du traitement d'une réunion de la file, en secondes : le reste
    /// de la sienne et celui des réunions qui passent avant elle.
    pub fn temps_restant(&self, id: IdReunion) -> Option<f64> {
        let rang = self.rang(id)?;
        let en_cours = self.traitement.as_ref().map(|t| t.reunion);
        en_cours.into_iter().chain(self.file.iter().copied()).take(rang + 1).map(|x| self.reunion(x).and_then(Reunion::temps_restant_estime)).sum()
    }

    pub fn aujourd_hui(&self) -> DateHeure {
        self.adaptateurs.horloge.maintenant()
    }

    /// Les autorisations de macOS dont les sources ont besoin.
    pub fn autorisations(&self) -> Autorisations {
        self.adaptateurs.capture.autorisations()
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

    /// Ce que la fenêtre d'import propose pour des fichiers déposés ou choisis : le média, la
    /// transcription et le bilan, la date et le titre, à modifier avant d'envoyer
    /// [`Commande::Importer`]. N'écrit rien.
    pub fn proposer_import(&self, fichiers: &[PathBuf]) -> Result<dossier::Import, Refus> {
        dossier::import::proposer(fichiers, |t| self.adaptateurs.horloge.en_heure_locale(t)).map_err(Refus::Impossible)
    }

    /// Vrai tant que le temps compte : enregistrement, traitement, file, installation ou
    /// vérification des ressources.
    pub fn occupe(&self) -> bool {
        self.enregistrement.is_some() || self.traitement.is_some() || !self.file.is_empty() || self.installation.is_some() || self.verification.is_some()
    }

    /// Pourquoi le Mac ne doit pas se mettre en veille automatiquement, tant qu'il ne le doit
    /// pas : pendant un enregistrement (l'écran aussi), et pendant un traitement (une réunion en
    /// cours ou en attente dans la file). L'interface en fait une assertion de non-veille de macOS.
    pub fn garder_le_mac_eveille(&self) -> Option<Eveil> {
        if self.enregistrement.is_some() {
            Some(Eveil { raison: "Greffier enregistre une réunion", ecran: true })
        } else if self.traitement.is_some() || !self.file.is_empty() {
            Some(Eveil { raison: "Greffier traite une réunion", ecran: false })
        } else {
            None
        }
    }

    // --- Commandes -----------------------------------------------------------------------

    pub fn executer(&mut self, commande: Commande) -> Result<Vec<Evenement>, Refus> {
        let mut ev = Vec::new();
        let touchee = match &commande {
            Commande::NombreDeLocuteurs { reunion, .. }
            | Commande::Refaire { reunion, .. }
            | Commande::NommerLocuteur { reunion, .. }
            | Commande::FusionnerLocuteurs { reunion, .. }
            | Commande::SeparerLocuteur { reunion, .. }
            | Commande::Trancher { reunion, .. }
            | Commande::CorrigerReplique { reunion, .. }
            | Commande::CocherAction { reunion, .. } => Some(*reunion),
            Commande::Traiter(id) | Commande::ArreterTraitement(id) | Commande::MarquerLue(id) => Some(*id),
            _ => None,
        };
        match commande {
            Commande::DemarrerEnregistrement => self.demarrer_enregistrement()?,
            Commande::ArreterEnregistrement => self.arreter_enregistrement(&mut ev)?,
            Commande::PoserRepere => {
                let Some(e) = &mut self.enregistrement else { return impossible("aucun enregistrement en cours") };
                e.reperes.push(e.duree);
                self.ecrire_la_fiche_d_enregistrement();
            }
            Commande::NombreDeLocuteursEnregistres(n) => {
                let Some(e) = &mut self.enregistrement else { return impossible("aucun enregistrement en cours") };
                e.nombre_de_locuteurs = n;
                self.ecrire_la_fiche_d_enregistrement();
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
                if !r.peut_refaire(etape) {
                    return impossible(if r.statut.dans_la_file() { "la réunion est déjà dans la file" } else { "les étapes précédentes ne sont pas faites" });
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
            Commande::Supprimer(id) => self.supprimer(id)?,
            Commande::NommerLocuteur { reunion, locuteur, nom } => {
                let nom = nom.trim();
                if nom.is_empty() {
                    return impossible("le nom est vide");
                }
                let r = self.reunion_mut(reunion)?;
                locuteur_mut(r, locuteur)?;
                // Un locuteur fusionné porte le nom de celui qui le remplace.
                let i = r.locuteur_affiche(locuteur);
                let l = &mut r.locuteurs[i];
                l.nom = Some(nom.to_string());
                l.fusion_proposee = None;
            }
            Commande::FusionnerLocuteurs { reunion, locuteur, dans } => {
                let r = self.reunion_mut(reunion)?;
                locuteur_mut(r, locuteur)?;
                locuteur_mut(r, dans)?;
                let (i, j) = (r.locuteur_affiche(locuteur), r.locuteur_affiche(dans));
                if !r.fusionner(i, j) {
                    return impossible("ces deux locuteurs sont déjà la même voix");
                }
                // Le nom suit la voix : celui qui reste prend le nom de l'autre s'il n'en a pas.
                // Un locuteur fusionné n'a pas de nom à lui.
                let nom = r.locuteurs[i].nom.take();
                if r.locuteurs[j].nom.is_none() {
                    r.locuteurs[j].nom = nom;
                }
                r.locuteurs[locuteur].fusion_proposee = None;
                r.locuteurs[i].fusion_proposee = None;
            }
            Commande::SeparerLocuteur { reunion, locuteur } => {
                let l = locuteur_mut(self.reunion_mut(reunion)?, locuteur)?;
                if l.fusionne_dans.take().is_none() {
                    return impossible("ce locuteur n'est pas fusionné");
                }
            }
            Commande::Trancher { reunion, passage, tranche } => {
                let r = self.reunion_mut(reunion)?;
                transcription_modifiable(r)?;
                trancher(r, passage, tranche)?;
            }
            Commande::CorrigerReplique { reunion, replique, texte } => {
                let r = self.reunion_mut(reunion)?;
                transcription_modifiable(r)?;
                corriger_la_replique(r, replique, texte)?;
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
            Commande::ChangerTitre { reunion, titre } => {
                let titre = titre.trim();
                if titre.is_empty() {
                    return impossible("le titre est vide");
                }
                let r = self.reunion_mut(reunion)?;
                if matches!(r.statut, Statut::EnCours { .. }) {
                    return impossible("le titre pourra changer à la fin de l'étape en cours");
                }
                r.titre = titre.to_string();
                self.renommer_d_apres_le_titre(reunion, &mut ev);
            }
            Commande::ChangerReglages(r) => {
                let cache = (r.cache_de_recherche != self.reglages.cache_de_recherche).then(|| r.cache_de_recherche.clone());
                self.reglages = r;
                if let Some(cache) = cache {
                    self.utiliser_le_cache_de_recherche(cache);
                }
            }
            Commande::DemanderAutorisation(a) => self.adaptateurs.capture.demander_autorisation(a),
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
            Commande::Importer(import) => {
                let id = self.prochain_id;
                let r = self.dossier.importer(&import, id, self.adaptateurs.mise_en_mp4.as_ref()).map_err(Refus::Impossible)?;
                // Les fichiers copiés par Greffier ne sont pas des modifications faites hors de l'app.
                self.noter(&r.nom_du_dossier.clone(), &Role::TOUS);
                self.prochain_id += 1;
                // Les plus récentes d'abord.
                let place = self.reunions.iter().position(|x| x.debut <= r.debut).unwrap_or(self.reunions.len());
                self.reunions.insert(place, r);
                ev.push(Evenement::ReunionImportee(id));
                self.persister(id, &mut ev);
            }
        }
        if let Some(id) = touchee {
            self.persister(id, &mut ev);
        }
        self.indexer();
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
        let sources = self.reglages.sources;
        if sources.aucune() {
            return impossible("Aucune source à enregistrer : active l'écran, le son du Mac ou le micro dans les réglages.");
        }
        let autorisations = self.adaptateurs.capture.autorisations();
        for a in sources.autorisations() {
            match autorisations.etat(a) {
                EtatDAutorisation::Accordee => {}
                EtatDAutorisation::ADemander => {
                    self.adaptateurs.capture.demander_autorisation(a);
                    return impossible(format!("Greffier demande l'autorisation pour « {} » : accepte la demande de macOS, puis relance l'enregistrement.", a.nom().to_lowercase()));
                }
                EtatDAutorisation::Refusee => {
                    return impossible(format!("Greffier n'a pas l'autorisation pour « {} » : Réglages Système › Confidentialité et sécurité › {}, active Greffier.", a.nom(), a.rubrique()));
                }
            }
        }
        let debut = self.adaptateurs.horloge.maintenant();
        let nom = self.dossier.nom_libre(debut, "Réunion");
        self.dossier.creer(&nom).map_err(|e| Refus::Impossible(format!("dossier de la réunion : {e}")))?;
        let demande = DemandeDeCapture { fichier: self.dossier.fichier(&nom, Role::Enregistrement), sources };
        self.adaptateurs.capture.demarrer(demande).map_err(Refus::Impossible)?;
        self.enregistrement = Some(Enregistrement { debut, duree: 0.0, niveaux: Niveaux::default(), reperes: vec![], nombre_de_locuteurs: None, nom_du_dossier: nom, sources });
        self.ecrire_la_fiche_d_enregistrement();
        Ok(())
    }

    /// La fiche de l'enregistrement en cours, réécrite à chaque repère : si l'app ou le Mac
    /// s'arrête net, la réunion est relue « enregistrement interrompu » avec ses repères et son
    /// nombre de locuteurs (la durée vient alors des fragments déjà écrits de l'enregistrement).
    fn ecrire_la_fiche_d_enregistrement(&self) {
        let Some(e) = &self.enregistrement else { return };
        let mut r = reunion_enregistree(0, e, 0.0);
        r.statut = Statut::EnregistrementEnCours;
        let _ = self.dossier.ecrire(&r);
    }

    fn arreter_enregistrement(&mut self, ev: &mut Vec<Evenement>) -> Result<(), Refus> {
        let Some(e) = self.enregistrement.take() else { return impossible("aucun enregistrement en cours") };
        let duree = match self.adaptateurs.capture.arreter() {
            Ok(duree) => duree,
            Err(message) => {
                self.garder_l_enregistrement_interrompu(&e, None, message, ev);
                return Ok(());
            }
        };
        let id = self.prochain_id;
        self.prochain_id += 1;
        self.reunions.insert(0, reunion_enregistree(id, &e, duree));
        ev.push(Evenement::ReunionEnregistree(id));
        if duree < DUREE_MINIMALE {
            ev.push(Evenement::SuppressionProposee(id));
        }
        if self.reglages.traiter_a_l_arret {
            self.mettre_en_file(id, Etape::Transcription);
        }
        self.persister(id, ev);
        Ok(())
    }

    /// Garde la réunion d'un enregistrement qui ne s'est pas fini normalement, « enregistrement
    /// interrompu », avec ce qui a été capturé. La durée est celle que la capture a rendue,
    /// sinon celle des fragments déjà écrits, sinon la dernière connue. Elle n'est pas mise en
    /// file : on la traite quand on veut.
    fn garder_l_enregistrement_interrompu(&mut self, e: &Enregistrement, duree: Option<f64>, message: String, ev: &mut Vec<Evenement>) {
        let fichier = self.dossier.fichier(&e.nom_du_dossier, Role::Enregistrement);
        let duree = duree.or_else(|| dossier::duree_du_mp4(&fichier)).unwrap_or(e.duree);
        let id = self.prochain_id;
        self.prochain_id += 1;
        let mut r = reunion_enregistree(id, e, duree);
        r.statut = Statut::EnregistrementInterrompu;
        self.reunions.insert(0, r);
        ev.push(Evenement::EnregistrementInterrompu { reunion: id, message });
        self.persister(id, ev);
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
            if let Some(message) = etat.erreur
                && let Some(e) = self.enregistrement.take()
            {
                // Finir le fichier avec ce qui a été capturé.
                let duree = self.adaptateurs.capture.arreter().ok();
                self.garder_l_enregistrement_interrompu(&e, duree, message, &mut ev);
            }
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
        // Une étape finie, un enregistrement arrêté : des réunions ont changé.
        if !ev.is_empty() {
            self.indexer();
        }
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
        let audio = self.dossier.enregistrement(&r.nom_du_dossier).unwrap_or_else(|| self.dossier.fichier(&r.nom_du_dossier, Role::Enregistrement));
        let a = &mut self.adaptateurs;
        self.ressources = a.ressources.etat();
        let manquantes: Vec<&str> = self.ressources.iter().filter(|x| x.etapes.contains(&etape) && x.etat != EtatDeRessource::Prete).map(|x| x.nom.as_str()).collect();
        if !manquantes.is_empty() {
            return Err(format!("Ressources manquantes : {}.", manquantes.join(", ")));
        }
        Ok(match etape {
            Etape::Transcription => {
                let vocabulaire = self.reglages.lexique.as_ref().and_then(|l| std::fs::read_to_string(l).ok()).map(|md| lexique::vocabulaire(&md)).unwrap_or_default();
                let mots = a.transcripteur.transcrire(DemandeDeTranscription { audio: audio.clone(), duree: r.duree, vocabulaire });
                let tours = a.separateur.separer(DemandeDeSeparation { audio, duree: r.duree, nombre_de_locuteurs: r.nombre_de_locuteurs });
                Travail::Transcription { mots: Partie::EnCours(mots), tours: Partie::EnCours(tours) }
            }
            Etape::Relecture => {
                let lexique = self.reglages.lexique.as_ref().and_then(|l| std::fs::read_to_string(l).ok()).unwrap_or_default();
                let demande = DemandeDeRelecture {
                    enregistrement: audio,
                    dossier_de_reunions: self.dossier.racine().to_path_buf(),
                    nom_du_dossier: r.nom_du_dossier.clone(),
                    titre: r.titre.clone(),
                    debut: r.debut,
                    nombre_de_locuteurs: r.nombre_de_locuteurs,
                    // Une relecture refaite repart de la transcription brute.
                    repliques: pour_l_agent(&r, if r.brute.is_empty() { &r.repliques } else { &r.brute }),
                    vocabulaire: lexique::vocabulaire(&lexique),
                    lexique,
                };
                Travail::Relecture(Partie::EnCours(a.agent.relire(demande)))
            }
            Etape::Bilan => {
                self.dossier.creer(&r.nom_du_dossier)?;
                let demande = DemandeDeBilan {
                    dossier_de_reunions: self.dossier.racine().to_path_buf(),
                    dossiers_de_contexte: self.reglages.dossiers_de_contexte.clone(),
                    fichier_du_bilan: self.dossier.fichier(&r.nom_du_dossier, Role::Bilan),
                    fiche: self.dossier.fichier(&r.nom_du_dossier, Role::Fiche),
                    transcription: self.dossier.fichier(&r.nom_du_dossier, Role::TranscriptionLisible),
                    titre: r.titre.clone(),
                    debut: r.debut,
                    duree: r.duree,
                    repliques: pour_l_agent(&r, &r.repliques),
                    locuteurs: r
                        .locuteurs
                        .iter()
                        .map(|l| LocuteurPourLAgent {
                            etiquette: l.etiquette.clone(),
                            nom: l.nom.clone(),
                            proposition: l.proposition.clone(),
                            justification: l.justification.clone(),
                            meme_voix_proposee: l.fusion_proposee.map(|j| r.locuteurs[j].etiquette.clone()),
                            fusionne_dans: l.fusionne_dans.map(|j| r.locuteurs[r.locuteur_affiche(j)].etiquette.clone()),
                        })
                        .collect(),
                    reperes: r.reperes.clone(),
                    bilan_actuel: (!r.bilan.is_empty()).then(|| r.bilan.clone()),
                };
                Travail::Bilan { resultat: Partie::EnCours(a.agent.faire_le_bilan(demande)) }
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
            // Les deux moitiés tournent en même temps : l'étape avance comme la plus lente.
            Travail::Transcription { mots, tours } => match (mots.sonder(ecoule), tours.sonder(ecoule)) {
                (Ok(a), Ok(b)) => issue(Ok(a.min(b)), mots.fini() && tours.fini()),
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
                    Travail::Bilan { resultat } => match resultat.prendre() {
                        Some(r) => Produit::Bilan(r),
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
        // La session de l'agent est gardée même si son bilan est refusé : on peut la reprendre.
        if let Produit::Bilan(resultat) = &produit
            && let Ok(r) = self.reunion_mut(id)
        {
            r.sessions.bilan = resultat.session.clone();
        }
        let nom = self.reunion(id).map(|r| r.nom_du_dossier.clone()).unwrap_or_default();
        let lu = match &produit {
            Produit::Bilan(..) => match self.dossier.lire_le_bilan(&nom) {
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
        let ajoutes = match &produit {
            Produit::Relecture(resultat) => self.enrichir_le_lexique(id, &resultat.ajouts_au_lexique, ev),
            _ => Vec::new(),
        };
        let en_enregistrement = self.enregistrement.is_some();
        let Ok(r) = self.reunion_mut(id) else { return };
        match produit {
            Produit::Transcription(mots, tours) => {
                let a = attribution::attribuer(attribution::sans_les_bruits(mots), tours);
                r.mots = a.mots;
                r.tours = a.tours;
                r.locuteurs = a.locuteurs;
                r.repliques = a.repliques;
                r.passages.clear();
                r.corrections = 0;
                r.brute.clear();
                r.relecture = None;
                r.corrections_a_la_main.clear();
            }
            Produit::Relecture(resultat) => appliquer_la_relecture(r, resultat, ajoutes),
            Produit::Bilan(resultat) => {
                r.poser_bilan(lu.unwrap_or_default());
                r.titre = resultat.titre;
                r.resume = resultat.resume;
                r.actions_principales = resultat.actions.into_iter().take(3).collect();
                for p in resultat.locuteurs {
                    let index = |etiquette: &str| r.locuteurs.iter().position(|l| l.etiquette == etiquette);
                    let Some(i) = index(&p.locuteur) else { continue };
                    let meme = p.meme_voix_que.as_deref().and_then(index).filter(|j| r.locuteur_affiche(*j) != r.locuteur_affiche(i));
                    let l = &mut r.locuteurs[i];
                    // Un nom vide : l'agent n'a pas su qui c'est, sa justification dit pourquoi.
                    l.proposition = Some(p.nom_propose).filter(|n| !n.trim().is_empty());
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
                self.renommer_d_apres_le_titre(id, ev);
            }
            Some(suivante) if en_enregistrement => {
                r.statut = Statut::EnAttente(suivante);
                self.file.push_front(id);
                self.persister(id, ev);
            }
            Some(suivante) => self.demarrer_etape(id, suivante, ev),
        }
    }

    /// Écrit dans le lexique les erreurs apprises par la relecture qu'il ne connaît pas encore ;
    /// rend celles qui ont été écrites. Sans lexique, rien n'est écrit.
    fn enrichir_le_lexique(&mut self, id: IdReunion, ajouts: &[AjoutAuLexique], ev: &mut Vec<Evenement>) -> Vec<AjoutAuLexique> {
        let Some(chemin) = self.reglages.lexique.clone() else { return Vec::new() };
        if ajouts.is_empty() {
            return Vec::new();
        }
        let md = std::fs::read_to_string(&chemin).unwrap_or_default();
        let (md, ajoutes) = lexique::ajouter_erreurs(&md, ajouts);
        if ajoutes.is_empty() {
            return ajoutes;
        }
        match lexique::ecrire(&chemin, &md) {
            Ok(()) => ajoutes,
            Err(e) => {
                ev.push(Evenement::EcritureImpossible { reunion: id, message: format!("{} : {e}", chemin.display()) });
                Vec::new()
            }
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

/// La réunion d'un enregistrement : son dossier, son début, ses repères et son nombre de
/// locuteurs. Durée et repères au centième de seconde, comme dans la fiche.
fn reunion_enregistree(id: IdReunion, e: &Enregistrement, duree: f64) -> Reunion {
    let centieme = |t: f64| (t * 100.0).round() / 100.0;
    let mut r = Reunion::new(id, e.nom_du_dossier.clone(), e.debut, centieme(duree));
    r.nombre_de_locuteurs = e.nombre_de_locuteurs;
    r.reperes = e.reperes.iter().enumerate().map(|(i, t)| Repere { instant: centieme(*t), libelle: format!("Repère {}", i + 1) }).collect();
    r
}

fn locuteur_mut(r: &mut Reunion, i: usize) -> Result<&mut Locuteur, Refus> {
    r.locuteurs.get_mut(i).ok_or(Refus::Impossible("locuteur inconnu".into()))
}

/// La transcription ne se corrige pas pendant qu'une étape va la refaire : la transcription
/// repart de zéro, la relecture rouvre tous les passages à vérifier.
fn transcription_modifiable(r: &Reunion) -> Result<(), Refus> {
    match r.statut {
        Statut::EnCours { etape, .. } | Statut::EnAttente(etape) if etape <= Etape::Relecture => {
            impossible("la transcription est en cours de traitement : attends la fin de la relecture")
        }
        _ => Ok(()),
    }
}

/// Garde le changement du texte d'une réplique comme correction à la main.
fn corriger_la_replique(r: &mut Reunion, replique: usize, texte: String) -> Result<(), Refus> {
    let texte = texte.trim();
    if texte.is_empty() {
        return impossible("une réplique ne peut pas être vide");
    }
    let Some(x) = r.repliques.get(replique) else { return impossible("réplique inconnue") };
    let Some((debut, fin, apres)) = difference_en_mots(&x.texte, texte) else { return Ok(()) };
    // Les passages à vérifier de la réplique se tranchent dans leur onglet.
    for p in x.passages.iter().filter_map(|&i| r.passages.get(i)) {
        let montre = p.texte(&p.statut);
        if let Some(a) = x.texte.find(montre)
            && debut < a + montre.len()
            && a < fin
        {
            return impossible("ce passage est à vérifier : il se tranche dans l'onglet À vérifier");
        }
    }
    let avant = x.texte[debut..fin].to_string();
    r.repliques[replique].texte = texte.to_string();
    r.corrections_a_la_main.push(CorrectionALaMain { replique, avant, apres });
    Ok(())
}

/// Ce qui change de `avant` à `apres`, élargi aux mots entiers : la plage d'octets de `avant` et
/// le texte qui la remplace. `None` s'ils sont identiques.
fn difference_en_mots(avant: &str, apres: &str) -> Option<(usize, usize, String)> {
    if avant == apres {
        return None;
    }
    // Le début commun, puis la fin commune sans chevaucher le début.
    let mut debut = avant.char_indices().zip(apres.chars()).find(|((_, a), b)| a != b).map_or(avant.len().min(apres.len()), |((i, _), _)| i);
    while !avant.is_char_boundary(debut) || !apres.is_char_boundary(debut) {
        debut -= 1;
    }
    let mut commun = 0;
    for (a, b) in avant[debut..].chars().rev().zip(apres[debut..].chars().rev()) {
        if a != b {
            break;
        }
        commun += a.len_utf8();
    }
    let (mut fin_avant, mut fin_apres) = (avant.len() - commun, apres.len() - commun);
    // Aux mots entiers : le début recule et la fin avance (dans la partie commune) jusqu'à un blanc.
    while let Some(c) = avant[..debut].chars().next_back().filter(|c| !c.is_whitespace()) {
        debut -= c.len_utf8();
    }
    while let Some(c) = avant[fin_avant..].chars().next().filter(|c| !c.is_whitespace()) {
        fin_avant += c.len_utf8();
        fin_apres += c.len_utf8();
    }
    Some((debut, fin_avant, apres[debut..fin_apres].to_string()))
}

/// Tranche un passage à vérifier : le texte de la réplique passe de ce que le passage y montre
/// (l'extrait, l'hypothèse ou la correction) à ce que la tranche y met.
fn trancher(r: &mut Reunion, passage: usize, tranche: Tranche) -> Result<(), Refus> {
    let Some(p) = r.passages.get(passage) else { return impossible("passage inconnu") };
    let statut = match tranche {
        Tranche::AccepterHypothese if p.hypothese.is_none() => return impossible("ce passage n'a pas d'hypothèse"),
        Tranche::AccepterHypothese => StatutDuPassage::HypotheseAcceptee,
        Tranche::GarderOriginal => StatutDuPassage::OriginalGarde,
        Tranche::Corriger(texte) if texte.trim().is_empty() => return impossible("la correction est vide"),
        Tranche::Corriger(texte) => StatutDuPassage::CorrigeALaMain(texte.trim().to_string()),
        Tranche::Rouvrir => StatutDuPassage::EnAttente,
    };
    let (actuel, nouveau) = (p.texte(&p.statut).to_string(), p.texte(&statut).to_string());
    let replique = p.replique;
    let Some(x) = r.repliques.get_mut(replique) else { return impossible("réplique inconnue") };
    if actuel != nouveau && !remplacer(&mut x.texte, &actuel, &nouveau) {
        return impossible(format!("« {actuel} » n'est plus dans la réplique : elle a été corrigée à la main"));
    }
    r.passages[passage].statut = statut;
    Ok(())
}

/// Remplace la première occurrence de `avant` par `apres` ; rend `false` si `avant` est vide ou
/// absent.
fn remplacer(texte: &mut String, avant: &str, apres: &str) -> bool {
    match texte.find(avant).filter(|_| !avant.is_empty()) {
        Some(i) => {
            texte.replace_range(i..i + avant.len(), apres);
            true
        }
        None => false,
    }
}

/// Une hypothèse qui n'en est pas une (vide, « ? », « ? (peut-être…) ») n'est pas gardée.
fn hypothese(h: Option<String>) -> Option<String> {
    h.map(|h| h.trim().to_string()).filter(|h| !h.is_empty() && !h.starts_with('?'))
}

/// Applique le résultat de la relecture, toujours à partir de la transcription brute (une
/// relecture refaite ne s'empile pas sur la précédente). Ce qui ne s'applique pas (réplique ou
/// locuteur inconnus, texte absent) est écarté et n'entre pas au journal. `ajoutes` : les erreurs
/// vraiment écrites dans le lexique.
fn appliquer_la_relecture(r: &mut Reunion, resultat: ResultatDeRelecture, ajoutes: Vec<AjoutAuLexique>) {
    if r.brute.is_empty() {
        r.brute = r.repliques.iter().map(|x| Replique { passages: vec![], ..x.clone() }).collect();
    }
    r.repliques = r.brute.clone();
    let mut journal = JournalDeRelecture { ajouts_au_lexique: ajoutes, ..Default::default() };
    for c in resultat.corrections {
        let Some(x) = r.repliques.get_mut(c.replique) else { continue };
        if c.avant != c.apres && remplacer(&mut x.texte, &c.avant, &c.apres) {
            journal.corrections.push(c);
        }
    }
    for c in resultat.changements_de_locuteur {
        let Some(i) = r.locuteurs.iter().position(|l| l.etiquette == c.locuteur) else { continue };
        let Some(x) = r.repliques.get(c.replique) else { continue };
        if r.locuteur_affiche(x.locuteur) == r.locuteur_affiche(i) {
            continue;
        }
        r.repliques[c.replique].locuteur = i;
        journal.changements_de_locuteur.push(c);
    }
    let mut a_verifier = Vec::new();
    for p in resultat.passages_corriges {
        let Some(x) = r.repliques.get_mut(p.replique) else { continue };
        let apres = p.apres.trim();
        if apres.is_empty() || apres.starts_with('?') || apres == p.extrait.trim() {
            continue;
        }
        if remplacer(&mut x.texte, &p.extrait, apres) {
            journal.passages_corriges.push(PassageCorrige { apres: apres.to_string(), ..p });
        } else {
            // L'extrait n'est plus dans la réplique (une correction sûre l'a touché) : à vérifier.
            a_verifier.push(PassageSignale { replique: p.replique, extrait: p.extrait, hypothese: Some(apres.to_string()), reecoute: p.reecoute });
        }
    }
    r.corrections = (journal.corrections.len() + journal.passages_corriges.len()) as u32;
    r.passages.clear();
    for p in resultat.passages_a_verifier.into_iter().chain(a_verifier) {
        let Some(x) = r.repliques.get_mut(p.replique) else { continue };
        // Chaque passage signalé compte, même à plusieurs dans une réplique ; le même deux fois
        // n'en fait qu'un.
        if p.extrait.trim().is_empty() || x.passages.iter().any(|&i| r.passages[i].extrait == p.extrait) {
            continue;
        }
        x.passages.push(r.passages.len());
        r.passages.push(PassageAVerifier { replique: p.replique, extrait: p.extrait, hypothese: hypothese(p.hypothese), reecoute: p.reecoute, statut: StatutDuPassage::EnAttente });
    }
    // Les corrections faites à la main sur la relecture précédente sont reprises si leur texte est
    // toujours là.
    let a_la_main = std::mem::take(&mut r.corrections_a_la_main);
    for c in a_la_main {
        if let Some(x) = r.repliques.get_mut(c.replique)
            && remplacer(&mut x.texte, &c.avant, &c.apres)
        {
            r.corrections_a_la_main.push(c);
        }
    }
    r.sessions.relecture = resultat.session;
    r.relecture = Some(journal);
}

fn pour_l_agent(r: &Reunion, repliques: &[Replique]) -> Vec<RepliquePourLAgent> {
    repliques
        .iter()
        .enumerate()
        .map(|(numero, x)| {
            let l = &r.locuteurs[r.locuteur_affiche(x.locuteur)];
            RepliquePourLAgent { numero, locuteur: l.etiquette.clone(), nom: l.nom.clone(), debut: x.debut, fin: x.fin, texte: x.texte.clone() }
        })
        .collect()
}
