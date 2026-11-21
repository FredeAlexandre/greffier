//! Ce que les vrais adaptateurs partagent pour un travail long : le faire tourner sur un fil
//! ([`sur_un_fil`], une [`Tache`] que le cœur sonde) et attendre un programme lancé qu'on peut
//! arrêter ([`attendre`]).

use std::process::{Child, ExitStatus};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use greffier_coeur::adaptateurs::{Avancement, Tache, TacheDe};

/// Comment l'avancement d'un travail sur un fil se lit.
#[derive(Clone, Copy, Debug)]
pub enum Progression {
    /// Le travail donne lui-même son avancement, de 0 à 1.
    Exacte,
    /// Le travail annonce des paliers (Claude ne dit pas où il en est) : l'avancement glisse
    /// lentement vers le palier suivant (1 après le dernier) en attendant.
    Paliers(&'static [f64]),
}

/// Constante de temps du glissement vers le palier suivant, en secondes.
const GLISSEMENT: f64 = 120.0;

struct Etat<T> {
    /// L'avancement annoncé, et quand.
    avance: f64,
    depuis: Instant,
    resultat: Option<Result<T, String>>,
}

/// Un travail sur un fil ; `sonder` lit son état.
struct SurUnFil<T> {
    etat: Arc<Mutex<Etat<T>>>,
    arret: Arc<AtomicBool>,
    progression: Progression,
}

/// Lance `travail` sur un fil nommé `nom`. Il reçoit le drapeau d'arrêt (levé par `arreter()` ou
/// quand la tâche est lâchée) et de quoi annoncer son avancement. Un travail qui panique finit en
/// échec.
pub fn sur_un_fil<T: Send + 'static>(nom: &str, progression: Progression, travail: impl FnOnce(&AtomicBool, &mut dyn FnMut(f64)) -> Result<T, String> + Send + 'static) -> TacheDe<T> {
    let etat = Arc::new(Mutex::new(Etat { avance: 0.0, depuis: Instant::now(), resultat: None }));
    let arret = Arc::new(AtomicBool::new(false));
    let (e, a) = (etat.clone(), arret.clone());
    let lance = std::thread::Builder::new().name(nom.into()).spawn(move || {
        let mut progres = |p: f64| {
            let mut e = e.lock().unwrap_or_else(|e| e.into_inner());
            e.avance = p;
            e.depuis = Instant::now();
        };
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| travail(&a, &mut progres))).unwrap_or_else(|_| Err("le travail s'est arrêté sans résultat".into()));
        e.lock().unwrap_or_else(|e| e.into_inner()).resultat = Some(r);
    });
    if let Err(err) = lance {
        etat.lock().unwrap_or_else(|e| e.into_inner()).resultat = Some(Err(format!("Impossible de lancer {nom} : {err}")));
    }
    Box::new(SurUnFil { etat, arret, progression })
}

impl<T> Tache<T> for SurUnFil<T> {
    fn sonder(&mut self, _: f64) -> Avancement<T> {
        let mut e = self.etat.lock().unwrap_or_else(|e| e.into_inner());
        match e.resultat.take() {
            Some(Ok(v)) => Avancement::Fini(v),
            Some(Err(m)) => Avancement::Echec(m),
            None => match self.progression {
                Progression::Exacte => Avancement::EnCours(e.avance),
                Progression::Paliers(paliers) => {
                    let suivant = paliers.iter().copied().find(|p| *p > e.avance).unwrap_or(1.0);
                    let t = e.depuis.elapsed().as_secs_f64();
                    Avancement::EnCours(e.avance + (suivant - e.avance) * 0.9 * (1.0 - (-t / GLISSEMENT).exp()))
                }
            },
        }
    }

    fn arreter(&mut self) {
        self.arret.store(true, Ordering::Relaxed);
    }
}

impl<T> Drop for SurUnFil<T> {
    fn drop(&mut self) {
        self.arret.store(true, Ordering::Relaxed);
    }
}

/// Pourquoi un programme attendu n'est pas allé jusqu'au bout.
#[derive(Debug)]
pub enum Interruption {
    /// Le drapeau d'arrêt s'est levé : le programme a été tué.
    Arrete,
    /// Le délai est dépassé : le programme a été tué.
    Delai,
    /// Son état n'a pas pu être lu.
    Erreur(std::io::Error),
}

