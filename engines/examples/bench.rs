//! Le moteur du banc de parité (`scripts/banc-parite`) : fait passer un média par les vrais
//! moteurs de Greffier, comme l'étape Transcription du cœur (whisper.cpp avec le vocabulaire du
//! lexique, pyannote, puis `attribution::attribuer(sans_les_bruits(mots), tours)`), et écrit le
//! résultat en JSON. Les deux moteurs tournent l'un après l'autre pour que leurs durées se
//! comparent à celles du vault.
//!
//!     cargo run --release -p greffier-moteurs --example banc -- <média> <sortie.json> \
//!         [--lexique lexique.md] [--locuteurs N] [--wav audio.wav]
//!
//! - `--lexique` : le lexique dont la section Vocabulaire est donnée à whisper (par défaut celui du
//!   dossier de l'app, comme Greffier).
//! - `--wav` : écrit aussi la piste de mixage décodée (PCM mono 16 kHz), pour la comparer à celle
//!   de ffmpeg.
//!
//! Autre usage, `banc --decouper <média> <début> <fin> <sortie.wav>` : écrit la piste de mixage
//! décodée par AVFoundation entre deux instants (en secondes), comme
//! `ffmpeg -ss <début> -to <fin> -i <média> -vn -ac 1 -ar 16000 -c:a pcm_s16le` (le WAV du banc
//! du vault).
//!
//! Sortie : `{ audio, vocabulaire, secondes_transcription, secondes_separation, brut: [mots de
//! whisper], words: [{start, end, text, locuteur}], tours: [{start, end, locuteur}] }`. Seuls le
//! fichier de sortie, le WAV demandé et le WAV temporaire de pyannote (dans le dossier de l'app)
//! sont écrits.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use greffier_core::adapters::{DiarizationRequest, TranscriptionRequest};
use greffier_core::{attribution, lexicon, resources};
use greffier_engines::{PyannoteDiarizer, WhisperTranscriber, audio};
use serde_json::{Value, json};

fn slice(args: &[String]) {
    let [media, start, end, output] = args else { panic!("usage : banc --decouper <média> <début> <fin> <sortie.wav>") };
    let instant = |s: &String| (s.parse::<f64>().expect("instant en secondes") * audio::SAMPLE_RATE as f64).round() as usize;
    let samples = audio::mixing(std::path::Path::new(media), &AtomicBool::new(false), |_| {}).unwrap_or_else(|e| panic!("audio : {e}"));
    let (from, a) = (instant(start).min(samples.len()), instant(end).min(samples.len()));
    audio::write_wav(std::path::Path::new(output), &samples[from..a]).unwrap_or_else(|e| panic!("{output} : {e}"));
}

fn main() {
    let all: Vec<String> = std::env::args().skip(1).collect();
    if all.first().map(String::as_str) == Some("--decouper") {
        return slice(&all[1..]);
    }
    let mut args = all.into_iter();
    let mut positional = Vec::new();
    let (mut lexicon_md, mut number, mut wav) = (None, None, None);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--lexique" => lexicon_md = Some(PathBuf::from(args.next().expect("--lexique <fichier>"))),
            "--locuteurs" => number = Some(args.next().and_then(|n| n.parse().ok()).expect("--locuteurs <nombre>")),
            "--wav" => wav = Some(PathBuf::from(args.next().expect("--wav <fichier>"))),
            _ => positional.push(PathBuf::from(a)),
        }
    }
    let [media, output] = <[PathBuf; 2]>::try_from(positional).expect("usage : banc <média> <sortie.json> [--lexique f] [--locuteurs N] [--wav f]");
    let app = resources::app_folder();
    let lexicon_md = lexicon_md.unwrap_or_else(|| app.join("lexique.md"));
    let vocabulary = std::fs::read_to_string(&lexicon_md).map(|md| lexicon::vocabulary(&md)).unwrap_or_default();
    let stop = AtomicBool::new(false);

    if let Some(wav) = &wav {
        let samples = audio::mixing(&media, &stop, |_| {}).unwrap_or_else(|e| panic!("audio : {e}"));
        audio::write_wav(wav, &samples).unwrap_or_else(|e| panic!("{} : {e}", wav.display()));
    }

    let t = Instant::now();
    let request = TranscriptionRequest { audio: media.clone(), duration: 0.0, vocabulary: vocabulary.clone() };
    let words = WhisperTranscriber::from_app(&app).transcribe_here(&request, &stop, &mut |_| {}).unwrap_or_else(|e| panic!("transcription : {e}"));
    let transcription_seconds = t.elapsed().as_secs_f64();

    let t = Instant::now();
    let request = DiarizationRequest { audio: media.clone(), duration: 0.0, speaker_count: number };
    let turns = PyannoteDiarizer::from_app(&app).diarize_here(&request, &stop, &mut |_| {}).unwrap_or_else(|e| panic!("séparation : {e}"));
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
    std::fs::write(&output, serde_json::to_string(&result).expect("json")).unwrap_or_else(|e| panic!("{} : {e}", output.display()));
    eprintln!("{} : {} mots, {} locuteurs, transcription {:.1} s, séparation {:.1} s", media.display(), a.words.len(), a.speakers.len(), transcription_seconds, diarization_seconds);
}
