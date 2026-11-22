//! Sépare les locuteurs d'un média avec pyannote dans le Python privé du dossier de l'app, sans
//! fenêtre : la vérification à la main de l'adaptateur pyannote.
//!
//!     cargo run --release -p greffier-moteurs --example separer -- <média> [--locuteurs N]
//!
//! Affiche les tours de parole (début, fin, locuteur) puis la durée.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use greffier_core::adapters::DiarizationRequest;
use greffier_engines::PyannoteDiarizer;

fn main() {
    let mut args = std::env::args().skip(1);
    let mut media = None;
    let mut speaker_count = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--locuteurs" => speaker_count = Some(args.next().and_then(|n| n.parse().ok()).expect("--locuteurs <nombre>")),
            _ => media = Some(PathBuf::from(a)),
        }
    }
    let media = media.expect("usage : separer <média> [--locuteurs N]");
    let engine = PyannoteDiarizer::from_app(&greffier_core::resources::app_folder());
    let request = DiarizationRequest { audio: media, duration: 0.0, speaker_count };
    let t = Instant::now();
    let mut last = -1;
    let turns = engine
        .diarize_here(&request, &AtomicBool::new(false), &mut |p| {
            let pc = (p * 100.0) as i32;
            if pc / 10 != last / 10 {
                eprintln!("  {pc} %");
                last = pc;
            }
        })
        .unwrap_or_else(|e| panic!("séparation : {e}"));
    let mut speakers: Vec<&str> = turns.iter().map(|t| t.speaker.as_str()).collect();
    speakers.sort_unstable();
    speakers.dedup();
    eprintln!("séparation : {} locuteurs, {} tours de parole en {:.1} s", speakers.len(), turns.len(), t.elapsed().as_secs_f64());
    for t in &turns {
        println!("{:8.2} {:8.2} {}", t.start, t.end, t.speaker);
    }
}
