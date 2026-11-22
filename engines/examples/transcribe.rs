//! Transcrit un média avec les vrais moteurs et les modèles du dossier de l'app, sans fenêtre :
//! la vérification à la main de l'adaptateur whisper.cpp.
//!
//!     cargo run --release -p greffier-moteurs --example transcrire -- <média> [--vocabulaire "…"] [--wav sortie.wav]
//!
//! Affiche les mots (début, fin, confiance) puis la durée de chaque partie. `--wav` écrit l'audio
//! converti par AVFoundation, pour le comparer à celui de ffmpeg.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use greffier_core::adapters::TranscriptionRequest;
use greffier_engines::{WhisperTranscriber, audio};

fn main() {
    let mut args = std::env::args().skip(1);
    let mut media = None;
    let mut vocabulary = String::new();
    let mut wav = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--vocabulaire" => vocabulary = args.next().expect("--vocabulaire <phrase>"),
            "--wav" => wav = Some(PathBuf::from(args.next().expect("--wav <fichier>"))),
            _ => media = Some(PathBuf::from(a)),
        }
    }
    let media = media.expect("usage : transcrire <média> [--vocabulaire \"…\"] [--wav sortie.wav]");
    let stop = AtomicBool::new(false);

    let t = Instant::now();
    let samples = audio::mixing(&media, &stop, |_| {}).unwrap_or_else(|e| panic!("audio : {e}"));
    eprintln!("audio : {:.2} s de son en {:.2} s", samples.len() as f64 / audio::SAMPLE_RATE as f64, t.elapsed().as_secs_f64());
    if let Some(w) = wav {
        audio::write_wav(&w, &samples).expect("wav");
    }

    let engine = WhisperTranscriber::from_app(&greffier_core::resources::app_folder());
    let request = TranscriptionRequest { audio: media, duration: 0.0, vocabulary };
    let t = Instant::now();
    let mut last = -1;
    let words = engine
        .transcribe_here(&request, &stop, &mut |p| {
            let pc = (p * 100.0) as i32;
            if pc / 10 != last / 10 {
                eprintln!("  {pc} %");
                last = pc;
            }
        })
        .unwrap_or_else(|e| panic!("transcription : {e}"));
    eprintln!("transcription : {} mots en {:.1} s", words.len(), t.elapsed().as_secs_f64());
    for m in &words {
        println!("{:8.2} {:8.2} {:.3} {}", m.start, m.end, m.confidence, m.text);
    }
}
