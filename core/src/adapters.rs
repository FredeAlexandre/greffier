//! Les interfaces de tout ce qui sort du processus. Le cœur ne connaît qu'elles : les vrais
//! moteurs (ScreenCaptureKit, whisper.cpp, pyannote, la CLI `claude`, les téléchargements) et les
//! faux adaptateurs des tests et du prototype les implémentent.
//!
//! Un travail long rend une [`Tache`] que le cœur sonde à chaque [`crate::Greffier::avancer`] :
//! un vrai adaptateur fait tourner son travail sur un fil à lui et rapporte son avancement ; un
//! faux avance avec le temps écoulé qu'on lui passe, ce qui rend les tests déterministes.

use std::path::PathBuf;

pub use crate::lexicon::LexiconAddition;
use crate::meeting::{Stage, Word, Marker, SpeakerTurn};
use crate::time::DateTime;

/// Où en est une tâche.
#[derive(Debug, PartialEq)]
pub enum TaskProgress<T> {
    /// En cours, de 0 à 1.
    Running(f64),
    Done(T),
    Failed(String),
}

/// Un travail qui tourne hors du cœur.
pub trait Task<T> {
    /// Appelée à chaque avancée du cœur ; `ecoule` est le temps passé depuis l'appel précédent,
    /// en secondes. Après `Fini` ou `Echec`, la tâche n'est plus sondée.
    fn poll(&mut self, elapsed: f64) -> TaskProgress<T>;
    /// Abandonne le travail (arrêt du traitement, suppression de la réunion).
    fn stop(&mut self) {}
}

pub type BoxedTask<T> = Box<dyn Task<T>>;

// --- Capture -------------------------------------------------------------------------------

/// Les sources enregistrées.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sources {
    pub screen: bool,
    pub mac_audio: bool,
    pub micro: bool,
}

impl Default for Sources {
    fn default() -> Sources {
        Sources { screen: true, mac_audio: true, micro: true }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CaptureRequest {
    /// Le fichier de l'enregistrement, à écrire au fil de l'eau.
    pub file: PathBuf,
    pub sources: Sources,
}

/// Niveaux en direct, de 0 à 1.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Levels {
    pub micro: f32,
    pub mac_audio: f32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CaptureState {
    /// Durée enregistrée, en secondes.
    pub duration: f64,
    pub levels: Levels,
    /// La capture s'est arrêtée d'elle-même (macOS a coupé le flux, l'écran a disparu…). Le cœur
    /// appelle alors [`Capture::arreter`] pour finir le fichier et garde la réunion
    /// « enregistrement interrompu ».
    pub error: Option<String>,
}

/// Où en est une autorisation de macOS.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PermissionState {
    Granted,
    /// Refusée : macOS ne la redemande plus, il faut passer par les Réglages Système.
    Denied,
    /// Jamais demandée : la demander affiche la question de macOS.
    ToRequest,
}

/// Les autorisations dont chaque source a besoin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Permission {
    Screen,
    Micro,
    MacAudio,
}

impl Permission {
    pub const ALL: [Permission; 3] = [Permission::Screen, Permission::MacAudio, Permission::Micro];

