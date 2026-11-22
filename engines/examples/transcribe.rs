//! Transcribes a medium with the real engines and the app folder's models, without a window: the
//! hands-on check of the whisper.cpp adapter.
//!
//!     cargo run --release -p greffier-engines --example transcribe -- <media> [--vocabulary "…"] [--wav output.wav]
//!
//! Prints the words (start, end, confidence) then the duration of each part. `--wav` writes the
//! audio converted by AVFoundation, to compare it with ffmpeg's.

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
            "--vocabulary" => vocabulary = args.next().expect("--vocabulary <sentence>"),
            "--wav" => wav = Some(PathBuf::from(args.next().expect("--wav <file>"))),
            _ => media = Some(PathBuf::from(a)),
        }
    }
    let media = media.expect("usage: transcribe <media> [--vocabulary \"…\"] [--wav output.wav]");
    let stop = AtomicBool::new(false);

    let t = Instant::now();
    let samples = audio::mixing(&media, &stop, |_| {}).unwrap_or_else(|e| panic!("audio: {e}"));
    eprintln!("audio: {:.2} s of sound in {:.2} s", samples.len() as f64 / audio::SAMPLE_RATE as f64, t.elapsed().as_secs_f64());
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
        .unwrap_or_else(|e| panic!("transcription: {e}"));
    eprintln!("transcription: {} words in {:.1} s", words.len(), t.elapsed().as_secs_f64());
    for m in &words {
        println!("{:8.2} {:8.2} {:.3} {}", m.start, m.end, m.confidence, m.text);
    }
}
