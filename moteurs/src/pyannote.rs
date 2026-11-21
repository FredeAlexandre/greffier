//! La séparation des locuteurs par pyannote community-1, dans le Python privé de l'app
//! (ADR 0003) : la piste de mixage extraite par AVFoundation ([`crate::audio`]) est écrite en WAV
//! dans le dossier temporaire du Python privé, puis `separer.py` (embarqué dans l'app) la sépare
//! hors ligne, sur MPS avec repli sur le CPU, en tours de parole exclusifs.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use greffier_coeur::TourDeParole;
use greffier_coeur::adaptateurs::{Avancement, DemandeDeSeparation, Separateur, Tache, TacheDe};
use greffier_outils::mac;
use greffier_outils::processus::{Interruption, attendre};
use greffier_coeur::ressources::MODELE_PYANNOTE;

use crate::audio;

/// Le script lancé dans le Python privé.
pub const SCRIPT: &str = include_str!("separer.py");
/// Part de l'avancement donnée à l'extraction de l'audio (le reste va à pyannote).
const PART_DE_L_AUDIO: f64 = 0.05;

/// pyannote community-1 dans le Python privé de l'app.
#[derive(Clone)]
pub struct SeparateurPyannote {
    /// Le dossier du Python privé (`<app>/python`).
    pub python: PathBuf,
    /// Le dossier du modèle (`<app>/modeles/pyannote-community-1`).
    pub modele: PathBuf,
}

impl SeparateurPyannote {
    /// Le Python privé et le modèle installés dans le dossier de l'app.
    pub fn de_l_app(dossier_de_l_app: &Path) -> SeparateurPyannote {
        SeparateurPyannote { python: dossier_de_l_app.join("python"), modele: dossier_de_l_app.join(MODELE_PYANNOTE) }
    }

    /// Sépare sur le fil appelant : l'audio, puis pyannote.
    pub fn separer_ici(&self, demande: &DemandeDeSeparation, arret: &AtomicBool, progres: &mut dyn FnMut(f64)) -> Result<Vec<TourDeParole>, String> {
        let echantillons = audio::mixage(&demande.audio, arret, |p| progres(p * PART_DE_L_AUDIO))?;
        if echantillons.len() < audio::FREQUENCE as usize {
            return Err("Le média n'a pas de son.".into());
        }
        let temporaire = self.python.join("tmp");
        std::fs::create_dir_all(&temporaire).map_err(|e| format!("{} : {e}", temporaire.display()))?;
        let base = temporaire.join(format!("separation-{}-{}", std::process::id(), numero()));
        let (wav, json) = (base.with_extension("wav"), base.with_extension("json"));
        let resultat = (|| {
            audio::ecrire_wav(&wav, &echantillons).map_err(|e| format!("{} : {e}", wav.display()))?;
            drop(echantillons);
            self.lancer(&wav, &json, demande.nombre_de_locuteurs, &temporaire, arret, &mut |p| progres(PART_DE_L_AUDIO + p * (1.0 - PART_DE_L_AUDIO)))?;
            let lu = std::fs::read_to_string(&json).map_err(|e| format!("pyannote n'a rien écrit ({e})."))?;
            tours(&lu)
        })();
        let _ = std::fs::remove_file(&wav);
        let _ = std::fs::remove_file(&json);
        resultat
    }

