//! Une réunion et tout ce qui en est tiré : locuteurs, répliques, passages à vérifier, repères,
//! bilan, et son statut dans le traitement. Le cœur seul les modifie ; l'interface les lit.

use crate::adaptateurs::{AjoutAuLexique, ChangementDeLocuteur, CorrectionSure, PassageCorrige};
use crate::dossier::Role;
use crate::temps::DateHeure;

/// L'identifiant d'une réunion dans le cœur, stable tant que l'app tourne.
pub type IdReunion = u32;

/// Un maillon du traitement, dans l'ordre.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Etape {
    Transcription,
    Relecture,
    Bilan,
}

impl Etape {
    pub const TOUTES: [Etape; 3] = [Etape::Transcription, Etape::Relecture, Etape::Bilan];

    pub fn suivante(self) -> Option<Etape> {
        match self {
            Etape::Transcription => Some(Etape::Relecture),
            Etape::Relecture => Some(Etape::Bilan),
            Etape::Bilan => None,
        }
    }

    pub fn precedente(self) -> Option<Etape> {
        match self {
            Etape::Transcription => None,
            Etape::Relecture => Some(Etape::Transcription),
            Etape::Bilan => Some(Etape::Relecture),
        }
    }

    /// Part relative de l'étape dans la durée d'un traitement, pour estimer le temps restant.
    pub fn poids(self) -> f64 {
        match self {
            Etape::Transcription => 6.0,
            Etape::Relecture => 4.5,
            Etape::Bilan => 4.5,
        }
    }
}

/// Où en est une réunion.
#[derive(Clone, Debug, PartialEq)]
pub enum Statut {
    EnregistrementEnCours,
    EnregistrementInterrompu,
    /// Enregistrée (ou importée), le traitement n'est pas lancé.
    ATraiter,
    /// Dans la file, en attente de son tour ; le traitement commencera à cette étape.
    EnAttente(Etape),
    /// L'étape tourne ; `progres` va de 0 à 1.
    EnCours { etape: Etape, progres: f64 },
    Echec { etape: Etape, message: String },
    Terminee,
}

impl Statut {
    pub fn en_traitement(&self) -> bool {
        matches!(self, Statut::EnCours { .. })
    }
    pub fn dans_la_file(&self) -> bool {
        matches!(self, Statut::EnCours { .. } | Statut::EnAttente(_))
    }
    pub fn progres(&self) -> f64 {
        match self {
            Statut::EnCours { progres, .. } => *progres,
            Statut::Terminee => 1.0,
            _ => 0.0,
        }
    }
}

/// Un mot transcrit, horodaté, avec sa confiance (0 à 1).
#[derive(Clone, Debug, PartialEq)]
pub struct Mot {
    pub debut: f64,
    pub fin: f64,
    pub texte: String,
    pub confiance: f32,
    /// L'index de son locuteur dans [`Reunion::locuteurs`], une fois attribué (le transcripteur
    /// rend des mots sans locuteur).
    pub locuteur: Option<usize>,
}

/// Un tour de parole rendu par le séparateur : un seul locuteur à la fois.
#[derive(Clone, Debug, PartialEq)]
pub struct TourDeParole {
    pub debut: f64,
    pub fin: f64,
    /// « SPEAKER_00 »
    pub locuteur: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Locuteur {
    /// « SPEAKER_00 », tel que le séparateur l'a nommé.
    pub etiquette: String,
    /// Le nom confirmé ou donné par l'utilisateur.
    pub nom: Option<String>,
    /// Le nom proposé par l'agent, et pourquoi.
    pub proposition: Option<String>,
    pub justification: String,
    /// Teinte de sa couleur dans l'interface (oklch).
    pub teinte: f32,
    /// Fusionné dans un autre locuteur (index), qui le remplace partout.
    pub fusionne_dans: Option<usize>,
    /// L'agent pense que c'est la même voix qu'un autre locuteur (index).
    pub fusion_proposee: Option<usize>,
}

impl Locuteur {
    pub fn new(etiquette: impl Into<String>, teinte: f32) -> Locuteur {
        Locuteur { etiquette: etiquette.into(), nom: None, proposition: None, justification: String::new(), teinte, fusionne_dans: None, fusion_proposee: None }
    }

