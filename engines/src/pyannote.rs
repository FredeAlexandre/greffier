//! Diarization by pyannote community-1, in the app's private Python (ADR 0003): the mix track
//! extracted by AVFoundation ([`crate::audio`]) is written as WAV into the private Python's
//! temporary folder, then `diarize.py` (embedded in the app) diarizes it offline, on MPS with a
//! fallback to the CPU, into exclusive speaker turns.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use greffier_core::SpeakerTurn;
use greffier_core::adapters::{TaskProgress, DiarizationRequest, Diarizer, Task, BoxedTask};
use greffier_tools::mac;
use greffier_tools::subprocess::{Interruption, wait};
use greffier_core::resources::PYANNOTE_MODEL;

use crate::audio;

/// The script run in the private Python.
pub const SCRIPT: &str = include_str!("diarize.py");
/// Share of the progress given to audio extraction (the rest goes to pyannote).
const AUDIO_SHARE: f64 = 0.05;

/// pyannote community-1 in the app's private Python.
#[derive(Clone)]
pub struct PyannoteDiarizer {
    /// The private Python's folder (`<app>/python`).
    pub python: PathBuf,
    /// The model's folder (`<app>/models/pyannote-community-1`).
    pub model: PathBuf,
}

impl PyannoteDiarizer {
    /// The private Python and the model installed in the app folder.
    pub fn from_app(app_folder: &Path) -> PyannoteDiarizer {
        PyannoteDiarizer { python: app_folder.join("python"), model: app_folder.join(PYANNOTE_MODEL) }
    }

    /// Diarizes on the calling thread: the audio, then pyannote.
    pub fn diarize_here(&self, request: &DiarizationRequest, stop: &AtomicBool, progress: &mut dyn FnMut(f64)) -> Result<Vec<SpeakerTurn>, String> {
        let samples = audio::mixing(&request.audio, stop, |p| progress(p * AUDIO_SHARE))?;
        if samples.len() < audio::SAMPLE_RATE as usize {
            return Err("Le média n'a pas de son.".into());
        }
        let temp = self.python.join("tmp");
        std::fs::create_dir_all(&temp).map_err(|e| format!("{} : {e}", temp.display()))?;
        let base = temp.join(format!("diarization-{}-{}", std::process::id(), number()));
        let (wav, json) = (base.with_extension("wav"), base.with_extension("json"));
        let result = (|| {
            audio::write_wav(&wav, &samples).map_err(|e| format!("{} : {e}", wav.display()))?;
            drop(samples);
            self.launch(&wav, &json, request.speaker_count, &temp, stop, &mut |p| progress(AUDIO_SHARE + p * (1.0 - AUDIO_SHARE)))?;
            let contents = std::fs::read_to_string(&json).map_err(|e| format!("pyannote n'a rien écrit ({e})."))?;
            turns(&contents)
        })();
        let _ = std::fs::remove_file(&wav);
        let _ = std::fs::remove_file(&json);
        result
    }

