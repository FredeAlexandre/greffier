//! Ce que les vrais adaptateurs partagent pour un travail long : le faire tourner sur un fil
//! ([`sur_un_fil`], une [`Tache`] que le cœur sonde) et attendre un programme lancé qu'on peut
//! arrêter ([`attendre`]).

use std::process::{Child, ExitStatus};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use greffier_core::adapters::{TaskProgress, Task, BoxedTask};

/// Comment l'avancement d'un travail sur un fil se lit.
#[derive(Clone, Copy, Debug)]
pub enum Progression {
    /// Le travail donne lui-même son avancement, de 0 à 1.
    Exact,
    /// Le travail annonce des paliers (Claude ne dit pas où il en est) : l'avancement glisse
    /// lentement vers le palier suivant (1 après le dernier) en attendant.
    Milestones(&'static [f64]),
}

/// Constante de temps du glissement vers le palier suivant, en secondes.
const GLIDE_TIME: f64 = 120.0;

struct State<T> {
    /// L'avancement annoncé, et quand.
    progress: f64,
    from_meeting: Instant,
    result: Option<Result<T, String>>,
}

/// Un travail sur un fil ; `sonder` lit son état.
struct OnThread<T> {
    state: Arc<Mutex<State<T>>>,
    stop: Arc<AtomicBool>,
    progression: Progression,
}

/// Lance `travail` sur un fil nommé `nom`. Il reçoit le drapeau d'arrêt (levé par `arreter()` ou
/// quand la tâche est lâchée) et de quoi annoncer son avancement. Un travail qui panique finit en
/// échec.
pub fn on_thread<T: Send + 'static>(name: &str, progression: Progression, job: impl FnOnce(&AtomicBool, &mut dyn FnMut(f64)) -> Result<T, String> + Send + 'static) -> BoxedTask<T> {
    let state = Arc::new(Mutex::new(State { progress: 0.0, from_meeting: Instant::now(), result: None }));
    let stop = Arc::new(AtomicBool::new(false));
    let (e, a) = (state.clone(), stop.clone());
    let spawned = std::thread::Builder::new().name(name.into()).spawn(move || {
        let mut progress = |p: f64| {
            let mut e = e.lock().unwrap_or_else(|e| e.into_inner());
            e.progress = p;
            e.from_meeting = Instant::now();
        };
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| job(&a, &mut progress))).unwrap_or_else(|_| Err("le travail s'est arrêté sans résultat".into()));
        e.lock().unwrap_or_else(|e| e.into_inner()).result = Some(r);
    });
    if let Err(err) = spawned {
        state.lock().unwrap_or_else(|e| e.into_inner()).result = Some(Err(format!("Impossible de lancer {name} : {err}")));
    }
    Box::new(OnThread { state, stop, progression })
}

impl<T> Task<T> for OnThread<T> {
    fn poll(&mut self, _: f64) -> TaskProgress<T> {
        let mut e = self.state.lock().unwrap_or_else(|e| e.into_inner());
        match e.result.take() {
            Some(Ok(v)) => TaskProgress::Done(v),
            Some(Err(m)) => TaskProgress::Failed(m),
            None => match self.progression {
                Progression::Exact => TaskProgress::Running(e.progress),
                Progression::Milestones(milestones) => {
                    let next = milestones.iter().copied().find(|p| *p > e.progress).unwrap_or(1.0);
                    let t = e.from_meeting.elapsed().as_secs_f64();
                    TaskProgress::Running(e.progress + (next - e.progress) * 0.9 * (1.0 - (-t / GLIDE_TIME).exp()))
                }
            },
        }
    }

    fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl<T> Drop for OnThread<T> {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Pourquoi un programme attendu n'est pas allé jusqu'au bout.
#[derive(Debug)]
pub enum Interruption {
    /// Le drapeau d'arrêt s'est levé : le programme a été tué.
    Stopped,
    /// Le délai est dépassé : le programme a été tué.
    Timeout,
    /// Son état n'a pas pu être lu.
    Error(std::io::Error),
}

/// Attend la fin de `enfant` en regardant toutes les `pas` ; le tue si `arret` se lève ou si
/// `delai` est dépassé. `pendant` est appelé à chaque tour (pour relire un avancement).
pub fn wait(child: &mut Child, stop: &AtomicBool, timeout: Option<Duration>, step: Duration, mut during: impl FnMut()) -> Result<ExitStatus, Interruption> {
    let start = Instant::now();
    loop {
        let interruption = if stop.load(Ordering::Relaxed) {
            Some(Interruption::Stopped)
        } else if timeout.is_some_and(|d| start.elapsed() > d) {
            Some(Interruption::Timeout)
        } else {
            None
        };
        if let Some(i) = interruption {
            let _ = child.kill();
            let _ = child.wait();
            return Err(i);
        }
        match child.try_wait() {
            Ok(Some(s)) => return Ok(s),
            Ok(None) => {}
            Err(e) => return Err(Interruption::Error(e)),
        }
        during();
        std::thread::sleep(step);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Command, Stdio};

    fn poll_to_completion<T>(t: &mut BoxedTask<T>) -> TaskProgress<T> {
        for _ in 0..500 {
            match t.poll(0.0) {
                TaskProgress::Running(_) => std::thread::sleep(Duration::from_millis(10)),
                on_done => return on_done,
            }
        }
        panic!("la tâche ne finit pas");
    }

    #[test]
    fn a_job_on_a_thread_returns_its_result_its_failure_or_a_panic_failure() {
        assert!(matches!(poll_to_completion(&mut on_thread("essai", Progression::Exact, |_, _| Ok(3))), TaskProgress::Done(3)));
        assert!(matches!(poll_to_completion(&mut on_thread::<()>("essai", Progression::Exact, |_, _| Err("non".into()))), TaskProgress::Failed(m) if m == "non"));
        assert!(matches!(poll_to_completion(&mut on_thread::<()>("essai", Progression::Exact, |_, _| panic!("boum"))), TaskProgress::Failed(_)));
    }

    #[test]
    fn progress_follows_the_work_or_glides_toward_the_next_milestone() {
        let (signal, waiting) = std::sync::mpsc::channel::<()>();
        let mut exact = on_thread("essai", Progression::Exact, move |_, progress| {
            progress(0.25);
            let _ = signal.send(());
            std::thread::sleep(Duration::from_millis(300));
            Ok(())
        });
        waiting.recv().unwrap();
        assert!(matches!(exact.poll(0.0), TaskProgress::Running(p) if p == 0.25));

        let (signal, waiting) = std::sync::mpsc::channel::<()>();
        let mut milestones = on_thread("essai", Progression::Milestones(&[0.5]), move |_, progress| {
            progress(0.1);
            let _ = signal.send(());
            std::thread::sleep(Duration::from_millis(300));
            Ok(())
        });
        waiting.recv().unwrap();
        std::thread::sleep(Duration::from_millis(20));
        assert!(matches!(milestones.poll(0.0), TaskProgress::Running(p) if p > 0.1 && p < 0.5));
    }

    #[test]
    fn stop_raises_the_work_flag() {
        let mut t = on_thread::<()>("essai", Progression::Exact, |stop, _| {
            while !stop.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(5));
            }
            Err("arrêté".into())
        });
        t.stop();
        assert!(matches!(poll_to_completion(&mut t), TaskProgress::Failed(m) if m == "arrêté"));
    }

    #[test]
    fn wait_returns_the_status_or_kills_the_program_on_stop_or_past_the_timeout() {
        let launch = |s: &str| Command::new("/bin/sh").args(["-c", s]).stdin(Stdio::null()).spawn().unwrap();
        let step = Duration::from_millis(10);
        let s = wait(&mut launch("exit 3"), &AtomicBool::new(false), None, step, || {}).unwrap();
        assert_eq!(s.code(), Some(3));
        assert!(matches!(wait(&mut launch("sleep 5"), &AtomicBool::new(true), None, step, || {}), Err(Interruption::Stopped)));
        let start = Instant::now();
        assert!(matches!(wait(&mut launch("sleep 5"), &AtomicBool::new(false), Some(Duration::from_millis(50)), step, || {}), Err(Interruption::Timeout)));
        assert!(start.elapsed() < Duration::from_secs(2));
    }
}
