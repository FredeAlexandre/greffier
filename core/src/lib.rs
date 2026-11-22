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

pub mod adapters;
pub mod attribution;
pub mod summary;
pub mod folder;
pub mod fake;
pub mod demo;
pub mod lexicon;
pub mod search;
pub mod settings;
pub mod resources;
mod meeting;
mod deletion;
mod watch;
mod time;
mod title;

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::path::PathBuf;

use adapters::*;
pub use folder::{MeetingsFolder, Role};
pub use meeting::*;
pub use time::*;

/// Les réglages qui changent le comportement du cœur.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// Mettre la réunion dans la file de traitement dès l'arrêt de l'enregistrement.
    pub process_on_stop: bool,
    pub sources: Sources,
    /// Le lexique (`lexique.md` du dossier de l'app). Sa section Vocabulaire est donnée au
    /// transcripteur ; il est relu au début de chaque transcription. Absent ou illisible : pas de
    /// vocabulaire.
    pub lexicon: Option<PathBuf>,
    /// Les dossiers ouverts à l'agent du bilan en plus du dossier de réunions (notes, projets…).
    pub context_folders: Vec<PathBuf>,
    /// Le dossier du cache de recherche (`recherche/` du dossier de l'app) : l'index y est
    /// gardé d'un lancement à l'autre, et refait s'il manque. Sans lui, l'index reste en mémoire.
    pub search_cache: Option<PathBuf>,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings { process_on_stop: true, sources: Sources::default(), lexicon: None, context_folders: Vec::new(), search_cache: None }
    }
}

/// Comment l'utilisateur tranche un passage à vérifier.
#[derive(Clone, Debug, PartialEq)]
pub enum Resolution {
    AcceptHypothesis,
    KeepOriginal,
    Correct(String),
    /// Remet le passage en attente.
    Reopen,
}

/// Tout ce qu'on peut demander au cœur.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    StartRecording,
    StopRecording,
    AddMarker,
    /// Le nombre de locuteurs indiqué pendant l'enregistrement (`None` : détecté).
    RecordingSpeakerCount(Option<u8>),
    /// Le nombre de locuteurs indiqué avant le traitement (`None` : détecté).
    SpeakerCount { meeting: MeetingId, number: Option<u8> },
    /// Met la réunion dans la file, à partir de la première étape qui lui manque.
    Process(MeetingId),
    /// Remet la réunion dans la file à partir de cette étape ; les suivantes sont refaites aussi.
    Redo { meeting: MeetingId, stage: Stage },
    /// Arrête le traitement (ou sort la réunion de la file) : elle redevient à traiter.
    StopProcessing(MeetingId),
    /// Met le dossier de la réunion à la corbeille du Mac et l'oublie (refusé si la corbeille
    /// refuse).
    Delete(MeetingId),
    /// Confirme ou corrige le nom d'un locuteur (celui qui le remplace s'il est fusionné).
    NameSpeaker { meeting: MeetingId, speaker: usize, name: String },
    /// `locuteur` est la même voix que `dans`, qui le remplace partout (fusions suivies des deux
    /// côtés) et prend son nom s'il n'en a pas.
    MergeSpeakers { meeting: MeetingId, speaker: usize, into: usize },
    /// Défait la fusion de `locuteur` : il retrouve ses répliques, sans nom.
    SplitSpeaker { meeting: MeetingId, speaker: usize },
    /// Tranche un passage à vérifier : le texte de sa réplique prend l'hypothèse ou la correction
    /// (rouvrir remet l'extrait), et les deux transcriptions sont réécrites, sans le marquage.
    Resolve { meeting: MeetingId, passage: usize, resolution: Resolution },
    /// Corrige à la main un ou plusieurs mots d'une réplique, dont `texte` est le nouveau texte. Le
    /// changement, élargi aux mots entiers, est gardé comme correction à la main (et repris si la
    /// relecture est refaite). Un passage à vérifier se tranche par [`Commande::Trancher`].
    CorrectUtterance { meeting: MeetingId, utterance: usize, text: String },
    /// Coche ou décoche la `action`-ième case du bilan.
    CheckAction { meeting: MeetingId, action: usize, done: bool },
    /// La réunion a été ouverte : elle n'est plus nouvelle.
    MarkRead(MeetingId),
    /// Change le titre : le dossier et ses fichiers sont renommés, liens compris. Refusé pendant
    /// une étape (l'adaptateur écrit dans le dossier).
    ChangeTitle { meeting: MeetingId, title: String },
    ChangeSettings(Settings),
    /// Télécharge, installe et vérifie les ressources qui manquent.
    InstallResources,
    /// Affiche la demande de macOS pour cette autorisation.
    RequestPermission(Permission),
    /// Refait les vérifications des ressources (la CLI `claude`) sans rien télécharger.
    CheckResources,
    /// Ajoute une réunion à partir de fichiers existants, copiés dans un nouveau dossier de
    /// réunion (voir [`Greffier::proposer_import`]).
    Import(folder::Import),
}

/// Ce qui s'est passé dans le cœur et que l'interface peut vouloir annoncer.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    MeetingRecorded(MeetingId),
    /// La capture s'est arrêtée d'elle-même, ou son arrêt a échoué : la réunion est gardée
    /// « enregistrement interrompu » avec ce qui a été capturé, et peut être traitée.
    RecordingInterrupted { meeting: MeetingId, message: String },
    /// L'enregistrement a duré moins de [`DUREE_MINIMALE`] : sans doute un faux départ, que
    /// l'interface propose de supprimer. Le traitement suit son cours tant qu'on la garde.
    DeletionProposed(MeetingId),
    MeetingImported(MeetingId),
    StageDone { meeting: MeetingId, stage: Stage },
    StageFailed { meeting: MeetingId, stage: Stage, message: String },
    SummaryReady { meeting: MeetingId, title: String },
    ResourcesInstalled,
    InstallationFailed(String),
    /// Un fichier de la réunion n'a pas pu être écrit dans le dossier de réunions.
    WriteFailed { meeting: MeetingId, message: String },
    /// Un fichier de la réunion a changé hors de l'app : elle a été relue.
    MeetingReviewed(MeetingId),
    /// Un dossier de réunion est apparu hors de l'app.
    MeetingAppeared(MeetingId),
    /// Le dossier de la réunion a disparu hors de l'app : elle n'est plus dans la liste.
    MeetingDisappeared(MeetingId),
}

