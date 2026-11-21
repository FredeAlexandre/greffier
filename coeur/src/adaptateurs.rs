//! Les interfaces de tout ce qui sort du processus. Le cœur ne connaît qu'elles : les vrais
//! moteurs (ScreenCaptureKit, whisper.cpp, pyannote, la CLI `claude`, les téléchargements) et les
//! faux adaptateurs des tests et du prototype les implémentent.
//!
//! Un travail long rend une [`Tache`] que le cœur sonde à chaque [`crate::Greffier::avancer`] :
//! un vrai adaptateur fait tourner son travail sur un fil à lui et rapporte son avancement ; un
//! faux avance avec le temps écoulé qu'on lui passe, ce qui rend les tests déterministes.

use std::path::PathBuf;

use crate::reunion::{Etape, Mot, Repere, TourDeParole};
use crate::temps::DateHeure;

/// Où en est une tâche.
#[derive(Debug, PartialEq)]
pub enum Avancement<T> {
    /// En cours, de 0 à 1.
    EnCours(f64),
    Fini(T),
    Echec(String),
}

/// Un travail qui tourne hors du cœur.
pub trait Tache<T> {
    /// Appelée à chaque avancée du cœur ; `ecoule` est le temps passé depuis l'appel précédent,
    /// en secondes. Après `Fini` ou `Echec`, la tâche n'est plus sondée.
    fn sonder(&mut self, ecoule: f64) -> Avancement<T>;
    /// Abandonne le travail (arrêt du traitement, suppression de la réunion).
    fn arreter(&mut self) {}
}

pub type TacheDe<T> = Box<dyn Tache<T>>;

// --- Capture -------------------------------------------------------------------------------

/// Les sources enregistrées.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sources {
    pub ecran: bool,
    pub son_du_mac: bool,
    pub micro: bool,
}

impl Default for Sources {
    fn default() -> Sources {
        Sources { ecran: true, son_du_mac: true, micro: true }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DemandeDeCapture {
    /// Le fichier de l'enregistrement, à écrire au fil de l'eau.
    pub fichier: PathBuf,
    pub sources: Sources,
}

/// Niveaux en direct, de 0 à 1.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Niveaux {
    pub micro: f32,
    pub son_du_mac: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EtatDeCapture {
    /// Durée enregistrée, en secondes.
    pub duree: f64,
    pub niveaux: Niveaux,
}

/// Où en est une autorisation de macOS.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EtatDAutorisation {
    Accordee,
    /// Refusée : macOS ne la redemande plus, il faut passer par les Réglages Système.
    Refusee,
    /// Jamais demandée : la demander affiche la question de macOS.
    ADemander,
}

/// Les autorisations dont chaque source a besoin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Autorisation {
    Ecran,
    Micro,
    SonDuMac,
}

impl Autorisation {
    pub const TOUTES: [Autorisation; 3] = [Autorisation::Ecran, Autorisation::SonDuMac, Autorisation::Micro];

    /// Le nom de la source, comme dans l'interface.
    pub fn nom(self) -> &'static str {
        match self {
            Autorisation::Ecran => "Écran",
            Autorisation::Micro => "Micro",
            Autorisation::SonDuMac => "Son du Mac",
        }
    }

    /// La rubrique de Réglages Système › Confidentialité et sécurité qui la donne.
    pub fn rubrique(self) -> &'static str {
        match self {
            Autorisation::Ecran | Autorisation::SonDuMac => "Enregistrement de l'écran et de l'audio du système",
            Autorisation::Micro => "Micro",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Autorisations {
    pub ecran: EtatDAutorisation,
    pub micro: EtatDAutorisation,
    pub son_du_mac: EtatDAutorisation,
}

impl Autorisations {
    pub const ACCORDEES: Autorisations = Autorisations { ecran: EtatDAutorisation::Accordee, micro: EtatDAutorisation::Accordee, son_du_mac: EtatDAutorisation::Accordee };

    pub fn etat(&self, a: Autorisation) -> EtatDAutorisation {
        match a {
            Autorisation::Ecran => self.ecran,
            Autorisation::Micro => self.micro,
            Autorisation::SonDuMac => self.son_du_mac,
        }
    }

    pub fn etat_mut(&mut self, a: Autorisation) -> &mut EtatDAutorisation {
        match a {
            Autorisation::Ecran => &mut self.ecran,
            Autorisation::Micro => &mut self.micro,
            Autorisation::SonDuMac => &mut self.son_du_mac,
        }
    }
}

impl Sources {
    /// Les autorisations nécessaires aux sources activées.
    pub fn autorisations(&self) -> impl Iterator<Item = Autorisation> {
        [(self.ecran, Autorisation::Ecran), (self.son_du_mac, Autorisation::SonDuMac), (self.micro, Autorisation::Micro)].into_iter().filter(|(on, _)| *on).map(|(_, a)| a)
    }

    pub fn aucune(&self) -> bool {
        !(self.ecran || self.son_du_mac || self.micro)
    }
}

/// Enregistre l'écran, le son du Mac et le micro dans un fichier.
pub trait Capture {
    /// L'état des autorisations de macOS (lecture rapide : appelée par l'interface).
    fn autorisations(&self) -> Autorisations {
        Autorisations::ACCORDEES
    }
    /// Affiche la demande de macOS pour cette autorisation, si elle n'a jamais été demandée.
    fn demander_autorisation(&mut self, _autorisation: Autorisation) {}
    fn demarrer(&mut self, demande: DemandeDeCapture) -> Result<(), String>;
    fn sonder(&mut self, ecoule: f64) -> EtatDeCapture;
    /// Arrête et finit d'écrire le fichier ; rend la durée enregistrée.
    fn arreter(&mut self) -> Result<f64, String>;
}

// --- Transcripteur et séparateur -----------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct DemandeDeTranscription {
    /// L'enregistrement de la réunion (la conversion de sa piste de mixage est l'affaire de
    /// l'adaptateur).
    pub audio: PathBuf,
    /// Durée de la réunion, pour estimer l'avancement.
    pub duree: f64,
    /// La section Vocabulaire du lexique, donnée en prompt.
    pub vocabulaire: String,
}

