//! Les vrais moteurs de Greffier, branchés sur les adaptateurs du cœur.
//!
//! - [`TranscripteurWhisper`] : la piste de mixage extraite par AVFoundation ([`audio`]), puis
//!   transcrite par whisper.cpp ([`whisper`]), compilé dans l'app depuis ses sources (build.rs),
//!   avec les paramètres du vault.
//! - [`SeparateurPyannote`] : la même piste de mixage, séparée en tours de parole par pyannote
//!   community-1 dans le Python privé de l'app ([`pyannote`]), pendant que whisper transcrit.
//!
//! Ces moteurs ne sont pas testés par la suite automatique (le cœur l'est, avec de faux
//! adaptateurs) ; ils se vérifient par le banc de parité et par
//! `cargo run -p greffier-moteurs --example transcrire -- <média>` (et `--example separer`).

pub mod audio;
pub mod pyannote;
pub mod reechantillonnage;
pub mod whisper;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use greffier_coeur::Mot;
pub use pyannote::SeparateurPyannote;
use greffier_coeur::adaptateurs::{Avancement, DemandeDeTranscription, Tache, TacheDe, Transcripteur};

/// Part de l'avancement donnée à l'extraction de l'audio (le reste va à whisper).
const PART_DE_L_AUDIO: f64 = 0.05;

/// whisper.cpp large-v3 sur Metal, VAD Silero, avec les modèles du dossier de l'app.
pub struct TranscripteurWhisper {
    pub modele: PathBuf,
    pub vad: PathBuf,
}

impl TranscripteurWhisper {
    /// Les modèles installés dans le dossier de l'app.
    pub fn de_l_app(dossier_de_l_app: &std::path::Path) -> TranscripteurWhisper {
        use greffier_coeur::ressources::{MODELE_VAD, MODELE_WHISPER};
        TranscripteurWhisper { modele: dossier_de_l_app.join(MODELE_WHISPER), vad: dossier_de_l_app.join(MODELE_VAD) }
    }

    /// Transcrit sur le fil appelant : l'audio, puis whisper.
    pub fn transcrire_ici(&self, demande: &DemandeDeTranscription, arret: &AtomicBool, progres: &mut dyn FnMut(f64)) -> Result<Vec<Mot>, String> {
        let echantillons = audio::mixage(&demande.audio, arret, |p| progres(p * PART_DE_L_AUDIO))?;
        if echantillons.len() < audio::FREQUENCE as usize {
            return Err("Le média n'a pas de son.".into());
        }
        whisper::transcrire(&self.modele, &self.vad, &echantillons, &demande.vocabulaire, arret, &mut |p| progres(PART_DE_L_AUDIO + p * (1.0 - PART_DE_L_AUDIO)))
    }
}

#[derive(Default)]
struct Etat {
    progres: f64,
    resultat: Option<Result<Vec<Mot>, String>>,
}

struct Transcription {
    etat: Arc<Mutex<Etat>>,
    arret: Arc<AtomicBool>,
}

impl Tache<Vec<Mot>> for Transcription {
    fn sonder(&mut self, _: f64) -> Avancement<Vec<Mot>> {
        let mut e = self.etat.lock().unwrap_or_else(|e| e.into_inner());
        match e.resultat.take() {
            Some(Ok(mots)) => Avancement::Fini(mots),
            Some(Err(m)) => Avancement::Echec(m),
            None => Avancement::EnCours(e.progres),
        }
    }

    fn arreter(&mut self) {
        self.arret.store(true, Ordering::Relaxed);
    }
}

impl Transcripteur for TranscripteurWhisper {
    fn transcrire(&mut self, demande: DemandeDeTranscription) -> TacheDe<Vec<Mot>> {
        let etat = Arc::new(Mutex::new(Etat::default()));
        let arret = Arc::new(AtomicBool::new(false));
        let moteur = TranscripteurWhisper { modele: self.modele.clone(), vad: self.vad.clone() };
        let (e, a) = (etat.clone(), arret.clone());
        let lance = std::thread::Builder::new().name("whisper".into()).spawn(move || {
            let mut progres = |p: f64| e.lock().unwrap_or_else(|e| e.into_inner()).progres = p.clamp(0.0, 1.0);
            let r = moteur.transcrire_ici(&demande, &a, &mut progres);
            e.lock().unwrap_or_else(|e| e.into_inner()).resultat = Some(r);
        });
        if let Err(err) = lance {
            etat.lock().unwrap_or_else(|e| e.into_inner()).resultat = Some(Err(format!("Impossible de lancer la transcription : {err}")));
        }
        Box::new(Transcription { etat, arret })
    }
}

/// La réécoute des passages douteux de la relecture : l'enregistrement est décodé une fois, puis
/// chaque extrait est retranscrit par whisper.cpp avec les réglages de la transcription (mêmes
/// modèles, VAD, vocabulaire en prompt).
pub struct ReecouteWhisper {
    pub modele: PathBuf,
    pub vad: PathBuf,
}

impl ReecouteWhisper {
    pub fn de_l_app(dossier_de_l_app: &std::path::Path) -> ReecouteWhisper {
        let t = TranscripteurWhisper::de_l_app(dossier_de_l_app);
        ReecouteWhisper { modele: t.modele, vad: t.vad }
    }
}

impl greffier_coeur::claude::relecture::Reecoute for ReecouteWhisper {
    fn reecouter(&self, enregistrement: &std::path::Path, extraits: &[(f64, f64)], vocabulaire: &str, arret: &AtomicBool) -> Result<Vec<String>, String> {
        let echantillons = audio::mixage(enregistrement, arret, |_| {})?;
        let f = audio::FREQUENCE as f64;
        let mut textes = Vec::with_capacity(extraits.len());
        for &(de, a) in extraits {
            let (i, j) = (((de.max(0.0) * f) as usize).min(echantillons.len()), ((a * f) as usize).min(echantillons.len()));
            if j <= i {
                textes.push(String::new());
                continue;
            }
            match whisper::transcrire(&self.modele, &self.vad, &echantillons[i..j], vocabulaire, arret, &mut |_| {}) {
                Ok(mots) => textes.push(mots.iter().map(|m| m.texte.trim()).filter(|t| !t.is_empty()).collect::<Vec<_>>().join(" ")),
                Err(e) if arret.load(Ordering::Relaxed) => return Err(e),
                // Un extrait raté n'empêche pas les autres : l'agent tranchera sans lui.
                Err(_) => textes.push(String::new()),
            }
        }
        Ok(textes)
    }
}
