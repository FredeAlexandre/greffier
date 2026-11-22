//! The engine of the parity bench (`scripts/parity-bench`): runs a medium through Greffier's real
//! engines, like the core's Transcription stage (whisper.cpp with the lexicon's vocabulary,
//! pyannote, then `attribution::attribute(without_noises(words), turns)`), and writes the result
//! as JSON. The two engines run one after the other so their durations can be compared with the
//! vault's.
//!
//!     cargo run --release -p greffier-engines --example bench -- <media> <output.json> \
//!         [--lexicon lexique.md] [--speakers N] [--wav audio.wav]
//!
//! - `--lexicon`: the lexicon whose Vocabulaire section is given to whisper (defaults to the one
//!   in the app folder, like Greffier).
//! - `--wav`: also writes the decoded mix track (16 kHz mono PCM), to compare it with ffmpeg's.
//!
//! Other use, `bench --cut <media> <start> <end> <output.wav>`: writes the mix track decoded
//! by AVFoundation between two times (in seconds), like
//! `ffmpeg -ss <start> -to <end> -i <media> -vn -ac 1 -ar 16000 -c:a pcm_s16le` (the vault bench's
//! WAV).
//!
//! Output: `{ audio, vocabulary, transcription_seconds, diarization_seconds, raw: [whisper's
//! words], words: [{start, end, text, speaker}], turns: [{start, end, speaker}] }`. Only the
//! output file, the requested WAV and pyannote's temporary WAV (in the app folder) are
//! written.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use greffier_core::adapters::{DiarizationRequest, TranscriptionRequest};
use greffier_core::{attribution, lexicon, resources};
use greffier_engines::{PyannoteDiarizer, WhisperTranscriber, audio};
use serde_json::{Value, json};

fn slice(args: &[String]) {
    let [media, start, end, output] = args else { panic!("usage: bench --cut <media> <start> <end> <output.wav>") };
    let instant = |s: &String| (s.parse::<f64>().expect("time in seconds") * audio::SAMPLE_RATE as f64).round() as usize;
    let samples = audio::mixing(std::path::Path::new(media), &AtomicBool::new(false), |_| {}).unwrap_or_else(|e| panic!("audio: {e}"));
    let (from, a) = (instant(start).min(samples.len()), instant(end).min(samples.len()));
    audio::write_wav(std::path::Path::new(output), &samples[from..a]).unwrap_or_else(|e| panic!("{output}: {e}"));
}

fn main() {
    let all: Vec<String> = std::env::args().skip(1).collect();
    if all.first().map(String::as_str) == Some("--cut") {
        return slice(&all[1..]);
    }
    let mut args = all.into_iter();
    let mut positional = Vec::new();
    let (mut lexicon_md, mut number, mut wav) = (None, None, None);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--lexicon" => lexicon_md = Some(PathBuf::from(args.next().expect("--lexicon <file>"))),
            "--speakers" => number = Some(args.next().and_then(|n| n.parse().ok()).expect("--speakers <number>")),
            "--wav" => wav = Some(PathBuf::from(args.next().expect("--wav <file>"))),
            _ => positional.push(PathBuf::from(a)),
        }
    }
    let [media, output] = <[PathBuf; 2]>::try_from(positional).expect("usage: bench <media> <output.json> [--lexicon f] [--speakers N] [--wav f]");
    let app = resources::app_folder();
    let lexicon_md = lexicon_md.unwrap_or_else(|| app.join("lexique.md"));
    let vocabulary = std::fs::read_to_string(&lexicon_md).map(|md| lexicon::vocabulary(&md)).unwrap_or_default();
    let stop = AtomicBool::new(false);

    if let Some(wav) = &wav {
        let samples = audio::mixing(&media, &stop, |_| {}).unwrap_or_else(|e| panic!("audio: {e}"));
        audio::write_wav(wav, &samples).unwrap_or_else(|e| panic!("{}: {e}", wav.display()));
    }

    let t = Instant::now();
    let request = TranscriptionRequest { audio: media.clone(), duration: 0.0, vocabulary: vocabulary.clone() };
    let words = WhisperTranscriber::from_app(&app).transcribe_here(&request, &stop, &mut |_| {}).unwrap_or_else(|e| panic!("transcription: {e}"));
    let transcription_seconds = t.elapsed().as_secs_f64();

    let t = Instant::now();
    let request = DiarizationRequest { audio: media.clone(), duration: 0.0, speaker_count: number };
    let turns = PyannoteDiarizer::from_app(&app).diarize_here(&request, &stop, &mut |_| {}).unwrap_or_else(|e| panic!("diarization: {e}"));
    let diarization_seconds = t.elapsed().as_secs_f64();

    let raw: Vec<Value> = words.iter().map(|m| json!({ "start": m.start, "end": m.end, "text": m.text })).collect();
    let a = attribution::attribute(attribution::without_noises(words), turns);
    let label = |i: Option<usize>| i.and_then(|i| a.speakers.get(i)).map(|l| l.label.clone());
    let words: Vec<Value> = a.words.iter().map(|m| json!({ "start": m.start, "end": m.end, "text": m.text, "speaker": label(m.speaker) })).collect();
    let turns: Vec<Value> = a.turns.iter().map(|t| json!({ "start": t.start, "end": t.end, "speaker": t.speaker })).collect();
    let result = json!({
        "audio": media,
        "vocabulary": lexicon_md,
        "transcription_seconds": transcription_seconds,
        "diarization_seconds": diarization_seconds,
        "raw": raw,
        "words": words,
        "turns": turns,
    });
    std::fs::write(&output, serde_json::to_string(&result).expect("json")).unwrap_or_else(|e| panic!("{}: {e}", output.display()));
    eprintln!("{}: {} words, {} speakers, transcription {:.1} s, diarization {:.1} s", media.display(), a.words.len(), a.speakers.len(), transcription_seconds, diarization_seconds);
}