/// Attend la fin de `enfant` en regardant toutes les `pas` ; le tue si `arret` se lève ou si
/// `delai` est dépassé. `pendant` est appelé à chaque tour (pour relire un avancement).
pub fn attendre(enfant: &mut Child, arret: &AtomicBool, delai: Option<Duration>, pas: Duration, mut pendant: impl FnMut()) -> Result<ExitStatus, Interruption> {
    let debut = Instant::now();
    loop {
        let interruption = if arret.load(Ordering::Relaxed) {
            Some(Interruption::Arrete)
        } else if delai.is_some_and(|d| debut.elapsed() > d) {
            Some(Interruption::Delai)
        } else {
            None
        };
        if let Some(i) = interruption {
            let _ = enfant.kill();
            let _ = enfant.wait();
            return Err(i);
        }
        match enfant.try_wait() {
            Ok(Some(s)) => return Ok(s),
            Ok(None) => {}
            Err(e) => return Err(Interruption::Erreur(e)),
        }
        pendant();
        std::thread::sleep(pas);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Command, Stdio};

    fn sonder_jusqu_au_bout<T>(t: &mut TacheDe<T>) -> Avancement<T> {
        for _ in 0..500 {
            match t.sonder(0.0) {
                Avancement::EnCours(_) => std::thread::sleep(Duration::from_millis(10)),
                fini => return fini,
            }
        }
        panic!("la tâche ne finit pas");
    }

    #[test]
    fn un_travail_sur_un_fil_rend_son_resultat_son_echec_ou_l_echec_d_une_panique() {
        assert!(matches!(sonder_jusqu_au_bout(&mut sur_un_fil("essai", Progression::Exacte, |_, _| Ok(3))), Avancement::Fini(3)));
        assert!(matches!(sonder_jusqu_au_bout(&mut sur_un_fil::<()>("essai", Progression::Exacte, |_, _| Err("non".into()))), Avancement::Echec(m) if m == "non"));
        assert!(matches!(sonder_jusqu_au_bout(&mut sur_un_fil::<()>("essai", Progression::Exacte, |_, _| panic!("boum"))), Avancement::Echec(_)));
    }

    #[test]
    fn l_avancement_est_celui_du_travail_ou_glisse_vers_le_palier_suivant() {
        let (signal, attente) = std::sync::mpsc::channel::<()>();
        let mut exacte = sur_un_fil("essai", Progression::Exacte, move |_, progres| {
            progres(0.25);
            let _ = signal.send(());
            std::thread::sleep(Duration::from_millis(300));
            Ok(())
        });
        attente.recv().unwrap();
        assert!(matches!(exacte.sonder(0.0), Avancement::EnCours(p) if p == 0.25));

        let (signal, attente) = std::sync::mpsc::channel::<()>();
        let mut paliers = sur_un_fil("essai", Progression::Paliers(&[0.5]), move |_, progres| {
            progres(0.1);
            let _ = signal.send(());
            std::thread::sleep(Duration::from_millis(300));
            Ok(())
        });
        attente.recv().unwrap();
        std::thread::sleep(Duration::from_millis(20));
        assert!(matches!(paliers.sonder(0.0), Avancement::EnCours(p) if p > 0.1 && p < 0.5));
    }

    #[test]
    fn arreter_leve_le_drapeau_du_travail() {
        let mut t = sur_un_fil::<()>("essai", Progression::Exacte, |arret, _| {
            while !arret.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(5));
            }
            Err("arrêté".into())
        });
        t.arreter();
        assert!(matches!(sonder_jusqu_au_bout(&mut t), Avancement::Echec(m) if m == "arrêté"));
    }

    #[test]
    fn attendre_rend_le_statut_ou_tue_le_programme_a_l_arret_ou_au_dela_du_delai() {
        let lancer = |s: &str| Command::new("/bin/sh").args(["-c", s]).stdin(Stdio::null()).spawn().unwrap();
        let pas = Duration::from_millis(10);
        let s = attendre(&mut lancer("exit 3"), &AtomicBool::new(false), None, pas, || {}).unwrap();
        assert_eq!(s.code(), Some(3));
        assert!(matches!(attendre(&mut lancer("sleep 5"), &AtomicBool::new(true), None, pas, || {}), Err(Interruption::Arrete)));
        let debut = Instant::now();
        assert!(matches!(attendre(&mut lancer("sleep 5"), &AtomicBool::new(false), Some(Duration::from_millis(50)), pas, || {}), Err(Interruption::Delai)));
        assert!(debut.elapsed() < Duration::from_secs(2));
    }
}