    fn lancer(&self, wav: &Path, json: &Path, locuteurs: Option<u8>, dossier: &Path, arret: &AtomicBool, progres: &mut dyn FnMut(f64)) -> Result<(), String> {
        let python = mac::python(&self.python);
        if !python.exists() {
            return Err(format!("Le Python privé n'est pas installé ({}).", python.display()));
        }
        let mut commande = Command::new(&python);
        commande.arg("-I").arg("-c").arg(SCRIPT).arg(wav).arg(json).arg(&self.modele);
        if let Some(n) = locuteurs {
            commande.arg("--locuteurs").arg(n.to_string());
        }
        let mut enfant = commande
            .env_clear()
            .envs(mac::environnement_python(&self.python))
            .current_dir(dossier)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Impossible de lancer pyannote ({}) : {e}", python.display()))?;

        // La sortie d'erreur est lue au fil de l'eau (sinon le tuyau se remplit et bloque
        // pyannote) : l'avancement, et les dernières lignes pour le message d'erreur.
        let avance = Arc::new(AtomicU64::new(0f64.to_bits()));
        let dernieres = Arc::new(Mutex::new(Vec::<String>::new()));
        let erreur = enfant.stderr.take().expect("sortie d'erreur");
        let (a, d) = (avance.clone(), dernieres.clone());
        let lecteur = std::thread::spawn(move || {
            for ligne in BufReader::new(erreur).lines().map_while(Result::ok) {
                if let Some(p) = ligne.strip_prefix("PROGRES ").and_then(|p| p.trim().parse::<f64>().ok()) {
                    a.store(p.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
                } else if !ligne.trim().is_empty() {
                    let mut d = d.lock().unwrap_or_else(|e| e.into_inner());
                    d.push(ligne);
                    let trop = d.len().saturating_sub(5);
                    d.drain(..trop);
                }
            }
        });

        let statut = match attendre(&mut enfant, arret, None, Duration::from_millis(100), || progres(f64::from_bits(avance.load(Ordering::Relaxed)))) {
            Ok(s) => s,
            Err(Interruption::Erreur(e)) => return Err(format!("pyannote : {e}")),
            Err(Interruption::Arrete | Interruption::Delai) => {
                let _ = lecteur.join();
                return Err("arrêté".into());
            }
        };
        let _ = lecteur.join();
        if !statut.success() {
            let d = dernieres.lock().unwrap_or_else(|e| e.into_inner());
            return Err(format!("pyannote a échoué ({statut}) : {}", d.join(" · ")));
        }
        progres(1.0);
        Ok(())
    }
}

/// Un numéro par séparation, pour que deux séparations n'écrivent pas dans le même fichier.
fn numero() -> u64 {
    static N: AtomicU64 = AtomicU64::new(0);
    N.fetch_add(1, Ordering::Relaxed)
}

/// Les tours de parole du JSON de `separer.py`, étiquetés SPEAKER_00, SPEAKER_01… dans l'ordre
/// de pyannote (le cœur les renumérote dans l'ordre du premier mot).
pub fn tours(json: &str) -> Result<Vec<TourDeParole>, String> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|e| format!("Sortie de pyannote illisible : {e}"))?;
    let segments = v["segments"].as_array().ok_or("Sortie de pyannote sans segments.")?;
    segments
        .iter()
        .map(|s| match (s["start"].as_f64(), s["end"].as_f64(), s["speaker"].as_u64()) {
            (Some(debut), Some(fin), Some(n)) => Ok(TourDeParole { debut, fin, locuteur: greffier_coeur::Locuteur::etiquette_numero(n as usize) }),
            _ => Err(format!("Tour de parole illisible : {s}")),
        })
        .collect()
}

#[derive(Default)]
struct Etat {
    progres: f64,
    resultat: Option<Result<Vec<TourDeParole>, String>>,
}

struct Separation {
    etat: Arc<Mutex<Etat>>,
    arret: Arc<AtomicBool>,
}

impl Tache<Vec<TourDeParole>> for Separation {
    fn sonder(&mut self, _: f64) -> Avancement<Vec<TourDeParole>> {
        let mut e = self.etat.lock().unwrap_or_else(|e| e.into_inner());
        match e.resultat.take() {
            Some(Ok(tours)) => Avancement::Fini(tours),
            Some(Err(m)) => Avancement::Echec(m),
            None => Avancement::EnCours(e.progres),
        }
    }

    fn arreter(&mut self) {
        self.arret.store(true, Ordering::Relaxed);
    }
}

impl Separateur for SeparateurPyannote {
    fn separer(&mut self, demande: DemandeDeSeparation) -> TacheDe<Vec<TourDeParole>> {
        let etat = Arc::new(Mutex::new(Etat::default()));
        let arret = Arc::new(AtomicBool::new(false));
        let moteur = self.clone();
        let (e, a) = (etat.clone(), arret.clone());
        let lance = std::thread::Builder::new().name("pyannote".into()).spawn(move || {
            let mut progres = |p: f64| e.lock().unwrap_or_else(|e| e.into_inner()).progres = p.clamp(0.0, 1.0);
            let r = moteur.separer_ici(&demande, &a, &mut progres);
            e.lock().unwrap_or_else(|e| e.into_inner()).resultat = Some(r);
        });
        if let Err(err) = lance {
            etat.lock().unwrap_or_else(|e| e.into_inner()).resultat = Some(Err(format!("Impossible de lancer la séparation des locuteurs : {err}")));
        }
        Box::new(Separation { etat, arret })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn les_tours_de_pyannote_sont_lus_et_etiquetes() {
        let json = r#"{"segments": [{"start": 0.5, "end": 2.25, "speaker": 0}, {"start": 2.25, "end": 3.0, "speaker": 11}], "speakers": 2, "device": "mps", "seconds": 1.2}"#;
        let t = tours(json).expect("lus");
        assert_eq!(t, [TourDeParole { debut: 0.5, fin: 2.25, locuteur: "SPEAKER_00".into() }, TourDeParole { debut: 2.25, fin: 3.0, locuteur: "SPEAKER_11".into() }]);
        assert!(tours("{}").is_err());
    }
}