    /// Le nom de la source, comme dans l'interface.
    pub fn name(self) -> &'static str {
        match self {
            Permission::Screen => "Écran",
            Permission::Micro => "Micro",
            Permission::MacAudio => "Son du Mac",
        }
    }

    /// La rubrique de Réglages Système › Confidentialité et sécurité qui la donne.
    pub fn section(self) -> &'static str {
        match self {
            Permission::Screen | Permission::MacAudio => "Enregistrement de l'écran et de l'audio du système",
            Permission::Micro => "Micro",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Permissions {
    pub screen: PermissionState,
    pub micro: PermissionState,
    pub mac_audio: PermissionState,
}

impl Permissions {
    pub const GRANTED: Permissions = Permissions { screen: PermissionState::Granted, micro: PermissionState::Granted, mac_audio: PermissionState::Granted };

    pub fn state(&self, a: Permission) -> PermissionState {
        match a {
            Permission::Screen => self.screen,
            Permission::Micro => self.micro,
            Permission::MacAudio => self.mac_audio,
        }
    }

    pub fn state_mut(&mut self, a: Permission) -> &mut PermissionState {
        match a {
            Permission::Screen => &mut self.screen,
            Permission::Micro => &mut self.micro,
            Permission::MacAudio => &mut self.mac_audio,
        }
    }
}

impl Sources {
    /// Les autorisations nécessaires aux sources activées.
    pub fn permissions(&self) -> impl Iterator<Item = Permission> {
        [(self.screen, Permission::Screen), (self.mac_audio, Permission::MacAudio), (self.micro, Permission::Micro)].into_iter().filter(|(on, _)| *on).map(|(_, a)| a)
    }

    pub fn none_enabled(&self) -> bool {
        !(self.screen || self.mac_audio || self.micro)
    }
}

/// Enregistre l'écran, le son du Mac et le micro dans un fichier.
pub trait Capture {
    /// L'état des autorisations de macOS (lecture rapide : appelée par l'interface).
    fn permissions(&self) -> Permissions {
        Permissions::GRANTED
    }
    /// Affiche la demande de macOS pour cette autorisation, si elle n'a jamais été demandée.
    fn request_permission(&mut self, _permission: Permission) {}
    fn start(&mut self, request: CaptureRequest) -> Result<(), String>;
    fn poll(&mut self, elapsed: f64) -> CaptureState;
    /// Arrête et finit d'écrire le fichier ; rend la durée enregistrée.
    fn stop(&mut self) -> Result<f64, String>;
}

// --- Transcripteur et séparateur -----------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct TranscriptionRequest {
    /// L'enregistrement de la réunion (la conversion de sa piste de mixage est l'affaire de
    /// l'adaptateur).
    pub audio: PathBuf,
    /// Durée de la réunion, pour estimer l'avancement.
    pub duration: f64,
    /// La section Vocabulaire du lexique, donnée en prompt.
    pub vocabulary: String,
}

/// Un audio et un vocabulaire en entrée ; des mots horodatés avec leur confiance en sortie.
pub trait Transcriber {
    fn transcribe(&mut self, request: TranscriptionRequest) -> BoxedTask<Vec<Word>>;
}

#[derive(Clone, Debug, PartialEq)]
pub struct DiarizationRequest {
    pub audio: PathBuf,
    pub duration: f64,
    /// Le nombre de locuteurs indiqué, sinon détecté.
    pub speaker_count: Option<u8>,
}

/// Un audio et un nombre de locuteurs éventuel en entrée ; des tours de parole exclusifs en sortie.
pub trait Diarizer {
    fn diarize(&mut self, request: DiarizationRequest) -> BoxedTask<Vec<SpeakerTurn>>;
}

/// Pas de séparation des locuteurs : aucun tour de parole, tous les mots vont à un seul locuteur
/// (pour les tests).
pub struct SingleSpeaker;

impl Diarizer for SingleSpeaker {
    fn diarize(&mut self, _: DiarizationRequest) -> BoxedTask<Vec<SpeakerTurn>> {
        struct NoTurns;
        impl Task<Vec<SpeakerTurn>> for NoTurns {
            fn poll(&mut self, _: f64) -> TaskProgress<Vec<SpeakerTurn>> {
                TaskProgress::Done(Vec::new())
            }
        }
        Box::new(NoTurns)
    }
}

// --- Agent ---------------------------------------------------------------------------------