/// Un audio et un vocabulaire en entrée ; des mots horodatés avec leur confiance en sortie.
pub trait Transcripteur {
    fn transcrire(&mut self, demande: DemandeDeTranscription) -> TacheDe<Vec<Mot>>;
}

#[derive(Clone, Debug, PartialEq)]
pub struct DemandeDeSeparation {
    pub audio: PathBuf,
    pub duree: f64,
    /// Le nombre de locuteurs indiqué, sinon détecté.
    pub nombre_de_locuteurs: Option<u8>,
}

/// Un audio et un nombre de locuteurs éventuel en entrée ; des tours de parole exclusifs en sortie.
pub trait Separateur {
    fn separer(&mut self, demande: DemandeDeSeparation) -> TacheDe<Vec<TourDeParole>>;
}

/// Pas de séparation des locuteurs : aucun tour de parole, tous les mots vont à un seul locuteur.
/// Branché tant que le vrai séparateur (pyannote) ne l'est pas.
pub struct UnSeulLocuteur;

impl Separateur for UnSeulLocuteur {
    fn separer(&mut self, _: DemandeDeSeparation) -> TacheDe<Vec<TourDeParole>> {
        struct Aucun;
        impl Tache<Vec<TourDeParole>> for Aucun {
            fn sonder(&mut self, _: f64) -> Avancement<Vec<TourDeParole>> {
                Avancement::Fini(Vec::new())
            }
        }
        Box::new(Aucun)
    }
}

// --- Agent ---------------------------------------------------------------------------------

