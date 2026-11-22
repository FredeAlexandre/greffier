//! Une réunion et tout ce qui en est tiré : locuteurs, répliques, passages à vérifier, repères,
//! bilan, et son statut dans le traitement. Le cœur seul les modifie ; l'interface les lit.

use crate::adapters::{LexiconAddition, SpeakerChange, ConfidentCorrection, CorrectedPassage};
use crate::folder::Role;
use crate::time::DateTime;

/// L'identifiant d'une réunion dans le cœur, stable tant que l'app tourne.
pub type MeetingId = u32;

/// Un maillon du traitement, dans l'ordre.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Stage {
    Transcription,
    Review,
    Summary,
}

impl Stage {
    pub const ALL: [Stage; 3] = [Stage::Transcription, Stage::Review, Stage::Summary];

    pub fn next(self) -> Option<Stage> {
        match self {
            Stage::Transcription => Some(Stage::Review),
            Stage::Review => Some(Stage::Summary),
            Stage::Summary => None,
        }
    }

    pub fn previous(self) -> Option<Stage> {
        match self {
            Stage::Transcription => None,
            Stage::Review => Some(Stage::Transcription),
            Stage::Summary => Some(Stage::Review),
        }
    }

    /// Part relative de l'étape dans la durée d'un traitement, pour estimer le temps restant.
    pub fn weight(self) -> f64 {
        match self {
            Stage::Transcription => 6.0,
            Stage::Review => 4.5,
            Stage::Summary => 4.5,
        }
    }
}

/// Où en est une réunion.
#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    Recording,
    RecordingInterrupted,
    /// Enregistrée (ou importée), le traitement n'est pas lancé.
    ToProcess,
    /// Dans la file, en attente de son tour ; le traitement commencera à cette étape.
    Pending(Stage),
    /// L'étape tourne ; `progres` va de 0 à 1.
    Running { stage: Stage, progress: f64 },
    Failed { stage: Stage, message: String },
    Done,
}

impl Status {
    pub fn is_processing(&self) -> bool {
        matches!(self, Status::Running { .. })
    }
    pub fn in_queue(&self) -> bool {
        matches!(self, Status::Running { .. } | Status::Pending(_))
    }
    pub fn progress(&self) -> f64 {
        match self {
            Status::Running { progress, .. } => *progress,
            Status::Done => 1.0,
            _ => 0.0,
        }
    }
}

/// Un mot transcrit, horodaté, avec sa confiance (0 à 1).
#[derive(Clone, Debug, PartialEq)]
pub struct Word {
    pub start: f64,
    pub end: f64,
    pub text: String,
    pub confidence: f32,
    /// L'index de son locuteur dans [`Reunion::locuteurs`], une fois attribué (le transcripteur
    /// rend des mots sans locuteur).
    pub speaker: Option<usize>,
}

/// Un tour de parole rendu par le séparateur : un seul locuteur à la fois.
#[derive(Clone, Debug, PartialEq)]
pub struct SpeakerTurn {
    pub start: f64,
    pub end: f64,
    /// « SPEAKER_00 »
    pub speaker: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Speaker {
    /// « SPEAKER_00 », tel que le séparateur l'a nommé.
    pub label: String,
    /// Le nom confirmé ou donné par l'utilisateur.
    pub name: Option<String>,
    /// Le nom proposé par l'agent, et pourquoi.
    pub proposal: Option<String>,
    pub justification: String,
    /// Teinte de sa couleur dans l'interface (oklch).
    pub tint: f32,
    /// Fusionné dans un autre locuteur (index), qui le remplace partout.
    pub merged_into: Option<usize>,
    /// L'agent pense que c'est la même voix qu'un autre locuteur (index).
    pub suggested_merge: Option<usize>,
}

impl Speaker {
    /// L'étiquette du n-ième locuteur : « SPEAKER_00 », « SPEAKER_01 »…
    pub fn numbered_label(n: usize) -> String {
        format!("SPEAKER_{n:02}")
    }