/// Une réplique telle qu'on la donne à l'agent.
#[derive(Clone, Debug, PartialEq)]
pub struct AgentUtterance {
    /// Index de la réplique dans la réunion ; les résultats y renvoient.
    pub number: usize,
    /// « SPEAKER_00 »
    pub speaker: String,
    /// Le nom donné au locuteur, s'il y en a un.
    pub name: Option<String>,
    pub start: f64,
    pub end: f64,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ReviewRequest {
    /// Pour la réécoute des passages douteux.
    pub recording: PathBuf,
    /// Le répertoire de travail de l'agent : il peut y relire les réunions précédentes.
    pub meetings_folder: PathBuf,
    /// Le dossier de la réunion dans le dossier de réunions.
    pub folder_name: String,
    pub title: String,
    pub start: DateTime,
    /// Le nombre de locuteurs indiqué, s'il l'a été.
    pub speaker_count: Option<u8>,
    /// Les répliques à relire : la transcription brute, d'avant toute relecture.
    pub utterances: Vec<AgentUtterance>,
    /// Le lexique (Markdown), vide s'il n'y en a pas.
    pub lexicon: String,
    /// La section Vocabulaire du lexique : le prompt de la réécoute, comme pour la transcription.
    pub vocabulary: String,
}

/// Une correction sûre : `avant` devient `apres` dans la réplique `replique`.
#[derive(Clone, Debug, PartialEq)]
pub struct ConfidentCorrection {
    pub utterance: usize,
    pub before: String,
    pub after: String,
}

/// La conversation prouve que la réplique est d'un autre locuteur.
#[derive(Clone, Debug, PartialEq)]
pub struct SpeakerChange {
    pub utterance: usize,
    pub speaker: String,
}

/// Un passage douteux que la réécoute a permis de corriger : `extrait` devient `apres`.
#[derive(Clone, Debug, PartialEq)]
pub struct CorrectedPassage {
    pub utterance: usize,
    pub excerpt: String,
    pub after: String,
    pub relisten: String,
}

/// Un passage que la relecture n'a pas su trancher, même après réécoute.
#[derive(Clone, Debug, PartialEq)]
pub struct FlaggedPassage {
    pub utterance: usize,
    pub excerpt: String,
    pub hypothesis: Option<String>,
    pub relisten: String,
}

/// Le résultat des deux passes de la relecture ; Greffier l'applique lui-même.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReviewResult {
    pub session: Option<String>,
    pub corrections: Vec<ConfidentCorrection>,
    pub speaker_changes: Vec<SpeakerChange>,
    pub corrected_passages: Vec<CorrectedPassage>,
    pub passages_to_check: Vec<FlaggedPassage>,
    /// Les erreurs apprises ; Greffier n'écrit dans le lexique que celles qu'il ne connaît pas.
    pub lexicon_additions: Vec<LexiconAddition>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AgentSpeaker {
    pub label: String,
    pub name: Option<String>,
    pub proposal: Option<String>,
    pub justification: String,
    /// La fusion déjà proposée (« SPEAKER_00 »), s'il y en a une.
    pub proposed_same_voice: Option<String>,
    /// Fusionné par l'utilisateur dans ce locuteur (« SPEAKER_00 ») : ses répliques sont sous
    /// celui-ci dans la transcription lisible.
    pub merged_into: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SummaryRequest {
    /// Le répertoire de travail de l'agent.
    pub meetings_folder: PathBuf,
    /// Les dossiers de contexte des réglages, ouverts à l'agent en plus du dossier de réunions.
    pub context_folders: Vec<PathBuf>,
    /// Le fichier que l'agent doit écrire.
    pub summary_file: PathBuf,
    /// La fiche et la transcription lisible de la réunion, que l'agent lit.
    pub sheet: PathBuf,
    pub transcription: PathBuf,
    pub title: String,
    pub start: DateTime,
    pub duration: f64,
    pub utterances: Vec<AgentUtterance>,
    pub speakers: Vec<AgentSpeaker>,
    pub markers: Vec<Marker>,
    /// Le bilan précédent quand on refait le bilan.
    pub current_summary: Option<String>,
}

/// Ce que l'agent propose pour un locuteur (un élément de `participants` dans sa réponse JSON :
/// la clé du prompt ne change pas).
#[derive(Clone, Debug, PartialEq)]
pub struct SummarySpeaker {
    /// « SPEAKER_00 »
    pub speaker: String,
    /// Vide quand l'agent ne sait pas qui c'est (la justification dit pourquoi).
    pub proposed_name: String,
    pub justification: String,
    /// Sans doute la même voix qu'un autre locuteur (« SPEAKER_00 »).
    pub same_voice_as: Option<String>,
}

/// Ce que l'agent renvoie après avoir écrit le fichier du bilan.
#[derive(Clone, Debug, PartialEq)]
pub struct SummaryResult {
    pub session: Option<String>,
    pub title: String,
    pub brief: String,
    /// Les 3 actions principales, au plus.
    pub actions: Vec<String>,
    pub speakers: Vec<SummarySpeaker>,
}

/// La relecture en deux passes (réécoute comprise) et le bilan, confiés à un agent.
pub trait Agent {
    fn review(&mut self, request: ReviewRequest) -> BoxedTask<ReviewResult>;
    /// Écrit `demande.fichier_du_bilan` et renvoie titre, résumé, actions et locuteurs.
    fn summarize(&mut self, request: SummaryRequest) -> BoxedTask<SummaryResult>;
}

// --- Ressources ----------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub enum ResourceState {
    Missing,
    /// Un téléchargement interrompu (app quittée, connexion coupée) : il reprendra là où il s'est
    /// arrêté. En octets.
    Interrupted { received: u64, total: u64 },
    /// En octets.
    Downloading { received: u64, total: u64 },
    /// Le calcul du sha256, ou la vérification de la CLI `claude`.
    Verifying,
    /// L'installation de l'environnement Python, sans progression mesurable.
    Installing,
    Ready,
    Failed(String),
}

/// Un modèle, l'environnement Python, ou une vérification (la CLI `claude`).
#[derive(Clone, Debug, PartialEq)]
pub struct Resource {
    pub name: String,
    /// D'où elle vient, ou ce que la vérification a trouvé (« huggingface.co », « 2.1.3 · connectée »).
    pub detail: String,
    pub state: ResourceState,
    /// Place prise sur le disque, en octets (téléchargements partiels compris).
    pub size: u64,
    /// Taille du téléchargement, en octets (0 : rien à télécharger, ou taille inconnue).
    pub total: u64,
    /// Ce qu'il faut faire quand elle manque et que l'app ne peut pas l'installer elle-même.
    pub help: Option<String>,
    /// Les étapes du traitement qui ne peuvent pas démarrer sans elle.
    pub stages: Vec<Stage>,
}

/// Télécharge et vérifie les modèles et l'environnement Python ; vérifie la CLI `claude`.
pub trait Resources {
    fn state(&mut self) -> Vec<Resource>;
    /// Télécharge, installe et vérifie ce qui manque, et refait les vérifications.
    fn install(&mut self) -> BoxedTask<()>;
    /// Refait les vérifications (la CLI `claude`) sans rien télécharger.
    fn check(&mut self) -> BoxedTask<()>;
}

// --- Horloge -------------------------------------------------------------------------------

/// L'heure locale : le début des enregistrements et le jour d'aujourd'hui.
pub trait Clock {
    fn now(&self) -> DateTime;
    /// Un instant (date de création d'un média, de modification d'un fichier) en heure locale.
    /// Par défaut en temps universel : la vraie horloge y met le fuseau du Mac.
    fn to_local_time(&self, instant: std::time::SystemTime) -> DateTime {
        let seconds = match instant.duration_since(std::time::UNIX_EPOCH) {
            Ok(d) => d.as_secs() as i64,
            Err(e) => -(e.duration().as_secs() as i64),
        };
        DateTime::from_unix(seconds)
    }
}

// --- Corbeille ------------------------------------------------------------------------------

/// La corbeille du Mac : une réunion supprimée y envoie son dossier entier, pour qu'on puisse
/// s'en rattraper.
pub trait Trash {
    /// Met ce dossier (ou fichier) à la corbeille. L'erreur est un message pour l'utilisateur.
    fn trash(&self, path: &std::path::Path) -> Result<(), String>;
}

// --- Mise en MP4 ----------------------------------------------------------------------------

/// Passer un média importé de la famille MP4 (QuickTime `.mov`, `.m4a`, `.m4v`) dans un fichier
/// `.mp4`, sans le réencoder : l'enregistrement d'une réunion s'appelle `… - enregistrement.mp4`.
pub trait Mp4Muxer {
    /// Écrit `vers` (un `.mp4`) avec les pistes de `de`, sans toucher à `de`. L'erreur est un
    /// message ; le cœur copie alors le média tel quel, avec son extension.
    fn to_mp4(&self, from: &std::path::Path, to: &std::path::Path) -> Result<(), String>;
}

/// Tous les adaptateurs branchés sur le cœur.
pub struct Adapters {
    pub capture: Box<dyn Capture>,
    pub transcriber: Box<dyn Transcriber>,
    pub diarizer: Box<dyn Diarizer>,
    pub agent: Box<dyn Agent>,
    pub resources: Box<dyn Resources>,
    pub clock: Box<dyn Clock>,
    pub trash: Box<dyn Trash>,
    pub mp4_muxing: Box<dyn Mp4Muxer>,
}