    /// Le nom à afficher : confirmé, sinon proposé (« Khalid ? »), sinon l'étiquette.
    pub fn affiche(&self) -> String {
        match (&self.nom, &self.proposition) {
            (Some(n), _) => n.clone(),
            (None, Some(p)) => format!("{p} ?"),
            (None, None) => self.etiquette.clone(),
        }
    }
}

/// Teintes données aux locuteurs détectés, dans l'ordre.
pub const TEINTES: [f32; 8] = [160.0, 255.0, 320.0, 200.0, 35.0, 80.0, 120.0, 280.0];

#[derive(Clone, Debug, PartialEq)]
pub struct Replique {
    /// Index dans `Reunion::locuteurs` (avant fusion : voir `Reunion::locuteur_affiche`).
    pub locuteur: usize,
    pub debut: f64,
    pub fin: f64,
    pub texte: String,
    /// Le passage à vérifier de cette réplique (index dans `Reunion::passages`).
    pub passage: Option<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum StatutDuPassage {
    EnAttente,
    HypotheseAcceptee,
    OriginalGarde,
    CorrigeALaMain(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct PassageAVerifier {
    pub replique: usize,
    pub extrait: String,
    pub hypothese: Option<String>,
    /// Ce que la réécoute a entendu.
    pub reecoute: String,
    pub statut: StatutDuPassage,
}

/// Marge de la réécoute autour de la réplique, et longueur maximale d'un extrait (Whisper entend
/// 30 s d'un coup), en secondes.
const MARGE: f64 = 1.5;
const EXTRAIT_AU_PLUS: f64 = 28.0;

/// L'extrait à réécouter (ou à écouter pour trancher) d'un passage d'une réplique : ±1,5 s autour
/// de la réplique ; au-delà de 28 s, 26 s centrées sur l'endroit du passage dans la réplique.
pub fn extrait_a_reecouter(debut: f64, fin: f64, texte: &str, passage: &str) -> (f64, f64) {
    let (mut de, mut a) = (debut - MARGE, fin + MARGE);
    if a - de > EXTRAIT_AU_PLUS {
        let position = texte.find(passage).map_or(0, |i| texte[..i].chars().count());
        let centre = debut + position as f64 / texte.chars().count().max(1) as f64 * (fin - debut);
        (de, a) = (centre - 12.0, centre + 14.0);
    }
    (de.max(0.0), a)
}

impl PassageAVerifier {
    /// Ce que le passage met dans sa réplique avec ce statut : l'extrait tant qu'il est en attente
    /// ou gardé, sinon l'hypothèse acceptée ou la correction.
    pub fn texte<'a>(&'a self, statut: &'a StatutDuPassage) -> &'a str {
        match statut {
            StatutDuPassage::HypotheseAcceptee => self.hypothese.as_deref().unwrap_or(&self.extrait),
            StatutDuPassage::CorrigeALaMain(t) => t,
            StatutDuPassage::EnAttente | StatutDuPassage::OriginalGarde => &self.extrait,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Repere {
    pub instant: f64,
    pub libelle: String,
}

/// Un sujet du bilan : un chapitre de la timeline.
#[derive(Clone, Debug, PartialEq)]
pub struct Sujet {
    pub debut: f64,
    pub titre: String,
}

/// Une décision horodatée du bilan.
#[derive(Clone, Debug, PartialEq)]
pub struct Decision {
    pub instant: f64,
    pub texte: String,
}

/// Ce que la relecture a changé, gardé dans la transcription structurée.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct JournalDeRelecture {
    /// Les corrections sûres appliquées.
    pub corrections: Vec<CorrectionSure>,
    pub changements_de_locuteur: Vec<ChangementDeLocuteur>,
    /// Les passages douteux corrigés après la réécoute.
    pub passages_corriges: Vec<PassageCorrige>,
    /// Les erreurs écrites dans le lexique.
    pub ajouts_au_lexique: Vec<AjoutAuLexique>,
}

/// Une correction faite à la main dans la transcription : `avant` devient `apres`.
#[derive(Clone, Debug, PartialEq)]
pub struct CorrectionALaMain {
    pub replique: usize,
    pub avant: String,
    pub apres: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sessions {
    pub relecture: Option<String>,
    pub bilan: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Reunion {
    pub id: IdReunion,
    /// Le nom du dossier de la réunion dans le dossier de réunions.
    pub nom_du_dossier: String,
    pub debut: DateHeure,
    pub duree: f64,
    pub titre: String,
    pub statut: Statut,
    /// La dernière étape menée à bien : le traitement reprend à la suivante.
    pub etape_faite: Option<Etape>,
    pub nombre_de_locuteurs: Option<u8>,
    pub reperes: Vec<Repere>,
    pub mots: Vec<Mot>,
    pub tours: Vec<TourDeParole>,
    pub locuteurs: Vec<Locuteur>,
    pub repliques: Vec<Replique>,
    pub passages: Vec<PassageAVerifier>,
    /// Corrections sûres appliquées par la relecture.
    pub corrections: u32,
    /// Les répliques d'avant la relecture (vide tant qu'elle n'est pas faite).
    pub brute: Vec<Replique>,
    /// Le journal de la relecture, quand elle est faite.
    pub relecture: Option<JournalDeRelecture>,
    pub corrections_a_la_main: Vec<CorrectionALaMain>,
    /// Le bilan en Markdown, tel que l'agent l'a écrit (cases cochées comprises).
    pub bilan: String,
    /// Tirés du bilan : chapitres et décisions de la timeline.
    pub sujets: Vec<Sujet>,
    pub decisions: Vec<Decision>,
    pub resume: String,
    pub actions_principales: Vec<String>,
    pub sessions: Sessions,
    /// Le bilan est prêt et la réunion n'a pas encore été ouverte.
    pub nouvelle: bool,
    /// Les fichiers présents dans son dossier, au dernier passage du cœur.
    pub fichiers: Vec<Role>,
}

impl Reunion {
    /// Une réunion vide, à traiter.
    pub fn new(id: IdReunion, nom_du_dossier: String, debut: DateHeure, duree: f64) -> Reunion {
        Reunion {
            id,
            nom_du_dossier,
            debut,
            duree,
            titre: "Réunion".into(),
            statut: Statut::ATraiter,
            etape_faite: None,
            nombre_de_locuteurs: None,
            reperes: vec![],
            mots: vec![],
            tours: vec![],
            locuteurs: vec![],
            repliques: vec![],
            passages: vec![],
            corrections: 0,
            brute: vec![],
            relecture: None,
            corrections_a_la_main: vec![],
            bilan: String::new(),
            sujets: vec![],
            decisions: vec![],
            resume: String::new(),
            actions_principales: vec![],
            sessions: Sessions::default(),
            nouvelle: false,
            fichiers: vec![],
        }
    }

    /// Remplace le bilan et en tire les sujets et les décisions.
    pub(crate) fn poser_bilan(&mut self, md: String) {
        self.sujets = crate::bilan::sujets(&md);
        self.decisions = crate::bilan::decisions(&md);
        self.bilan = md;
    }

    /// Construit une réunion déjà traitée (données d'exemple, fixtures).
    pub fn avec_bilan(mut self, md: impl Into<String>) -> Reunion {
        self.poser_bilan(md.into());
        self
    }

    /// Si `etape` peut être refaite ([`crate::Commande::Refaire`]) : la réunion n'est ni dans la
    /// file ni en enregistrement, et les étapes d'avant sont faites.
    pub fn peut_refaire(&self, etape: Etape) -> bool {
        !self.statut.dans_la_file() && self.statut != Statut::EnregistrementEnCours && etape.precedente().is_none_or(|p| self.etape_faite >= Some(p))
    }

    /// Le début et la fin de l'extrait à écouter pour trancher ce passage : le même que la
    /// réécoute de la relecture.
    pub fn extrait_a_ecouter(&self, passage: usize) -> Option<(f64, f64)> {
        let p = self.passages.get(passage)?;
        let x = self.repliques.get(p.replique)?;
        Some(extrait_a_reecouter(x.debut, x.fin, &x.texte, p.texte(&p.statut)))
    }

    pub fn passages_en_attente(&self) -> usize {
        self.passages.iter().filter(|p| p.statut == StatutDuPassage::EnAttente).count()
    }

    /// Le locuteur affiché pour un index brut, en suivant les fusions.
    pub fn locuteur_affiche(&self, mut i: usize) -> usize {
        while let Some(j) = self.locuteurs[i].fusionne_dans {
            i = j;
        }
        i
    }

    /// Les locuteurs qui n'ont pas été fusionnés dans un autre.
    pub fn locuteurs_actifs(&self) -> impl Iterator<Item = usize> + '_ {
        (0..self.locuteurs.len()).filter(|&i| self.locuteurs[i].fusionne_dans.is_none())
    }

    pub fn replique_a(&self, t: f64) -> Option<usize> {
        let i = self.repliques.partition_point(|u| u.debut <= t);
        if i == 0 { None } else { Some(i - 1) }
    }

    pub fn sujet_a(&self, t: f64) -> Option<usize> {
        let i = self.sujets.partition_point(|x| x.debut <= t);
        if i == 0 { None } else { Some(i - 1) }
    }

    /// Part du temps de parole d'un locuteur (fusions comprises), de 0 à 1.
    pub fn temps_de_parole(&self, locuteur: usize) -> f64 {
        let total: f64 = self.repliques.iter().map(|u| u.fin - u.debut).sum();
        let sien: f64 = self.repliques.iter().filter(|u| self.locuteur_affiche(u.locuteur) == locuteur).map(|u| u.fin - u.debut).sum();
        if total > 0.0 { sien / total } else { 0.0 }
    }

    /// Estimation du temps de traitement restant, en secondes, pour une réunion dans la file.
    /// Environ 90 s par tranche de 10 min de réunion, plus 4 min fixes, réparties selon le poids
    /// des étapes.
    pub fn temps_restant_estime(&self) -> Option<f64> {
        let (depuis, progres) = match &self.statut {
            Statut::EnCours { etape, progres } => (*etape, *progres),
            Statut::EnAttente(etape) => (*etape, 0.0),
            _ => return None,
        };
        let total: f64 = Etape::TOUTES.iter().map(|e| e.poids()).sum();
        let reste: f64 = Etape::TOUTES.iter().filter(|e| **e >= depuis).map(|e| e.poids()).sum::<f64>() - progres * depuis.poids();
        Some(reste / total * (self.duree / 600.0 * 90.0 + 240.0))
    }
}
