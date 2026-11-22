//! Diarizes a medium with pyannote in the app folder's private Python, without a window: the
//! hands-on check of the pyannote adapter.
//!
//!     cargo run --release -p greffier-engines --example diarize -- <media> [--speakers N]
//!
//! Prints the speaker turns (start, end, speaker) then the duration.

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
            "--speakers" => speaker_count = Some(args.next().and_then(|n| n.parse().ok()).expect("--speakers <number>")),
            _ => media = Some(PathBuf::from(a)),
        }
    }
    let media = media.expect("usage: diarize <media> [--speakers N]");
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
        .unwrap_or_else(|e| panic!("diarization: {e}"));
    let mut speakers: Vec<&str> = turns.iter().map(|t| t.speaker.as_str()).collect();
    speakers.sort_unstable();
    speakers.dedup();
    eprintln!("diarization: {} speakers, {} speaker turns in {:.1} s", speakers.len(), turns.len(), t.elapsed().as_secs_f64());
    for t in &turns {
        println!("{:8.2} {:8.2} {}", t.start, t.end, t.speaker);
    }
}
