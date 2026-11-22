//! Imports a medium into a meetings folder and has the core transcribe it with the real engines
//! (whisper.cpp and pyannote, in parallel), without a window: the hands-on, end-to-end check of
//! the Transcription stage. Stops once the transcription is written.
//!
//!     cargo run --release -p greffier-engines --example process -- <meetings folder> <media> [--speakers N]
//!
//! Then `GREFFIER_FOLDER=<meetings folder> cargo run` to see the meeting in the app.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use greffier_core::adapters::Adapters;
use greffier_core::folder::Import;
use greffier_core::fake::FakeResources;
use greffier_core::{Command, DateTime, Stage, Event, Greffier, Status};
use greffier_engines::{PyannoteDiarizer, WhisperTranscriber};

fn main() {
    let mut args = std::env::args().skip(1);
    let mut positional = Vec::new();
    let mut number = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--speakers" => number = Some(args.next().and_then(|n| n.parse().ok()).expect("--speakers <number>")),
            _ => positional.push(PathBuf::from(a)),
        }
    }
    let [folder, media] = <[PathBuf; 2]>::try_from(positional).expect("usage: process <meetings folder> <media> [--speakers N]");
    std::fs::create_dir_all(&folder).expect("meetings folder");
    let app = greffier_core::resources::app_folder();
    let adapters = Adapters {
        transcriber: Box::new(WhisperTranscriber::from_app(&app)),
        diarizer: Box::new(PyannoteDiarizer::from_app(&app)),
        resources: Box::new(FakeResources::ready()),
        ..greffier_core::demo::adapters()
    };
    let mut greffier = Greffier::open(&folder, adapters);
    let title = media.file_stem().map_or("Réunion".into(), |s| s.to_string_lossy().into_owned());
    let import = Import { recording: media, transcription: None, summary: None, start: DateTime::new(2026, 11, 21, 10, 0), title };
    let ev = greffier.execute(Command::Import(import)).expect("import");
    let id = ev.iter().find_map(|e| if let Event::MeetingImported(id) = e { Some(*id) } else { None }).expect("meeting imported");
    greffier.execute(Command::SpeakerCount { meeting: id, number }).expect("speaker count");
    greffier.execute(Command::Process(id)).expect("process");

    let t = Instant::now();
    let mut last = -1;
    loop {
        greffier.advance(0.1);
        let r = greffier.meetings().iter().find(|r| r.id == id).expect("meeting");
        match &r.status {
            Status::Running { stage: Stage::Transcription, progress } => {
                let pc = (progress * 100.0) as i32;
                if pc / 10 != last / 10 {
                    eprintln!("  {pc} %");
                    last = pc;
                }
            }
            Status::Failed { message, .. } if r.completed_stage.is_none() => panic!("transcription: {message}"),
            _ if r.completed_stage.is_some() => break,
            _ => {}
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let r = greffier.meetings().iter().find(|r| r.id == id).expect("meeting");
    eprintln!("transcription: {} words, {} speakers, {} turns, {} utterances in {:.1} s → {}", r.words.len(), r.speakers.len(), r.turns.len(), r.utterances.len(), t.elapsed().as_secs_f64(), r.folder_name);
    for x in &r.utterances {
        println!("[{:7.2}] {}: {}", x.start, r.speakers[x.speaker].label, x.text);
    }
    // The review that follows is not what this check is about: once stopped, the meeting stays
    // "to process" after its transcription.
    let _ = greffier.execute(Command::StopProcessing(id));
}