/// En dessous de cette durée (en secondes), un enregistrement est proposé à la suppression.
pub const MIN_DURATION: f64 = 60.0;

/// Pourquoi une commande n'a pas été exécutée. L'état n'a pas changé.
#[derive(Clone, Debug, PartialEq)]
pub enum Rejection {
    UnknownMeeting(MeetingId),
    Impossible(String),
}

impl fmt::Display for Rejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Rejection::UnknownMeeting(id) => write!(f, "réunion inconnue ({id})"),
            Rejection::Impossible(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for Rejection {}

fn impossible<T>(message: impl Into<String>) -> Result<T, Rejection> {
    Err(Rejection::Impossible(message.into()))
}

/// Ce qui doit rester éveillé, et pourquoi (voir [`Greffier::garder_le_mac_eveille`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Wake {
    /// Le nom de l'assertion de non-veille (« Greffier enregistre une réunion »).
    pub reason: &'static str,
    /// L'écran aussi : une fois l'écran éteint, ScreenCaptureKit ne capture plus rien. Sinon,
    /// seul le Mac reste éveillé et l'écran peut s'éteindre.
    pub screen: bool,
}

/// L'enregistrement en cours.
#[derive(Clone, Debug, PartialEq)]
pub struct Recording {
    pub start: DateTime,
    /// Durée enregistrée, en secondes.
    pub duration: f64,
    pub levels: Levels,
    /// Instants des repères posés.
    pub markers: Vec<f64>,
    pub speaker_count: Option<u8>,
    pub folder_name: String,
    /// Les sources enregistrées (celles des réglages au démarrage).
    pub sources: Sources,
}

enum Part<T> {
    Running(BoxedTask<T>),
    Done(T),
}

impl<T> Part<T> {
    fn poll(&mut self, elapsed: f64) -> Result<f64, String> {
        let a = match self {
            Part::Running(t) => t.poll(elapsed),
            Part::Done(_) => return Ok(1.0),
        };
        match a {
            TaskProgress::Running(p) => Ok(p),
            TaskProgress::Done(v) => {
                *self = Part::Done(v);
                Ok(1.0)
            }
            TaskProgress::Failed(e) => Err(e),
        }
    }
    fn stop(&mut self) {
        if let Part::Running(t) = self {
            t.stop();
        }
    }
    fn on_done(&self) -> bool {
        matches!(self, Part::Done(_))
    }
    fn take(self) -> Option<T> {
        match self {
            Part::Done(v) => Some(v),
            Part::Running(_) => None,
        }
    }
}

enum Job {
    /// La transcription et la séparation des locuteurs tournent en parallèle.
    Transcription { words: Part<Vec<Word>>, turns: Part<Vec<SpeakerTurn>> },
    Review(Part<ReviewResult>),
    Summary { result: Part<SummaryResult> },
}

impl Job {
    fn stage(&self) -> Stage {
        match self {
            Job::Transcription { .. } => Stage::Transcription,
            Job::Review(_) => Stage::Review,
            Job::Summary { .. } => Stage::Summary,
        }
    }
    fn stop(&mut self) {
        match self {
            Job::Transcription { words, turns } => {
                words.stop();
                turns.stop();
            }
            Job::Review(p) => p.stop(),
            Job::Summary { result, .. } => result.stop(),
        }
    }
}

struct Processing {
    meeting: MeetingId,
    job: Job,
}

enum StageOutput {
    Transcription(Vec<Word>, Vec<SpeakerTurn>),
    Review(ReviewResult),
    Summary(SummaryResult),
}

/// Le cœur de Greffier.
pub struct Greffier {
    folder: MeetingsFolder,
    adapters: Adapters,
    settings: Settings,
    /// Les plus récentes d'abord.
    meetings: Vec<Meeting>,
    next_id: MeetingId,
    recording: Option<Recording>,
    /// Les réunions en attente, dans l'ordre où elles seront traitées.
    file: VecDeque<MeetingId>,
    processing: Option<Processing>,
    installation: Option<BoxedTask<()>>,
    verification: Option<BoxedTask<()>>,
    /// L'état des ressources, relu à chaque avancée.
    resources: Vec<Resource>,
    /// L'empreinte de chaque dossier de réunion au dernier passage du cœur (voir
    /// [`Greffier::relire_le_dossier`]).
    fingerprints: HashMap<String, folder::watch::Fingerprints>,
    /// L'index de la recherche ⌘K (voir [`Greffier::rechercher`]).
    index: search::Index,
}

impl Greffier {
    /// Le cœur sur un dossier de réunions, avec les réunions qu'il contient déjà.
    pub fn open(folder: impl Into<PathBuf>, adapters: Adapters) -> Greffier {
        let folder = MeetingsFolder::new(folder);
        let meetings = folder.read_all(1);
        Greffier::with_meetings(folder.root(), adapters, meetings)
    }

    /// Le cœur avec des réunions déjà connues (les données fictives du prototype).
    pub fn with_meetings(folder: impl Into<PathBuf>, adapters: Adapters, mut meetings: Vec<Meeting>) -> Greffier {
        let mut file = VecDeque::new();
        for r in &mut meetings {
            if let Status::Running { stage, .. } = r.status {
                r.status = Status::Pending(stage);
            }
            if let Status::Pending(_) = r.status {
                file.push_back(r.id);
            }
        }
        let next_id = meetings.iter().map(|r| r.id + 1).max().unwrap_or(1);
        let mut adapters = adapters;
        let resources = adapters.resources.state();
        let folder = MeetingsFolder::new(folder);
        let fingerprints = folder.all_fingerprints();
        let mut greffier = Greffier {
            folder,
            adapters,
            settings: Settings::default(),
            meetings,
            next_id,
            recording: None,
            file,
            processing: None,
            installation: None,
            verification: None,
            resources,
            fingerprints,
            index: search::Index::default(),
        };
        greffier.indexer();
        greffier
    }

    // --- Lecture de l'état ---------------------------------------------------------------

    pub fn meetings(&self) -> &[Meeting] {
        &self.meetings
    }

    pub fn meeting(&self, id: MeetingId) -> Option<&Meeting> {
        self.meetings.iter().find(|r| r.id == id)
    }