    pub fn new(label: impl Into<String>, tint: f32) -> Speaker {
        Speaker { label: label.into(), name: None, proposal: None, justification: String::new(), tint, merged_into: None, suggested_merge: None }
    }

    /// Le nom à afficher : confirmé, sinon proposé (« Khalid ? »), sinon l'étiquette.
    pub fn display_name(&self) -> String {
        match (&self.name, &self.proposal) {
            (Some(n), _) => n.clone(),
            (None, Some(p)) => format!("{p} ?"),
            (None, None) => self.label.clone(),
        }
    }
}

/// Teintes données aux locuteurs détectés, dans l'ordre.
pub const HUES: [f32; 8] = [160.0, 255.0, 320.0, 200.0, 35.0, 80.0, 120.0, 280.0];

#[derive(Clone, Debug, PartialEq)]
pub struct Utterance {
    /// Index dans `Reunion::locuteurs` (avant fusion : voir `Reunion::locuteur_affiche`).
    pub speaker: usize,
    pub start: f64,
    pub end: f64,
    pub text: String,
    /// Les passages à vérifier de cette réplique (index dans `Reunion::passages`, dans l'ordre).
    pub passages: Vec<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PassageStatus {
    Pending,
    HypothesisAccepted,
    OriginalKept,
    CorrectedManually(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct PassageToCheck {
    pub utterance: usize,
    pub excerpt: String,
    pub hypothesis: Option<String>,
    /// Ce que la réécoute a entendu.
    pub relisten: String,
    pub status: PassageStatus,
}

/// Marge de la réécoute autour de la réplique, et longueur maximale d'un extrait (Whisper entend
/// 30 s d'un coup), en secondes.
const MARGIN: f64 = 1.5;
const MAX_EXCERPT: f64 = 28.0;

/// L'extrait à réécouter (ou à écouter pour trancher) d'un passage d'une réplique : ±1,5 s autour
/// de la réplique ; au-delà de 28 s, 26 s centrées sur l'endroit du passage dans la réplique.
pub fn excerpt_to_relisten(start: f64, end: f64, text: &str, passage: &str) -> (f64, f64) {
    let (mut from, mut a) = (start - MARGIN, end + MARGIN);
    if a - from > MAX_EXCERPT {
        let position = text.find(passage).map_or(0, |i| text[..i].chars().count());
        let center = start + position as f64 / text.chars().count().max(1) as f64 * (end - start);
        (from, a) = (center - 12.0, center + 14.0);
    }
    (from.max(0.0), a)
}

impl PassageToCheck {
    /// Ce que le passage met dans sa réplique avec ce statut : l'extrait tant qu'il est en attente
    /// ou gardé, sinon l'hypothèse acceptée ou la correction.
    pub fn text<'a>(&'a self, status: &'a PassageStatus) -> &'a str {
        match status {
            PassageStatus::HypothesisAccepted => self.hypothesis.as_deref().unwrap_or(&self.excerpt),
            PassageStatus::CorrectedManually(t) => t,
            PassageStatus::Pending | PassageStatus::OriginalKept => &self.excerpt,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Marker {
    pub instant: f64,
    pub label: String,
}

/// Un sujet du bilan : un chapitre de la timeline.
#[derive(Clone, Debug, PartialEq)]
pub struct Topic {
    pub start: f64,
    pub title: String,
}

/// Une décision horodatée du bilan.
#[derive(Clone, Debug, PartialEq)]
pub struct Decision {
    pub instant: f64,
    pub text: String,
}

/// Ce que la relecture a changé, gardé dans la transcription structurée.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReviewLog {
    /// Les corrections sûres appliquées.
    pub corrections: Vec<ConfidentCorrection>,
    pub speaker_changes: Vec<SpeakerChange>,
    /// Les passages douteux corrigés après la réécoute.
    pub corrected_passages: Vec<CorrectedPassage>,
    /// Les erreurs écrites dans le lexique.
    pub lexicon_additions: Vec<LexiconAddition>,
}

/// Une correction faite à la main dans la transcription : `avant` devient `apres`.
#[derive(Clone, Debug, PartialEq)]
pub struct ManualCorrection {
    pub utterance: usize,
    pub before: String,
    pub after: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sessions {
    pub review: Option<String>,
    pub summary: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Meeting {
    pub id: MeetingId,
    /// Le nom du dossier de la réunion dans le dossier de réunions.
    pub folder_name: String,
    pub start: DateTime,
    pub duration: f64,
    pub title: String,
    pub status: Status,
    /// La dernière étape menée à bien : le traitement reprend à la suivante.
    pub completed_stage: Option<Stage>,
    pub speaker_count: Option<u8>,
    pub markers: Vec<Marker>,
    pub words: Vec<Word>,
    pub turns: Vec<SpeakerTurn>,
    pub speakers: Vec<Speaker>,
    pub utterances: Vec<Utterance>,
    pub passages: Vec<PassageToCheck>,
    /// Corrections sûres appliquées par la relecture.
    pub corrections: u32,
    /// Les répliques d'avant la relecture (vide tant qu'elle n'est pas faite).
    pub raw: Vec<Utterance>,
    /// Le journal de la relecture, quand elle est faite.
    pub review: Option<ReviewLog>,
    pub manual_corrections: Vec<ManualCorrection>,
    /// Le bilan en Markdown, tel que l'agent l'a écrit (cases cochées comprises).
    pub summary: String,
    /// Tirés du bilan : chapitres et décisions de la timeline.
    pub topics: Vec<Topic>,
    pub decisions: Vec<Decision>,
    pub brief: String,
    pub key_actions: Vec<String>,
    pub sessions: Sessions,
    /// Le bilan est prêt et la réunion n'a pas encore été ouverte.
    pub is_new: bool,
    /// Les fichiers présents dans son dossier, au dernier passage du cœur.
    pub files: Vec<Role>,
}

impl Meeting {
    /// Une réunion vide, à traiter.
    pub fn new(id: MeetingId, folder_name: String, start: DateTime, duration: f64) -> Meeting {
        Meeting {
            id,
            folder_name,
            start,
            duration,
            title: "Réunion".into(),
            status: Status::ToProcess,
            completed_stage: None,
            speaker_count: None,
            markers: vec![],
            words: vec![],
            turns: vec![],
            speakers: vec![],
            utterances: vec![],
            passages: vec![],
            corrections: 0,
            raw: vec![],
            review: None,
            manual_corrections: vec![],
            summary: String::new(),
            topics: vec![],
            decisions: vec![],
            brief: String::new(),
            key_actions: vec![],
            sessions: Sessions::default(),
            is_new: false,
            files: vec![],
        }
    }

    /// Remplace le bilan et en tire les sujets et les décisions.
    pub(crate) fn set_summary(&mut self, md: String) {
        self.topics = crate::summary::topics(&md);
        self.decisions = crate::summary::decisions(&md);
        self.summary = md;
    }

    /// Construit une réunion déjà traitée (données d'exemple, fixtures).
    pub fn with_summary(mut self, md: impl Into<String>) -> Meeting {
        self.set_summary(md.into());
        self
    }

    /// Si `etape` peut être refaite ([`crate::Commande::Refaire`]) : la réunion n'est ni dans la
    /// file ni en enregistrement, et les étapes d'avant sont faites.
    pub fn can_redo(&self, stage: Stage) -> bool {
        !self.status.in_queue() && self.status != Status::Recording && stage.previous().is_none_or(|p| self.completed_stage >= Some(p))
    }

    /// Le début et la fin de l'extrait à écouter pour trancher ce passage : le même que la
    /// réécoute de la relecture.
    pub fn excerpt_to_listen(&self, passage: usize) -> Option<(f64, f64)> {
        let p = self.passages.get(passage)?;
        let x = self.utterances.get(p.utterance)?;
        Some(excerpt_to_relisten(x.start, x.end, &x.text, p.text(&p.status)))
    }

    pub fn pending_passages(&self) -> usize {
        self.passages.iter().filter(|p| p.status == PassageStatus::Pending).count()
    }

    /// Le locuteur affiché pour un index brut, en suivant les fusions.
    pub fn displayed_speaker(&self, mut i: usize) -> usize {
        while let Some(j) = self.speakers[i].merged_into {
            i = j;
        }
        i
    }

    /// Le nom donné au locuteur affiché pour un index brut (fusions suivies).
    pub fn speaker_name(&self, i: usize) -> Option<&str> {
        self.speakers[self.displayed_speaker(i)].name.as_deref()
    }

    /// Après avoir relu noms et fusions : un locuteur fusionné n'a pas de nom à lui (les fichiers
    /// lui donnent celui de la voix qui le remplace).
    pub(crate) fn forget_merged_names(&mut self) {
        for l in &mut self.speakers {
            if l.merged_into.is_some() {
                l.name = None;
            }
        }
    }

    /// Fusionne le locuteur `i` dans `j` : celui qu'affiche `i` est désormais remplacé partout par
    /// celui qu'affiche `j` (les fusions déjà faites suivent). Rend `false`, sans rien changer, si
    /// c'est déjà la même voix ou si un index est inconnu : une fusion ne fait jamais de boucle.
    pub fn merge(&mut self, i: usize, j: usize) -> bool {
        if i >= self.speakers.len() || j >= self.speakers.len() {
            return false;
        }
        let (i, j) = (self.displayed_speaker(i), self.displayed_speaker(j));
        if i == j {
            return false;
        }
        self.speakers[i].merged_into = Some(j);
        true
    }

    /// Relit une fusion écrite (fiche, transcription structurée) telle quelle : `i` pointe sur `j`,
    /// sauf si `i` est déjà fusionné ou si cela ferait une boucle.
    pub(crate) fn restore_merge(&mut self, i: usize, j: usize) {
        if i < self.speakers.len() && j < self.speakers.len() && self.speakers[i].merged_into.is_none() && self.displayed_speaker(j) != i {
            self.speakers[i].merged_into = Some(j);
        }
    }

    /// Les locuteurs qui n'ont pas été fusionnés dans un autre.
    pub fn active_speakers(&self) -> impl Iterator<Item = usize> + '_ {
        (0..self.speakers.len()).filter(|&i| self.speakers[i].merged_into.is_none())
    }

    pub fn utterance_at(&self, t: f64) -> Option<usize> {
        let i = self.utterances.partition_point(|u| u.start <= t);
        if i == 0 { None } else { Some(i - 1) }
    }

    pub fn topic_at(&self, t: f64) -> Option<usize> {
        let i = self.topics.partition_point(|x| x.start <= t);
        if i == 0 { None } else { Some(i - 1) }
    }

    /// Part du temps de parole d'un locuteur (fusions comprises), de 0 à 1.
    pub fn speaking_time(&self, speaker: usize) -> f64 {
        let total: f64 = self.utterances.iter().map(|u| u.end - u.start).sum();
        let own: f64 = self.utterances.iter().filter(|u| self.displayed_speaker(u.speaker) == speaker).map(|u| u.end - u.start).sum();
        if total > 0.0 { own / total } else { 0.0 }
    }

    /// Estimation du temps de traitement restant, en secondes, pour une réunion dans la file.
    /// Environ 90 s par tranche de 10 min de réunion, plus 4 min fixes, réparties selon le poids
    /// des étapes.
    pub fn estimated_time_remaining(&self) -> Option<f64> {
        let (from_meeting, progress) = match &self.status {
            Status::Running { stage, progress } => (*stage, *progress),
            Status::Pending(stage) => (*stage, 0.0),
            _ => return None,
        };
        let total: f64 = Stage::ALL.iter().map(|e| e.weight()).sum();
        let rest: f64 = Stage::ALL.iter().filter(|e| **e >= from_meeting).map(|e| e.weight()).sum::<f64>() - progress * from_meeting.weight();
        Some(rest / total * (self.duration / 600.0 * 90.0 + 240.0))
    }
}