/// Une réplique telle qu'on la donne à l'agent.
#[derive(Clone, Debug, PartialEq)]
pub struct RepliquePourLAgent {
    /// Index de la réplique dans la réunion ; les résultats y renvoient.
    pub numero: usize,
    /// « SPEAKER_00 »
    pub locuteur: String,
    /// Le nom donné au locuteur, s'il y en a un.
    pub nom: Option<String>,
    pub debut: f64,
    pub fin: f64,
    pub texte: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DemandeDeRelecture {
    /// Pour la réécoute des passages douteux.
    pub enregistrement: PathBuf,
    pub repliques: Vec<RepliquePourLAgent>,
    pub lexique: String,
}

/// Une correction sûre : `avant` devient `apres` dans la réplique `replique`.
#[derive(Clone, Debug, PartialEq)]
pub struct CorrectionSure {
    pub replique: usize,
    pub avant: String,
    pub apres: String,
}

/// La conversation prouve que la réplique est d'un autre locuteur.
#[derive(Clone, Debug, PartialEq)]
pub struct ChangementDeLocuteur {
    pub replique: usize,
    pub locuteur: String,
}

/// Un passage que la relecture n'a pas su trancher, même après réécoute.
#[derive(Clone, Debug, PartialEq)]
pub struct PassageSignale {
    pub replique: usize,
    pub extrait: String,
    pub hypothese: Option<String>,
    pub reecoute: String,
}

/// Le résultat des deux passes de la relecture ; Greffier l'applique lui-même.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ResultatDeRelecture {
    pub session: Option<String>,
    pub corrections: Vec<CorrectionSure>,
    pub changements_de_locuteur: Vec<ChangementDeLocuteur>,
    pub passages_a_verifier: Vec<PassageSignale>,
    pub ajouts_au_lexique: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LocuteurPourLAgent {
    pub etiquette: String,
    pub nom: Option<String>,
    pub proposition: Option<String>,
    pub justification: String,
    /// La fusion déjà proposée (« SPEAKER_00 »), s'il y en a une.
    pub meme_voix_proposee: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DemandeDeBilan {
    /// Le répertoire de travail de l'agent.
    pub dossier_de_reunions: PathBuf,
    /// Le fichier que l'agent doit écrire.
    pub fichier_du_bilan: PathBuf,
    pub titre: String,
    pub debut: DateHeure,
    pub duree: f64,
    pub repliques: Vec<RepliquePourLAgent>,
    pub locuteurs: Vec<LocuteurPourLAgent>,
    pub reperes: Vec<Repere>,
    /// Le bilan précédent quand on refait le bilan.
    pub bilan_actuel: Option<String>,
}

/// Ce que l'agent propose pour un locuteur.
#[derive(Clone, Debug, PartialEq)]
pub struct Participant {
    /// « SPEAKER_00 »
    pub locuteur: String,
    pub nom_propose: String,
    pub justification: String,
    /// Sans doute la même voix qu'un autre locuteur (« SPEAKER_00 »).
    pub meme_voix_que: Option<String>,
}

/// Ce que l'agent renvoie après avoir écrit le fichier du bilan.
#[derive(Clone, Debug, PartialEq)]
pub struct ResultatDuBilan {
    pub session: Option<String>,
    pub titre: String,
    pub resume: String,
    /// Les 3 actions principales, au plus.
    pub actions: Vec<String>,
    pub participants: Vec<Participant>,
}

/// La relecture en deux passes (réécoute comprise) et le bilan, confiés à un agent.
pub trait Agent {
    fn relire(&mut self, demande: DemandeDeRelecture) -> TacheDe<ResultatDeRelecture>;
    /// Écrit `demande.fichier_du_bilan` et renvoie titre, résumé, actions et participants.
    fn faire_le_bilan(&mut self, demande: DemandeDeBilan) -> TacheDe<ResultatDuBilan>;
}

// --- Ressources ----------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub enum EtatDeRessource {
    Absente,
    /// Un téléchargement interrompu (app quittée, connexion coupée) : il reprendra là où il s'est
    /// arrêté. En octets.
    Interrompue { recu: u64, total: u64 },
    /// En octets.
    EnTelechargement { recu: u64, total: u64 },
    /// Le calcul du sha256, ou la vérification de la CLI `claude`.
    EnVerification,
    /// L'installation de l'environnement Python, sans progression mesurable.
    EnInstallation,
    Prete,
    Echec(String),
}

/// Un modèle, l'environnement Python, ou une vérification (la CLI `claude`).
#[derive(Clone, Debug, PartialEq)]
pub struct Ressource {
    pub nom: String,
    /// D'où elle vient, ou ce que la vérification a trouvé (« huggingface.co », « 2.1.3 · connectée »).
    pub detail: String,
    pub etat: EtatDeRessource,
    /// Place prise sur le disque, en octets (téléchargements partiels compris).
    pub taille: u64,
    /// Taille du téléchargement, en octets (0 : rien à télécharger, ou taille inconnue).
    pub total: u64,
    /// Ce qu'il faut faire quand elle manque et que l'app ne peut pas l'installer elle-même.
    pub aide: Option<String>,
    /// Les étapes du traitement qui ne peuvent pas démarrer sans elle.
    pub etapes: Vec<Etape>,
}

/// Télécharge et vérifie les modèles et l'environnement Python ; vérifie la CLI `claude`.
pub trait Ressources {
    fn etat(&mut self) -> Vec<Ressource>;
    /// Télécharge, installe et vérifie ce qui manque, et refait les vérifications.
    fn installer(&mut self) -> TacheDe<()>;
    /// Refait les vérifications (la CLI `claude`) sans rien télécharger.
    fn verifier(&mut self) -> TacheDe<()>;
}

// --- Horloge -------------------------------------------------------------------------------

/// L'heure locale : le début des enregistrements et le jour d'aujourd'hui.
pub trait Horloge {
    fn maintenant(&self) -> DateHeure;
    /// Un instant (date de création d'un média, de modification d'un fichier) en heure locale.
    /// Par défaut en temps universel : la vraie horloge y met le fuseau du Mac.
    fn en_heure_locale(&self, instant: std::time::SystemTime) -> DateHeure {
        let secondes = match instant.duration_since(std::time::UNIX_EPOCH) {
            Ok(d) => d.as_secs() as i64,
            Err(e) => -(e.duration().as_secs() as i64),
        };
        DateHeure::depuis_unix(secondes)
    }
}

/// Tous les adaptateurs branchés sur le cœur.
pub struct Adaptateurs {
    pub capture: Box<dyn Capture>,
    pub transcripteur: Box<dyn Transcripteur>,
    pub separateur: Box<dyn Separateur>,
    pub agent: Box<dyn Agent>,
    pub ressources: Box<dyn Ressources>,
    pub horloge: Box<dyn Horloge>,
}
