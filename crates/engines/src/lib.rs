//! Greffier's real engines, plugged into the core's adapters.
//!
//! - [`WhisperTranscriber`]: the mix track extracted by AVFoundation ([`audio`]), then
//!   transcribed by whisper.cpp ([`whisper`]), built into the app from its sources (build.rs),
//!   with the vault's parameters.
//! - [`PyannoteDiarizer`]: the same mix track, split into speaker turns by pyannote community-1
//!   in the app's private Python ([`pyannote`]), while whisper transcribes.
//!
//! These engines are not tested by the automated suite (the core is, with fake adapters); they
//! are checked by hand with
//! `cargo run -p greffier-engines --example transcribe -- <media>` (and `--example diarize`).

pub mod audio;
pub mod pyannote;
pub mod resampling;
pub mod whisper;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use greffier_core::Word;
pub use pyannote::PyannoteDiarizer;
use greffier_core::adapters::{TaskProgress, TranscriptionRequest, Task, BoxedTask, Transcriber};

/// Share of the progress given to audio extraction (the rest goes to whisper).
const AUDIO_SHARE: f64 = 0.05;

/// whisper.cpp large-v3 on Metal, Silero VAD, with the models from the app folder.
pub struct WhisperTranscriber {
    pub model: PathBuf,
    pub vad: PathBuf,
}

impl WhisperTranscriber {
    /// The models installed in the app folder.
    pub fn from_app(app_folder: &std::path::Path) -> WhisperTranscriber {
        use greffier_core::resources::{VAD_MODEL, WHISPER_MODEL};
        WhisperTranscriber { model: app_folder.join(WHISPER_MODEL), vad: app_folder.join(VAD_MODEL) }
    }

    /// Transcribes on the calling thread: the audio, then whisper.
    pub fn transcribe_here(&self, request: &TranscriptionRequest, stop: &AtomicBool, progress: &mut dyn FnMut(f64)) -> Result<Vec<Word>, String> {
        let samples = audio::mixing(&request.audio, stop, |p| progress(p * AUDIO_SHARE))?;
        if samples.len() < audio::SAMPLE_RATE as usize {
            return Err("Le média n'a pas de son.".into());
        }
        whisper::transcribe(&self.model, &self.vad, &samples, &request.vocabulary, stop, &mut |p| progress(AUDIO_SHARE + p * (1.0 - AUDIO_SHARE)))
    }
}

#[derive(Default)]
struct State {
    progress: f64,
    result: Option<Result<Vec<Word>, String>>,
}

struct Transcription {
    state: Arc<Mutex<State>>,
    stop: Arc<AtomicBool>,
}

impl Task<Vec<Word>> for Transcription {
    fn poll(&mut self, _: f64) -> TaskProgress<Vec<Word>> {
        let mut e = self.state.lock().unwrap_or_else(|e| e.into_inner());
        match e.result.take() {
            Some(Ok(words)) => TaskProgress::Done(words),
            Some(Err(m)) => TaskProgress::Failed(m),
            None => TaskProgress::Running(e.progress),
        }
    }

    fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Transcriber for WhisperTranscriber {
    fn transcribe(&mut self, request: TranscriptionRequest) -> BoxedTask<Vec<Word>> {
        let state = Arc::new(Mutex::new(State::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let engine = WhisperTranscriber { model: self.model.clone(), vad: self.vad.clone() };
        let (e, a) = (state.clone(), stop.clone());
        let spawned = std::thread::Builder::new().name("whisper".into()).spawn(move || {
            let mut progress = |p: f64| e.lock().unwrap_or_else(|e| e.into_inner()).progress = p.clamp(0.0, 1.0);
            let r = engine.transcribe_here(&request, &a, &mut progress);
            e.lock().unwrap_or_else(|e| e.into_inner()).result = Some(r);
        });
        if let Err(err) = spawned {
            state.lock().unwrap_or_else(|e| e.into_inner()).result = Some(Err(format!("Impossible de lancer la transcription : {err}")));
        }
        Box::new(Transcription { state, stop })
    }
}

/// Relistening to the review's doubtful passages: the recording is decoded once, then each
/// excerpt is transcribed again by whisper.cpp with the transcription's settings (same models,
/// VAD, vocabulary as prompt).
pub struct WhisperRelisten {
    pub model: PathBuf,
    pub vad: PathBuf,
}

impl WhisperRelisten {
    pub fn from_app(app_folder: &std::path::Path) -> WhisperRelisten {
        let t = WhisperTranscriber::from_app(app_folder);
        WhisperRelisten { model: t.model, vad: t.vad }
    }
}

impl greffier_tools::claude::review::Relisten for WhisperRelisten {
    fn relisten(&self, recording: &std::path::Path, excerpts: &[(f64, f64)], vocabulary: &str, stop: &AtomicBool) -> Result<Vec<String>, String> {
        let samples = audio::mixing(recording, stop, |_| {})?;
        let f = audio::SAMPLE_RATE as f64;
        let mut texts = Vec::with_capacity(excerpts.len());
        for &(from, a) in excerpts {
            let (i, j) = (((from.max(0.0) * f) as usize).min(samples.len()), ((a * f) as usize).min(samples.len()));
            if j <= i {
                texts.push(String::new());
                continue;
            }
            match whisper::transcribe(&self.model, &self.vad, &samples[i..j], vocabulary, stop, &mut |_| {}) {
                Ok(words) => texts.push(words.iter().map(|m| m.text.trim()).filter(|t| !t.is_empty()).collect::<Vec<_>>().join(" ")),
                Err(e) if stop.load(Ordering::Relaxed) => return Err(e),
                // A failed excerpt doesn't stop the others: the agent will decide without it.
                Err(_) => texts.push(String::new()),
            }
        }
        Ok(texts)
    }
}
