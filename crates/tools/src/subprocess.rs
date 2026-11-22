//! What the real adapters share for a long job: running it on a thread ([`on_thread`], a
//! [`Task`] that the core polls) and waiting for a launched program that can be stopped
//! ([`wait`]).

use std::process::{Child, ExitStatus};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use greffier_core::adapters::{TaskProgress, Task, BoxedTask};

/// How the progress of a job on a thread is read.
#[derive(Clone, Copy, Debug)]
pub enum Progression {
    /// The job reports its own progress, from 0 to 1.
    Exact,
    /// The job announces milestones (Claude does not say how far along it is): meanwhile, the
    /// progress slowly glides toward the next milestone (1 after the last one).
    Milestones(&'static [f64]),
}

/// Time constant of the glide toward the next milestone, in seconds.
const GLIDE_TIME: f64 = 120.0;

struct State<T> {
    /// The announced progress, and when.
    progress: f64,
    from_meeting: Instant,
    result: Option<Result<T, String>>,
}

/// A job on a thread; `poll` reads its state.
struct OnThread<T> {
    state: Arc<Mutex<State<T>>>,
    stop: Arc<AtomicBool>,
    progression: Progression,
}

/// Runs `job` on a thread named `name`. It receives the stop flag (raised by `stop()` or when the
/// task is dropped) and a way to announce its progress. A job that panics ends in failure.
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

/// Why a waited-for program did not run to the end.
#[derive(Debug)]
pub enum Interruption {
    /// The stop flag was raised: the program was killed.
    Stopped,
    /// The timeout was exceeded: the program was killed.
    Timeout,
    /// Its state could not be read.
    Error(std::io::Error),
}

/// Waits for `child` to finish, checking every `step`; kills it if `stop` is raised or if
/// `timeout` is exceeded. `during` is called on each round (to read a progress again).
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
        panic!("the task does not finish");
    }

    #[test]
    fn a_job_on_a_thread_returns_its_result_its_failure_or_a_panic_failure() {
        assert!(matches!(poll_to_completion(&mut on_thread("test", Progression::Exact, |_, _| Ok(3))), TaskProgress::Done(3)));
        assert!(matches!(poll_to_completion(&mut on_thread::<()>("test", Progression::Exact, |_, _| Err("no".into()))), TaskProgress::Failed(m) if m == "no"));
        assert!(matches!(poll_to_completion(&mut on_thread::<()>("test", Progression::Exact, |_, _| panic!("boom"))), TaskProgress::Failed(_)));
    }

    #[test]
    fn progress_follows_the_work_or_glides_toward_the_next_milestone() {
        let (signal, waiting) = std::sync::mpsc::channel::<()>();
        let mut exact = on_thread("test", Progression::Exact, move |_, progress| {
            progress(0.25);
            let _ = signal.send(());
            std::thread::sleep(Duration::from_millis(300));
            Ok(())
        });
        waiting.recv().unwrap();
        assert!(matches!(exact.poll(0.0), TaskProgress::Running(p) if p == 0.25));

        let (signal, waiting) = std::sync::mpsc::channel::<()>();
        let mut milestones = on_thread("test", Progression::Milestones(&[0.5]), move |_, progress| {
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
        let mut t = on_thread::<()>("test", Progression::Exact, |stop, _| {
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