    pub fn recording(&self) -> Option<&Recording> {
        self.recording.as_ref()
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Le dossier d'une réunion sur le disque.
    pub fn meeting_folder(&self, id: MeetingId) -> Option<PathBuf> {
        self.meeting(id).map(|r| self.folder.folder(&r.folder_name))
    }

    /// Le fichier de l'enregistrement d'une réunion, s'il existe (un média importé qui n'a pas
    /// pu passer en MP4 garde son extension).
    pub fn recording_file(&self, id: MeetingId) -> Option<PathBuf> {
        self.meeting(id).and_then(|r| self.folder.recording(&r.folder_name))
    }

    /// Le lexique des réglages, lu maintenant ; sans lexique (ou illisible), le lexique livré.
    fn lexicon_text(&self) -> String {
        self.settings.lexicon.as_ref().and_then(|c| std::fs::read_to_string(c).ok()).unwrap_or_else(|| lexicon::INITIAL_LEXICON.to_string())
    }

    /// Ce que compte le lexique (personnes, mots justes, erreurs connues), pour les réglages.
    pub fn lexicon(&self) -> lexicon::Overview {
        lexicon::preview(&self.lexicon_text())
    }

    /// Les personnes du lexique, proposées pour nommer les locuteurs.
    pub fn people(&self) -> Vec<String> {
        lexicon::people(&self.lexicon_text())
    }

    pub fn folder(&self) -> &MeetingsFolder {
        &self.folder
    }

    /// Le rang d'une réunion dans la file : 0 pour celle en cours de traitement.
    pub fn rank(&self, id: MeetingId) -> Option<usize> {
        let running = self.processing.as_ref().map(|t| t.meeting);
        running.into_iter().chain(self.file.iter().copied()).position(|x| x == id)
    }

    /// Le temps estimé avant la fin du traitement d'une réunion de la file, en secondes : le reste
    /// de la sienne et celui des réunions qui passent avant elle.
    pub fn time_remaining(&self, id: MeetingId) -> Option<f64> {
        let rank = self.rank(id)?;
        let running = self.processing.as_ref().map(|t| t.meeting);
        running.into_iter().chain(self.file.iter().copied()).take(rank + 1).map(|x| self.meeting(x).and_then(Meeting::estimated_time_remaining)).sum()
    }

    pub fn today(&self) -> DateTime {
        self.adapters.clock.now()
    }

    /// Les autorisations de macOS dont les sources ont besoin.
    pub fn permissions(&self) -> Permissions {
        self.adapters.capture.permissions()
    }

    /// Les modèles, l'environnement Python et la vérification de `claude`, avec leur état et la
    /// place qu'ils prennent.
    pub fn resources(&self) -> &[Resource] {
        &self.resources
    }

    pub fn installing(&self) -> bool {
        self.installation.is_some()
    }

    pub fn verification_in_progress(&self) -> bool {
        self.verification.is_some()
    }

    /// Ce que la fenêtre d'import propose pour des fichiers déposés ou choisis : le média, la
    /// transcription et le bilan, la date et le titre, à modifier avant d'envoyer
    /// [`Commande::Importer`]. N'écrit rien.
    pub fn propose_import(&self, files: &[PathBuf]) -> Result<folder::Import, Rejection> {
        folder::import::propose(files, |t| self.adapters.clock.to_local_time(t)).map_err(Rejection::Impossible)
    }

    /// Vrai tant que le temps compte : enregistrement, traitement, file, installation ou
    /// vérification des ressources.
    pub fn busy(&self) -> bool {
        self.recording.is_some() || self.processing.is_some() || !self.file.is_empty() || self.installation.is_some() || self.verification.is_some()
    }

    /// Pourquoi le Mac ne doit pas se mettre en veille automatiquement, tant qu'il ne le doit
    /// pas : pendant un enregistrement (l'écran aussi), et pendant un traitement (une réunion en
    /// cours ou en attente dans la file). L'interface en fait une assertion de non-veille de macOS.
    pub fn keep_mac_awake(&self) -> Option<Wake> {
        if self.recording.is_some() {
            Some(Wake { reason: "Greffier enregistre une réunion", screen: true })
        } else if self.processing.is_some() || !self.file.is_empty() {
            Some(Wake { reason: "Greffier traite une réunion", screen: false })
        } else {
            None
        }
    }

    // --- Commandes -----------------------------------------------------------------------

    pub fn execute(&mut self, command: Command) -> Result<Vec<Event>, Rejection> {
        let mut ev = Vec::new();
        let affected = match &command {
            Command::SpeakerCount { meeting, .. }
            | Command::Redo { meeting, .. }
            | Command::NameSpeaker { meeting, .. }
            | Command::MergeSpeakers { meeting, .. }
            | Command::SplitSpeaker { meeting, .. }
            | Command::Resolve { meeting, .. }
            | Command::CorrectUtterance { meeting, .. }
            | Command::CheckAction { meeting, .. } => Some(*meeting),
            Command::Process(id) | Command::StopProcessing(id) | Command::MarkRead(id) => Some(*id),
            _ => None,
        };
        match command {
            Command::StartRecording => self.start_recording()?,
            Command::StopRecording => self.stop_recording(&mut ev)?,
            Command::AddMarker => {
                let Some(e) = &mut self.recording else { return impossible("aucun enregistrement en cours") };
                e.markers.push(e.duration);
                self.write_recording_sheet();
            }
            Command::RecordingSpeakerCount(n) => {
                let Some(e) = &mut self.recording else { return impossible("aucun enregistrement en cours") };
                e.speaker_count = n;
                self.write_recording_sheet();
            }
            Command::SpeakerCount { meeting, number } => self.meeting_mut(meeting)?.speaker_count = number,
            Command::Process(id) => {
                let r = self.meeting_mut(id)?;
                let stage = match &r.status {
                    Status::Failed { stage, .. } => *stage,
                    Status::ToProcess | Status::RecordingInterrupted => match r.completed_stage {
                        None => Stage::Transcription,
                        Some(e) => match e.next() {
                            Some(s) => s,
                            None => return impossible("la réunion est déjà traitée"),
                        },
                    },
                    s => return impossible(format!("la réunion ne peut pas être traitée ({s:?})")),
                };
                self.enqueue(id, stage);
            }
            Command::Redo { meeting, stage } => {
                let r = self.meeting_mut(meeting)?;
                if !r.can_redo(stage) {
                    return impossible(if r.status.in_queue() { "la réunion est déjà dans la file" } else { "les étapes précédentes ne sont pas faites" });
                }
                r.completed_stage = stage.previous();
                self.enqueue(meeting, stage);
            }
            Command::StopProcessing(id) => {
                let r = self.meeting_mut(id)?;
                if !r.status.in_queue() {
                    return impossible("la réunion n'est pas en traitement");
                }
                r.status = Status::ToProcess;
                self.remove_from_queue(id);
            }
            Command::Delete(id) => self.delete(id)?,
            Command::NameSpeaker { meeting, speaker, name } => {
                let name = name.trim();
                if name.is_empty() {
                    return impossible("le nom est vide");
                }
                let r = self.meeting_mut(meeting)?;
                speaker_mut(r, speaker)?;
                // Un locuteur fusionné porte le nom de celui qui le remplace.
                let i = r.displayed_speaker(speaker);
                let l = &mut r.speakers[i];
                l.name = Some(name.to_string());
                l.suggested_merge = None;
            }
            Command::MergeSpeakers { meeting, speaker, into } => {
                let r = self.meeting_mut(meeting)?;
                speaker_mut(r, speaker)?;
                speaker_mut(r, into)?;
                let (i, j) = (r.displayed_speaker(speaker), r.displayed_speaker(into));
                if !r.merge(i, j) {
                    return impossible("ces deux locuteurs sont déjà la même voix");
                }
                // Le nom suit la voix : celui qui reste prend le nom de l'autre s'il n'en a pas.
                // Un locuteur fusionné n'a pas de nom à lui.
                let name = r.speakers[i].name.take();
                if r.speakers[j].name.is_none() {
                    r.speakers[j].name = name;
                }
                r.speakers[speaker].suggested_merge = None;
                r.speakers[i].suggested_merge = None;
            }
            Command::SplitSpeaker { meeting, speaker } => {
                let l = speaker_mut(self.meeting_mut(meeting)?, speaker)?;
                if l.merged_into.take().is_none() {
                    return impossible("ce locuteur n'est pas fusionné");
                }
            }
            Command::Resolve { meeting, passage, resolution } => {
                let r = self.meeting_mut(meeting)?;
                transcription_editable(r)?;
                resolve(r, passage, resolution)?;
            }
            Command::CorrectUtterance { meeting, utterance, text } => {
                let r = self.meeting_mut(meeting)?;
                transcription_editable(r)?;
                correct_utterance(r, utterance, text)?;
            }
            Command::CheckAction { meeting, action, done } => {
                let r = self.meeting(meeting).ok_or(Rejection::UnknownMeeting(meeting))?;
                if action >= summary::checkbox_count(&r.summary) {
                    return impossible("action inconnue");
                }
                // Le fichier du bilan est celui de l'agent : on n'y change que la case, et il fait foi.
                let name = r.folder_name.clone();
                let file = self.folder.tick_in_summary(&name, action, done).map_err(Rejection::Impossible)?;
                let r = self.meeting_mut(meeting)?;
                match file {
                    Some(md) => r.set_summary(md),
                    None => _ = summary::tick(&mut r.summary, action, done),
                }
                self.note_end(&name, &[Role::Summary]);
            }
            Command::MarkRead(id) => self.meeting_mut(id)?.is_new = false,
            Command::ChangeTitle { meeting, title } => {
                let title = title.trim();
                if title.is_empty() {
                    return impossible("le titre est vide");
                }
                let r = self.meeting_mut(meeting)?;
                if matches!(r.status, Status::Running { .. }) {
                    return impossible("le titre pourra changer à la fin de l'étape en cours");
                }
                r.title = title.to_string();
                self.rename_from_title(meeting, &mut ev);
            }
            Command::ChangeSettings(r) => {
                let cache = (r.search_cache != self.settings.search_cache).then(|| r.search_cache.clone());
                self.settings = r;
                if let Some(cache) = cache {
                    self.use_search_cache(cache);
                }
            }
            Command::RequestPermission(a) => self.adapters.capture.request_permission(a),
            Command::InstallResources => {
                if self.installation.is_none() {
                    // L'installation refait aussi les vérifications.
                    if let Some(mut v) = self.verification.take() {
                        v.stop();
                    }
                    self.installation = Some(self.adapters.resources.install());
                    self.resources = self.adapters.resources.state();
                }
            }
            Command::CheckResources => {
                if self.installation.is_none() && self.verification.is_none() {
                    self.verification = Some(self.adapters.resources.check());
                    self.resources = self.adapters.resources.state();
                }
            }
            Command::Import(import) => {
                let id = self.next_id;
                let r = self.folder.import(&import, id, self.adapters.mp4_muxing.as_ref()).map_err(Rejection::Impossible)?;
                // Les fichiers copiés par Greffier ne sont pas des modifications faites hors de l'app.
                self.note_end(&r.folder_name.clone(), &Role::ALL);
                self.next_id += 1;
                // Les plus récentes d'abord.
                let storage = self.meetings.iter().position(|x| x.start <= r.start).unwrap_or(self.meetings.len());
                self.meetings.insert(storage, r);
                ev.push(Event::MeetingImported(id));
                self.persist(id, &mut ev);
            }
        }
        if let Some(id) = affected {
            self.persist(id, &mut ev);
        }
        self.indexer();
        Ok(ev)
    }

    /// Écrit les fichiers de la réunion (fiche, transcriptions) et note ceux qui sont présents.
    fn persist(&mut self, id: MeetingId, ev: &mut Vec<Event>) {
        let Some(r) = self.meeting(id) else { return };
        let name = r.folder_name.clone();
        let result = self.folder.write_files(r);
        let files = self.folder.present_roles(&name);
        if let Ok(r) = self.meeting_mut(id) {
            r.files = files;
        }
        self.note_end(&name, result.as_deref().unwrap_or_default());
        if let Err(message) = result {
            ev.push(Event::WriteFailed { meeting: id, message });
        }
    }

    fn meeting_mut(&mut self, id: MeetingId) -> Result<&mut Meeting, Rejection> {
        self.meetings.iter_mut().find(|r| r.id == id).ok_or(Rejection::UnknownMeeting(id))
    }

    fn start_recording(&mut self) -> Result<(), Rejection> {
        if self.recording.is_some() {
            return impossible("un enregistrement est déjà en cours");
        }
        let sources = self.settings.sources;
        if sources.none_enabled() {
            return impossible("Aucune source à enregistrer : active l'écran, le son du Mac ou le micro dans les réglages.");
        }
        let permissions = self.adapters.capture.permissions();
        for a in sources.permissions() {
            match permissions.state(a) {
                PermissionState::Granted => {}
                PermissionState::ToRequest => {
                    self.adapters.capture.request_permission(a);
                    return impossible(format!("Greffier demande l'autorisation pour « {} » : accepte la demande de macOS, puis relance l'enregistrement.", a.name().to_lowercase()));
                }
                PermissionState::Denied => {
                    return impossible(format!("Greffier n'a pas l'autorisation pour « {} » : Réglages Système › Confidentialité et sécurité › {}, active Greffier.", a.name(), a.section()));
                }
            }
        }
        let start = self.adapters.clock.now();
        let name = self.folder.available_name(start, "Réunion");
        self.folder.create(&name).map_err(|e| Rejection::Impossible(format!("dossier de la réunion : {e}")))?;
        let request = CaptureRequest { file: self.folder.file(&name, Role::Recording), sources };
        self.adapters.capture.start(request).map_err(Rejection::Impossible)?;
        self.recording = Some(Recording { start, duration: 0.0, levels: Levels::default(), markers: vec![], speaker_count: None, folder_name: name, sources });
        self.write_recording_sheet();
        Ok(())
    }

    /// La fiche de l'enregistrement en cours, réécrite à chaque repère : si l'app ou le Mac
    /// s'arrête net, la réunion est relue « enregistrement interrompu » avec ses repères et son
    /// nombre de locuteurs (la durée vient alors des fragments déjà écrits de l'enregistrement).
    fn write_recording_sheet(&self) {
        let Some(e) = &self.recording else { return };
        let mut r = recorded_meeting(0, e, 0.0);
        r.status = Status::Recording;
        let _ = self.folder.write(&r);
    }

    fn stop_recording(&mut self, ev: &mut Vec<Event>) -> Result<(), Rejection> {
        let Some(e) = self.recording.take() else { return impossible("aucun enregistrement en cours") };
        let duration = match self.adapters.capture.stop() {
            Ok(duration) => duration,
            Err(message) => {
                self.keep_interrupted_recording(&e, None, message, ev);
                return Ok(());
            }
        };
        let id = self.next_id;
        self.next_id += 1;
        self.meetings.insert(0, recorded_meeting(id, &e, duration));
        ev.push(Event::MeetingRecorded(id));
        if duration < MIN_DURATION {
            ev.push(Event::DeletionProposed(id));
        }
        if self.settings.process_on_stop {
            self.enqueue(id, Stage::Transcription);
        }
        self.persist(id, ev);
        Ok(())
    }

    /// Garde la réunion d'un enregistrement qui ne s'est pas fini normalement, « enregistrement
    /// interrompu », avec ce qui a été capturé. La durée est celle que la capture a rendue,
    /// sinon celle des fragments déjà écrits, sinon la dernière connue. Elle n'est pas mise en
    /// file : on la traite quand on veut.
    fn keep_interrupted_recording(&mut self, e: &Recording, duration: Option<f64>, message: String, ev: &mut Vec<Event>) {
        let file = self.folder.file(&e.folder_name, Role::Recording);
        let duration = duration.or_else(|| folder::mp4_duration(&file)).unwrap_or(e.duration);
        let id = self.next_id;
        self.next_id += 1;
        let mut r = recorded_meeting(id, e, duration);
        r.status = Status::RecordingInterrupted;
        self.meetings.insert(0, r);
        ev.push(Event::RecordingInterrupted { meeting: id, message });
        self.persist(id, ev);
    }

    fn enqueue(&mut self, id: MeetingId, stage: Stage) {
        if let Ok(r) = self.meeting_mut(id) {
            r.status = Status::Pending(stage);
            self.file.push_back(id);
        }
    }

    /// Arrête le traitement de la réunion s'il tourne, et la retire de la file.
    fn remove_from_queue(&mut self, id: MeetingId) {
        if let Some(mut t) = self.processing.take_if(|t| t.meeting == id) {
            t.job.stop();
        }
        self.file.retain(|x| *x != id);
    }

    // --- Le temps ------------------------------------------------------------------------

    /// Fait passer `ecoule` secondes : la capture, l'installation des ressources et la file de
    /// traitement avancent. Une seule réunion est traitée à la fois ; pendant un enregistrement,
    /// aucune étape ne démarre (celle qui tourne va jusqu'au bout).
    pub fn advance(&mut self, elapsed: f64) -> Vec<Event> {
        let mut ev = Vec::new();
        if let Some(e) = &mut self.recording {
            let state = self.adapters.capture.poll(elapsed);
            e.duration = state.duration;
            e.levels = state.levels;
            if let Some(message) = state.error
                && let Some(e) = self.recording.take()
            {
                // Finir le fichier avec ce qui a été capturé.
                let duration = self.adapters.capture.stop().ok();
                self.keep_interrupted_recording(&e, duration, message, &mut ev);
            }
        }
        if let Some(t) = &mut self.installation {
            match t.poll(elapsed) {
                TaskProgress::Running(_) => {}
                TaskProgress::Done(()) => {
                    self.installation = None;
                    ev.push(Event::ResourcesInstalled);
                }
                TaskProgress::Failed(m) => {
                    self.installation = None;
                    ev.push(Event::InstallationFailed(m));
                }
            }
        }
        if let Some(t) = &mut self.verification
            && !matches!(t.poll(elapsed), TaskProgress::Running(_))
        {
            self.verification = None;
        }
        self.resources = self.adapters.resources.state();
        if self.processing.is_none()
            && self.recording.is_none()
            && let Some(id) = self.file.pop_front()
            && let Some(Status::Pending(stage)) = self.meeting(id).map(|r| r.status.clone())
        {
            self.start_stage(id, stage, &mut ev);
        }
        self.advance_processing(elapsed, &mut ev);
        // Une étape finie, un enregistrement arrêté : des réunions ont changé.
        if !ev.is_empty() {
            self.indexer();
        }
        ev
    }

    fn start_stage(&mut self, id: MeetingId, stage: Stage, ev: &mut Vec<Event>) {
        match self.prepare(id, stage) {
            Ok(job) => {
                if let Ok(r) = self.meeting_mut(id) {
                    r.status = Status::Running { stage, progress: 0.0 };
                }
                self.processing = Some(Processing { meeting: id, job });
                self.persist(id, ev);
            }
            Err(message) => self.failure(id, stage, message, ev),
        }
    }

    fn prepare(&mut self, id: MeetingId, stage: Stage) -> Result<Job, String> {
        let r = self.meeting(id).ok_or("réunion inconnue")?.clone();
        let audio = self.folder.recording(&r.folder_name).unwrap_or_else(|| self.folder.file(&r.folder_name, Role::Recording));
        let a = &mut self.adapters;
        self.resources = a.resources.state();
        let missing: Vec<&str> = self.resources.iter().filter(|x| x.stages.contains(&stage) && x.state != ResourceState::Ready).map(|x| x.name.as_str()).collect();
        if !missing.is_empty() {
            return Err(format!("Ressources manquantes : {}.", missing.join(", ")));
        }
        Ok(match stage {
            Stage::Transcription => {
                let vocabulary = self.settings.lexicon.as_ref().and_then(|l| std::fs::read_to_string(l).ok()).map(|md| lexicon::vocabulary(&md)).unwrap_or_default();
                let words = a.transcriber.transcribe(TranscriptionRequest { audio: audio.clone(), duration: r.duration, vocabulary });
                let turns = a.diarizer.diarize(DiarizationRequest { audio, duration: r.duration, speaker_count: r.speaker_count });
                Job::Transcription { words: Part::Running(words), turns: Part::Running(turns) }
            }
            Stage::Review => {
                let lexicon = self.settings.lexicon.as_ref().and_then(|l| std::fs::read_to_string(l).ok()).unwrap_or_default();
                let request = ReviewRequest {
                    recording: audio,
                    meetings_folder: self.folder.root().to_path_buf(),
                    folder_name: r.folder_name.clone(),
                    title: r.title.clone(),
                    start: r.start,
                    speaker_count: r.speaker_count,
                    // Une relecture refaite repart de la transcription brute.
                    utterances: for_agent(&r, if r.raw.is_empty() { &r.utterances } else { &r.raw }),
                    vocabulary: lexicon::vocabulary(&lexicon),
                    lexicon,
                };
                Job::Review(Part::Running(a.agent.review(request)))
            }
            Stage::Summary => {
                self.folder.create(&r.folder_name)?;
                let request = SummaryRequest {
                    meetings_folder: self.folder.root().to_path_buf(),
                    context_folders: self.settings.context_folders.clone(),
                    summary_file: self.folder.file(&r.folder_name, Role::Summary),
                    sheet: self.folder.file(&r.folder_name, Role::Sheet),
                    transcription: self.folder.file(&r.folder_name, Role::ReadableTranscription),
                    title: r.title.clone(),
                    start: r.start,
                    duration: r.duration,
                    utterances: for_agent(&r, &r.utterances),
                    speakers: r
                        .speakers
                        .iter()
                        .map(|l| AgentSpeaker {
                            label: l.label.clone(),
                            name: l.name.clone(),
                            proposal: l.proposal.clone(),
                            justification: l.justification.clone(),
                            proposed_same_voice: l.suggested_merge.map(|j| r.speakers[j].label.clone()),
                            merged_into: l.merged_into.map(|j| r.speakers[r.displayed_speaker(j)].label.clone()),
                        })
                        .collect(),
                    markers: r.markers.clone(),
                    current_summary: (!r.summary.is_empty()).then(|| r.summary.clone()),
                };
                Job::Summary { result: Part::Running(a.agent.summarize(request)) }
            }
        })
    }

    fn advance_processing(&mut self, elapsed: f64, ev: &mut Vec<Event>) {
        let Some(t) = &mut self.processing else { return };
        let (id, stage) = (t.meeting, t.job.stage());
        let issue = |p: Result<f64, String>, on_done: bool| match p {
            Err(e) => TaskProgress::Failed(e),
            Ok(_) if on_done => TaskProgress::Done(()),
            Ok(x) => TaskProgress::Running(x),
        };
        let issue = match &mut t.job {
            // Les deux moitiés tournent en même temps : l'étape avance comme la plus lente.
            Job::Transcription { words, turns } => match (words.poll(elapsed), turns.poll(elapsed)) {
                (Ok(a), Ok(b)) => issue(Ok(a.min(b)), words.on_done() && turns.on_done()),
                (Err(e), _) | (_, Err(e)) => TaskProgress::Failed(e),
            },
            Job::Review(p) => {
                let a = p.poll(elapsed);
                issue(a, p.on_done())
            }
            Job::Summary { result, .. } => {
                let a = result.poll(elapsed);
                issue(a, result.on_done())
            }
        };
        match issue {
            TaskProgress::Running(p) => {
                if let Ok(r) = self.meeting_mut(id) {
                    r.status = Status::Running { stage, progress: p };
                }
            }
            TaskProgress::Failed(message) => {
                if let Some(mut t) = self.processing.take() {
                    t.job.stop();
                }
                self.failure(id, stage, message, ev);
            }
            TaskProgress::Done(()) => {
                let Some(t) = self.processing.take() else { return };
                let produced = match t.job {
                    Job::Transcription { words, turns } => StageOutput::Transcription(words.take().unwrap_or_default(), turns.take().unwrap_or_default()),
                    Job::Review(p) => StageOutput::Review(p.take().unwrap_or_default()),
                    Job::Summary { result } => match result.take() {
                        Some(r) => StageOutput::Summary(r),
                        None => return,
                    },
                };
                self.apply(id, stage, produced, ev);
            }
        }
    }

    /// Applique le produit d'une étape finie, puis lance la suivante (ou la met en attente
    /// pendant un enregistrement).
    fn apply(&mut self, id: MeetingId, stage: Stage, produced: StageOutput, ev: &mut Vec<Event>) {
        // La session de l'agent est gardée même si son bilan est refusé : on peut la reprendre.
        if let StageOutput::Summary(result) = &produced
            && let Ok(r) = self.meeting_mut(id)
        {
            r.sessions.summary = result.session.clone();
        }
        let name = self.meeting(id).map(|r| r.folder_name.clone()).unwrap_or_default();
        let contents = match &produced {
            StageOutput::Summary(..) => match self.folder.read_summary(&name) {
                Ok(md) => Some(md),
                Err(e) => return self.failure(id, stage, format!("L'agent n'a pas écrit le bilan ({e})."), ev),
            },
            _ => None,
        };
        if let Some(missing) = contents.as_deref().and_then(summary::missing_parts) {
            return self.failure(id, stage, missing, ev);
        }
        // Le bilan vient d'être écrit par l'agent pour Greffier : ce n'est pas une modification
        // faite hors de l'app.
        if contents.is_some()
            && let Some(name) = self.meeting(id).map(|r| r.folder_name.clone())
        {
            self.note_end(&name, &[Role::Summary]);
        }
        let added = match &produced {
            StageOutput::Review(result) => self.enrich_lexicon(id, &result.lexicon_additions, ev),
            _ => Vec::new(),
        };
        let is_recording = self.recording.is_some();
        let Ok(r) = self.meeting_mut(id) else { return };
        match produced {
            StageOutput::Transcription(words, turns) => {
                let a = attribution::attribute(attribution::without_noises(words), turns);
                r.words = a.words;
                r.turns = a.turns;
                r.speakers = a.speakers;
                r.utterances = a.utterances;
                r.passages.clear();
                r.corrections = 0;
                r.raw.clear();
                r.review = None;
                r.manual_corrections.clear();
            }
            StageOutput::Review(result) => apply_review(r, result, added),
            StageOutput::Summary(result) => {
                r.set_summary(contents.unwrap_or_default());
                r.title = result.title;
                r.brief = result.brief;
                r.key_actions = result.actions.into_iter().take(3).collect();
                for p in result.speakers {
                    let index = |label: &str| r.speakers.iter().position(|l| l.label == label);
                    let Some(i) = index(&p.speaker) else { continue };
                    let same = p.same_voice_as.as_deref().and_then(index).filter(|j| r.displayed_speaker(*j) != r.displayed_speaker(i));
                    let l = &mut r.speakers[i];
                    // Un nom vide : l'agent n'a pas su qui c'est, sa justification dit pourquoi.
                    l.proposal = Some(p.proposed_name).filter(|n| !n.trim().is_empty());
                    l.justification = p.justification;
                    l.suggested_merge = same;
                }
            }
        }
        r.completed_stage = Some(stage);
        ev.push(Event::StageDone { meeting: id, stage });
        match stage.next() {
            None => {
                r.status = Status::Done;
                r.is_new = true;
                ev.push(Event::SummaryReady { meeting: id, title: r.title.clone() });
                self.rename_from_title(id, ev);
            }
            Some(next) if is_recording => {
                r.status = Status::Pending(next);
                self.file.push_front(id);
                self.persist(id, ev);
            }
            Some(next) => self.start_stage(id, next, ev),
        }
    }

    /// Écrit dans le lexique les erreurs apprises par la relecture qu'il ne connaît pas encore ;
    /// rend celles qui ont été écrites. Sans lexique, rien n'est écrit.
    fn enrich_lexicon(&mut self, id: MeetingId, additions: &[LexiconAddition], ev: &mut Vec<Event>) -> Vec<LexiconAddition> {
        let Some(path) = self.settings.lexicon.clone() else { return Vec::new() };
        if additions.is_empty() {
            return Vec::new();
        }
        let md = std::fs::read_to_string(&path).unwrap_or_default();
        let (md, added) = lexicon::add_errors(&md, additions);
        if added.is_empty() {
            return added;
        }
        match lexicon::write(&path, &md) {
            Ok(()) => added,
            Err(e) => {
                ev.push(Event::WriteFailed { meeting: id, message: format!("{} : {e}", path.display()) });
                Vec::new()
            }
        }
    }

    fn failure(&mut self, id: MeetingId, stage: Stage, message: String, ev: &mut Vec<Event>) {
        if let Ok(r) = self.meeting_mut(id) {
            r.status = Status::Failed { stage, message: message.clone() };
        }
        ev.push(Event::StageFailed { meeting: id, stage, message });
        self.persist(id, ev);
    }
}

/// La réunion d'un enregistrement : son dossier, son début, ses repères et son nombre de
/// locuteurs. Durée et repères au centième de seconde, comme dans la fiche.
fn recorded_meeting(id: MeetingId, e: &Recording, duration: f64) -> Meeting {
    let hundredth = |t: f64| (t * 100.0).round() / 100.0;
    let mut r = Meeting::new(id, e.folder_name.clone(), e.start, hundredth(duration));
    r.speaker_count = e.speaker_count;
    r.markers = e.markers.iter().enumerate().map(|(i, t)| Marker { instant: hundredth(*t), label: format!("Repère {}", i + 1) }).collect();
    r
}

fn speaker_mut(r: &mut Meeting, i: usize) -> Result<&mut Speaker, Rejection> {
    r.speakers.get_mut(i).ok_or(Rejection::Impossible("locuteur inconnu".into()))
}

/// La transcription ne se corrige pas pendant qu'une étape va la refaire : la transcription
/// repart de zéro, la relecture rouvre tous les passages à vérifier.
fn transcription_editable(r: &Meeting) -> Result<(), Rejection> {
    match r.status {
        Status::Running { stage, .. } | Status::Pending(stage) if stage <= Stage::Review => {
            impossible("la transcription est en cours de traitement : attends la fin de la relecture")
        }
        _ => Ok(()),
    }
}

/// Garde le changement du texte d'une réplique comme correction à la main.
fn correct_utterance(r: &mut Meeting, utterance: usize, text: String) -> Result<(), Rejection> {
    let text = text.trim();
    if text.is_empty() {
        return impossible("une réplique ne peut pas être vide");
    }
    let Some(x) = r.utterances.get(utterance) else { return impossible("réplique inconnue") };
    let Some((start, end, after)) = word_difference(&x.text, text) else { return Ok(()) };
    // Les passages à vérifier de la réplique se tranchent dans leur onglet.
    for p in x.passages.iter().filter_map(|&i| r.passages.get(i)) {
        let shown = p.text(&p.status);
        if let Some(a) = x.text.find(shown)
            && start < a + shown.len()
            && a < end
        {
            return impossible("ce passage est à vérifier : il se tranche dans l'onglet À vérifier");
        }
    }
    let before = x.text[start..end].to_string();
    r.utterances[utterance].text = text.to_string();
    r.manual_corrections.push(ManualCorrection { utterance, before, after });
    Ok(())
}

/// Ce qui change de `avant` à `apres`, élargi aux mots entiers : la plage d'octets de `avant` et
/// le texte qui la remplace. `None` s'ils sont identiques.
fn word_difference(before: &str, after: &str) -> Option<(usize, usize, String)> {
    if before == after {
        return None;
    }
    // Le début commun, puis la fin commune sans chevaucher le début.
    let mut start = before.char_indices().zip(after.chars()).find(|((_, a), b)| a != b).map_or(before.len().min(after.len()), |((i, _), _)| i);
    while !before.is_char_boundary(start) || !after.is_char_boundary(start) {
        start -= 1;
    }
    let mut common = 0;
    for (a, b) in before[start..].chars().rev().zip(after[start..].chars().rev()) {
        if a != b {
            break;
        }
        common += a.len_utf8();
    }
    let (mut end_before, mut end_after) = (before.len() - common, after.len() - common);
    // Aux mots entiers : le début recule et la fin avance (dans la partie commune) jusqu'à un blanc.
    while let Some(c) = before[..start].chars().next_back().filter(|c| !c.is_whitespace()) {
        start -= c.len_utf8();
    }
    while let Some(c) = before[end_before..].chars().next().filter(|c| !c.is_whitespace()) {
        end_before += c.len_utf8();
        end_after += c.len_utf8();
    }
    Some((start, end_before, after[start..end_after].to_string()))
}

/// Tranche un passage à vérifier : le texte de la réplique passe de ce que le passage y montre
/// (l'extrait, l'hypothèse ou la correction) à ce que la tranche y met.
fn resolve(r: &mut Meeting, passage: usize, resolution: Resolution) -> Result<(), Rejection> {
    let Some(p) = r.passages.get(passage) else { return impossible("passage inconnu") };
    let status = match resolution {
        Resolution::AcceptHypothesis if p.hypothesis.is_none() => return impossible("ce passage n'a pas d'hypothèse"),
        Resolution::AcceptHypothesis => PassageStatus::HypothesisAccepted,
        Resolution::KeepOriginal => PassageStatus::OriginalKept,
        Resolution::Correct(text) if text.trim().is_empty() => return impossible("la correction est vide"),
        Resolution::Correct(text) => PassageStatus::CorrectedManually(text.trim().to_string()),
        Resolution::Reopen => PassageStatus::Pending,
    };
    let (current, new) = (p.text(&p.status).to_string(), p.text(&status).to_string());
    let utterance = p.utterance;
    let Some(x) = r.utterances.get_mut(utterance) else { return impossible("réplique inconnue") };
    if current != new && !replace(&mut x.text, &current, &new) {
        return impossible(format!("« {current} » n'est plus dans la réplique : elle a été corrigée à la main"));
    }
    r.passages[passage].status = status;
    Ok(())
}

/// Remplace la première occurrence de `avant` par `apres` ; rend `false` si `avant` est vide ou
/// absent.
fn replace(text: &mut String, before: &str, after: &str) -> bool {
    match text.find(before).filter(|_| !before.is_empty()) {
        Some(i) => {
            text.replace_range(i..i + before.len(), after);
            true
        }
        None => false,
    }
}

/// Une hypothèse qui n'en est pas une (vide, « ? », « ? (peut-être…) ») n'est pas gardée.
fn hypothesis(h: Option<String>) -> Option<String> {
    h.map(|h| h.trim().to_string()).filter(|h| !h.is_empty() && !h.starts_with('?'))
}

/// Applique le résultat de la relecture, toujours à partir de la transcription brute (une
/// relecture refaite ne s'empile pas sur la précédente). Ce qui ne s'applique pas (réplique ou
/// locuteur inconnus, texte absent) est écarté et n'entre pas au journal. `ajoutes` : les erreurs
/// vraiment écrites dans le lexique.
fn apply_review(r: &mut Meeting, result: ReviewResult, added: Vec<LexiconAddition>) {
    if r.raw.is_empty() {
        r.raw = r.utterances.iter().map(|x| Utterance { passages: vec![], ..x.clone() }).collect();
    }
    r.utterances = r.raw.clone();
    let mut log = ReviewLog { lexicon_additions: added, ..Default::default() };
    for c in result.corrections {
        let Some(x) = r.utterances.get_mut(c.utterance) else { continue };
        if c.before != c.after && replace(&mut x.text, &c.before, &c.after) {
            log.corrections.push(c);
        }
    }
    for c in result.speaker_changes {
        let Some(i) = r.speakers.iter().position(|l| l.label == c.speaker) else { continue };
        let Some(x) = r.utterances.get(c.utterance) else { continue };
        if r.displayed_speaker(x.speaker) == r.displayed_speaker(i) {
            continue;
        }
        r.utterances[c.utterance].speaker = i;
        log.speaker_changes.push(c);
    }
    let mut to_check = Vec::new();
    for p in result.corrected_passages {
        let Some(x) = r.utterances.get_mut(p.utterance) else { continue };
        let after = p.after.trim();
        if after.is_empty() || after.starts_with('?') || after == p.excerpt.trim() {
            continue;
        }
        if replace(&mut x.text, &p.excerpt, after) {
            log.corrected_passages.push(CorrectedPassage { after: after.to_string(), ..p });
        } else {
            // L'extrait n'est plus dans la réplique (une correction sûre l'a touché) : à vérifier.
            to_check.push(FlaggedPassage { utterance: p.utterance, excerpt: p.excerpt, hypothesis: Some(after.to_string()), relisten: p.relisten });
        }
    }
    r.corrections = (log.corrections.len() + log.corrected_passages.len()) as u32;
    r.passages.clear();
    for p in result.passages_to_check.into_iter().chain(to_check) {
        let Some(x) = r.utterances.get_mut(p.utterance) else { continue };
        // Chaque passage signalé compte, même à plusieurs dans une réplique ; le même deux fois
        // n'en fait qu'un.
        if p.excerpt.trim().is_empty() || x.passages.iter().any(|&i| r.passages[i].excerpt == p.excerpt) {
            continue;
        }
        x.passages.push(r.passages.len());
        r.passages.push(PassageToCheck { utterance: p.utterance, excerpt: p.excerpt, hypothesis: hypothesis(p.hypothesis), relisten: p.relisten, status: PassageStatus::Pending });
    }
    // Les corrections faites à la main sur la relecture précédente sont reprises si leur texte est
    // toujours là.
    let manual = std::mem::take(&mut r.manual_corrections);
    for c in manual {
        if let Some(x) = r.utterances.get_mut(c.utterance)
            && replace(&mut x.text, &c.before, &c.after)
        {
            r.manual_corrections.push(c);
        }
    }
    r.sessions.review = result.session;
    r.review = Some(log);
}

fn for_agent(r: &Meeting, utterances: &[Utterance]) -> Vec<AgentUtterance> {
    utterances
        .iter()
        .enumerate()
        .map(|(number, x)| {
            let l = &r.speakers[r.displayed_speaker(x.speaker)];
            AgentUtterance { number, speaker: l.label.clone(), name: l.name.clone(), start: x.start, end: x.end, text: x.text.clone() }
        })
        .collect()
}
