//! Smoke tests of the real engines on this Mac, run on purpose with `cargo xtask smoke`
//! (`#[ignore]`d by `cargo test`): whisper.cpp and pyannote, with the models installed in the app
//! folder, on an excerpt of a real meeting.
//!
//! The meeting is not in the repository: `GREFFIER_SMOKE_AUDIO=<media>` (default
//! `~/Bench-Transcription/audio/A.wav`), and `GREFFIER_SMOKE_EXCERPT=<start>-<end>` in seconds
//! (default `230-260`: 30 s where two people talk). ffmpeg cuts the excerpt into a temporary AAC
//! file, like a recording's mix track. Without the media, ffmpeg or the models, the tests are
//! skipped; with them, they fail if the engine's behaviour is wrong. Only what holds for any
//! speech is checked, never the exact words.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use greffier_core::adapters::{BoxedTask, DiarizationRequest, Diarizer, TaskProgress, TranscriptionRequest, Transcriber};
use greffier_core::attribution::without_noises;
use greffier_core::resources::{PYANNOTE_MODEL, VAD_MODEL, WHISPER_MODEL};
use greffier_engines::{PyannoteDiarizer, WhisperTranscriber};

/// Words that come back in any French conversation.
const FRENCH: [&str; 30] = [
    "le", "la", "les", "de", "des", "du", "un", "une", "et", "est", "que", "qui", "on", "pas", "je", "tu", "il", "ça", "c'est", "pour", "en", "à", "au", "ce", "mais", "ou", "donc", "alors", "oui", "ouais",
];

#[test]
#[ignore = "real Mac: whisper large-v3 on a real meeting excerpt (cargo xtask smoke)"]
fn whisper_transcribes_an_excerpt_into_timed_french_words() {
    let app = greffier_core::resources::app_folder();
    for model in [WHISPER_MODEL, VAD_MODEL] {
        if !app.join(model).is_file() {
            return skip(&format!("{} is not installed: launch Greffier once to install its resources", app.join(model).display()));
        }
    }
    let Some(excerpt) = Excerpt::cut("whisper") else { return };

    let mut engine = WhisperTranscriber::from_app(&app);
    let task = engine.transcribe(TranscriptionRequest { audio: excerpt.file.clone(), duration: excerpt.seconds, vocabulary: String::new() });
    let words = finish(task, "whisper");
    let length = excerpt.seconds;

    let n = words.len() as f64;
    eprintln!("whisper: {n} words in {length} s: {}", words.iter().map(|m| m.text.as_str()).collect::<Vec<_>>().join(" "));
    assert!((length..=6.0 * length).contains(&n), "{n} words for {length} s of conversation: between 1 and 6 per second expected");
    for m in &words {
        assert!(0.0 <= m.start && m.start <= m.end && m.end <= length + 0.5, "« {} » at {:.2} → {:.2} s, outside the {length} s excerpt", m.text, m.start, m.end);
        assert!(!m.text.contains("[_") && !m.text.contains("<|"), "a whisper special token in the text: « {} »", m.text);
    }
    assert!(words.windows(2).all(|w| w[0].start <= w[1].start), "words out of order: {:?}", words.iter().map(|m| m.start).collect::<Vec<_>>());
    let last = words.last().map_or(0.0, |m| m.end);
    assert!(last >= length - 8.0, "the last word ends at {last:.2} s of {length} s: the timestamps do not cover the excerpt (wrong sample rate?)");

    // Speech, not noise annotations (`[Musique]`, `(rires)`, which the core drops).
    let spoken = without_noises(words.clone()).len() as f64;
    assert!(spoken >= 0.9 * n, "only {spoken} of {n} words are left once the noises are dropped");
    // French, as the vault's parameters ask: not translated, not another language.
    let french = words.iter().filter(|m| FRENCH.contains(&m.text.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'').to_lowercase().as_str())).count() as f64;
    assert!(french >= 0.15 * n, "{french} of {n} words are common French words: not a French transcription?");
}