    fn launch(&self, wav: &Path, json: &Path, speakers: Option<u8>, folder: &Path, stop: &AtomicBool, progress: &mut dyn FnMut(f64)) -> Result<(), String> {
        let python = mac::python(&self.python);
        if !python.exists() {
            return Err(format!("Le Python privé n'est pas installé ({}).", python.display()));
        }
        let mut command = Command::new(&python);
        command.arg("-I").arg("-c").arg(SCRIPT).arg(wav).arg(json).arg(&self.model);
        if let Some(n) = speakers {
            command.arg("--speakers").arg(n.to_string());
        }
        let mut child = command
            .env_clear()
            .envs(mac::python_env(&self.python))
            .current_dir(folder)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Impossible de lancer pyannote ({}) : {e}", python.display()))?;

        // Stderr is read as it comes (otherwise the pipe fills up and blocks pyannote): the
        // progress, and the last lines for the error message.
        let fraction = Arc::new(AtomicU64::new(0f64.to_bits()));
        let last_lines = Arc::new(Mutex::new(Vec::<String>::new()));
        let error = child.stderr.take().expect("stderr");
        let (a, d) = (fraction.clone(), last_lines.clone());
        let reader = std::thread::spawn(move || {
            for line in BufReader::new(error).lines().map_while(Result::ok) {
                if let Some(p) = line.strip_prefix("PROGRESS ").and_then(|p| p.trim().parse::<f64>().ok()) {
                    a.store(p.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
                } else if !line.trim().is_empty() {
                    let mut d = d.lock().unwrap_or_else(|e| e.into_inner());
                    d.push(line);
                    let excess = d.len().saturating_sub(5);
                    d.drain(..excess);
                }
            }
        });

        let status = match wait(&mut child, stop, None, Duration::from_millis(100), || progress(f64::from_bits(fraction.load(Ordering::Relaxed)))) {
            Ok(s) => s,
            Err(Interruption::Error(e)) => return Err(format!("pyannote : {e}")),
            Err(Interruption::Stopped | Interruption::Timeout) => {
                let _ = reader.join();
                return Err("arrêté".into());
            }
        };
        let _ = reader.join();
        if !status.success() {
            let d = last_lines.lock().unwrap_or_else(|e| e.into_inner());
            return Err(format!("pyannote a échoué ({status}) : {}", d.join(" · ")));
        }
        progress(1.0);
        Ok(())
    }
}

/// One number per diarization, so that two diarizations don't write to the same file.
fn number() -> u64 {
    static N: AtomicU64 = AtomicU64::new(0);
    N.fetch_add(1, Ordering::Relaxed)
}

/// The speaker turns from `diarize.py`'s JSON, labelled SPEAKER_00, SPEAKER_01… in pyannote's
/// order (the core renumbers them in order of first word).
pub fn turns(json: &str) -> Result<Vec<SpeakerTurn>, String> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|e| format!("Sortie de pyannote illisible : {e}"))?;
    let segments = v["segments"].as_array().ok_or("Sortie de pyannote sans segments.")?;
    segments
        .iter()
        .map(|s| match (s["start"].as_f64(), s["end"].as_f64(), s["speaker"].as_u64()) {
            (Some(start), Some(end), Some(n)) => Ok(SpeakerTurn { start, end, speaker: greffier_core::Speaker::numbered_label(n as usize) }),
            _ => Err(format!("Tour de parole illisible : {s}")),
        })
        .collect()
}

#[derive(Default)]
struct State {
    progress: f64,
    result: Option<Result<Vec<SpeakerTurn>, String>>,
}

struct Diarization {
    state: Arc<Mutex<State>>,
    stop: Arc<AtomicBool>,
}

impl Task<Vec<SpeakerTurn>> for Diarization {
    fn poll(&mut self, _: f64) -> TaskProgress<Vec<SpeakerTurn>> {
        let mut e = self.state.lock().unwrap_or_else(|e| e.into_inner());
        match e.result.take() {
            Some(Ok(turns)) => TaskProgress::Done(turns),
            Some(Err(m)) => TaskProgress::Failed(m),
            None => TaskProgress::Running(e.progress),
        }
    }

    fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Diarizer for PyannoteDiarizer {
    fn diarize(&mut self, request: DiarizationRequest) -> BoxedTask<Vec<SpeakerTurn>> {
        let state = Arc::new(Mutex::new(State::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let engine = self.clone();
        let (e, a) = (state.clone(), stop.clone());
        let spawned = std::thread::Builder::new().name("pyannote".into()).spawn(move || {
            let mut progress = |p: f64| e.lock().unwrap_or_else(|e| e.into_inner()).progress = p.clamp(0.0, 1.0);
            let r = engine.diarize_here(&request, &a, &mut progress);
            e.lock().unwrap_or_else(|e| e.into_inner()).result = Some(r);
        });
        if let Err(err) = spawned {
            state.lock().unwrap_or_else(|e| e.into_inner()).result = Some(Err(format!("Impossible de lancer la séparation des locuteurs : {err}")));
        }
        Box::new(Diarization { state, stop })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pyannote_turns_are_read_and_labeled() {
        let json = r#"{"segments": [{"start": 0.5, "end": 2.25, "speaker": 0}, {"start": 2.25, "end": 3.0, "speaker": 11}], "speakers": 2, "device": "mps", "seconds": 1.2}"#;
        let t = turns(json).expect("read");
        assert_eq!(t, [SpeakerTurn { start: 0.5, end: 2.25, speaker: "SPEAKER_00".into() }, SpeakerTurn { start: 2.25, end: 3.0, speaker: "SPEAKER_11".into() }]);
        assert!(turns("{}").is_err());
    }
}