#[test]
#[ignore = "real Mac: pyannote in the private Python on a real meeting excerpt (cargo xtask smoke)"]
fn pyannote_splits_an_excerpt_between_the_two_requested_speakers() {
    let app = greffier_core::resources::app_folder();
    let engine = PyannoteDiarizer::from_app(&app);
    if !greffier_tools::mac::python(&engine.python).is_file() {
        return skip(&format!("the private Python is not installed in {}: launch Greffier once to install its resources", engine.python.display()));
    }
    if !app.join(PYANNOTE_MODEL).is_dir() {
        return skip(&format!("{} is not installed: launch Greffier once to install its resources", app.join(PYANNOTE_MODEL).display()));
    }
    let Some(excerpt) = Excerpt::cut("pyannote") else { return };

    let mut engine = engine;
    let task = engine.diarize(DiarizationRequest { audio: excerpt.file.clone(), duration: excerpt.seconds, speaker_count: Some(2) });
    let turns = finish(task, "pyannote");
    let length = excerpt.seconds;
    eprintln!("pyannote: {}", turns.iter().map(|t| format!("{:.1}–{:.1} {}", t.start, t.end, t.speaker)).collect::<Vec<_>>().join(", "));

    let mut speakers: Vec<&str> = turns.iter().map(|t| t.speaker.as_str()).collect();
    speakers.sort_unstable();
    speakers.dedup();
    assert_eq!(speakers.len(), 2, "two speakers asked for: {speakers:?}");
    for t in &turns {
        assert!(0.0 <= t.start && t.start < t.end && t.end <= length + 0.5, "turn {:.2} → {:.2} s outside the {length} s excerpt", t.start, t.end);
    }
    // Exclusive turns, in order: one speaker at a time, as the transcription is attributed.
    assert!(turns.windows(2).all(|w| w[0].end <= w[1].start + 1e-3), "turns out of order or overlapping: {turns:?}");
    let total = |s: &str| turns.iter().filter(|t| t.speaker == s).map(|t| t.end - t.start).sum::<f64>();
    let speech: f64 = speakers.iter().map(|s| total(s)).sum();
    assert!(speech >= 0.3 * length, "{speech:.1} s of speech found in {length} s of conversation");
    for s in &speakers {
        assert!(total(s) >= 1.0, "{s} speaks {:.2} s: a speaker that does not really talk", total(s));
    }
}

/// Waits for an adapter's task, checking its progress goes forward within 0–1.
fn finish<T>(mut task: BoxedTask<T>, what: &str) -> T {
    let start = Instant::now();
    let mut last = 0.0;
    loop {
        match task.poll(0.1) {
            TaskProgress::Done(v) => {
                eprintln!("{what}: {:.1} s", start.elapsed().as_secs_f64());
                return v;
            }
            TaskProgress::Failed(e) => panic!("{what} failed: {e}"),
            TaskProgress::Running(p) => {
                assert!((0.0..=1.0).contains(&p) && p >= last, "{what}: progress {p} after {last}");
                last = p;
            }
        }
        assert!(start.elapsed() < Duration::from_secs(600), "{what}: more than 10 min for a short excerpt");
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// The excerpt of the meeting, cut by ffmpeg into a temporary AAC file (48 kHz stereo, like a
/// recording's mix track).
struct Excerpt {
    file: PathBuf,
    seconds: f64,
}

impl Excerpt {
    /// `None` (and says why) if the meeting or ffmpeg is missing.
    fn cut(name: &str) -> Option<Excerpt> {
        let media = std::env::var_os("GREFFIER_SMOKE_AUDIO").map(PathBuf::from).unwrap_or_else(|| greffier_core::settings::home().join("Bench-Transcription/audio/A.wav"));
        if !media.is_file() {
            skip(&format!("no meeting to transcribe at {}: set GREFFIER_SMOKE_AUDIO=<media of a meeting with at least two people>", media.display()));
            return None;
        }
        let Some(ffmpeg) = program("ffmpeg") else {
            skip("ffmpeg not found (brew install ffmpeg)");
            return None;
        };
        let window = std::env::var("GREFFIER_SMOKE_EXCERPT").unwrap_or_else(|_| "230-260".into());
        let (start, end) = window.split_once('-').and_then(|(a, b)| Some((a.trim().parse::<f64>().ok()?, b.trim().parse::<f64>().ok()?))).filter(|(a, b)| b > a).unwrap_or_else(|| panic!("GREFFIER_SMOKE_EXCERPT={window}: expected <start>-<end> in seconds"));
        let file = std::env::temp_dir().join(format!("greffier-smoke-{name}-{}.m4a", std::process::id()));
        let out = Command::new(ffmpeg)
            .args(["-v", "error", "-y", "-ss", &start.to_string(), "-t", &(end - start).to_string(), "-i"])
            .arg(&media)
            .args(["-vn", "-ac", "2", "-ar", "48000", "-c:a", "aac", "-b:a", "160k"])
            .arg(&file)
            .output()
            .expect("ffmpeg");
        assert!(out.status.success(), "ffmpeg could not cut {window} s out of {}: {}", media.display(), String::from_utf8_lossy(&out.stderr));
        let seconds = greffier_core::folder::mp4_duration(&file).unwrap_or(0.0);
        assert!((seconds - (end - start)).abs() < 0.5, "the excerpt lasts {seconds:.1} s instead of {} s: is {} shorter than {end} s? Set GREFFIER_SMOKE_EXCERPT", end - start, media.display());
        Some(Excerpt { file, seconds: end - start })
    }
}

impl Drop for Excerpt {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.file);
    }
}

/// Says why the test checks nothing on this Mac (`cargo xtask smoke` lists these lines).
fn skip(why: &str) {
    eprintln!("SKIPPED: {why}");
}

/// A program from the PATH, or from Homebrew (an IDE may not have the terminal's PATH).
fn program(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect::<Vec<_>>()).unwrap_or_default();
    path.into_iter().chain(["/opt/homebrew/bin", "/usr/local/bin"].map(PathBuf::from)).map(|d| d.join(name)).find(|p: &PathBuf| Path::is_file(p))
}
