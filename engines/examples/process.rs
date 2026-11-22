//! Importe un média dans un dossier de réunions et le fait transcrire par le cœur avec les vrais
//! moteurs (whisper.cpp et pyannote, en parallèle), sans fenêtre : la vérification à la main de
//! l'étape Transcription de bout en bout. S'arrête quand la transcription est écrite.
//!
//!     cargo run --release -p greffier-moteurs --example traiter -- <dossier de réunions> <média> [--locuteurs N]
//!
//! Puis `GREFFIER_DOSSIER=<dossier de réunions> cargo run` pour voir la réunion dans l'app.

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
            "--locuteurs" => number = Some(args.next().and_then(|n| n.parse().ok()).expect("--locuteurs <nombre>")),
            _ => positional.push(PathBuf::from(a)),
        }
    }
    let [folder, media] = <[PathBuf; 2]>::try_from(positional).expect("usage : traiter <dossier de réunions> <média> [--locuteurs N]");
    std::fs::create_dir_all(&folder).expect("dossier de réunions");
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
    let id = ev.iter().find_map(|e| if let Event::MeetingImported(id) = e { Some(*id) } else { None }).expect("réunion importée");
    greffier.execute(Command::SpeakerCount { meeting: id, number }).expect("nombre de locuteurs");
    greffier.execute(Command::Process(id)).expect("traiter");

    let t = Instant::now();
    let mut last = -1;
    loop {
        greffier.advance(0.1);
        let r = greffier.meetings().iter().find(|r| r.id == id).expect("réunion");
        match &r.status {
            Status::Running { stage: Stage::Transcription, progress } => {
                let pc = (progress * 100.0) as i32;
                if pc / 10 != last / 10 {
                    eprintln!("  {pc} %");
                    last = pc;
                }
            }
            Status::Failed { message, .. } if r.completed_stage.is_none() => panic!("transcription : {message}"),
            _ if r.completed_stage.is_some() => break,
            _ => {}
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let r = greffier.meetings().iter().find(|r| r.id == id).expect("réunion");
    eprintln!("transcription : {} mots, {} locuteurs, {} tours, {} répliques en {:.1} s → {}", r.words.len(), r.speakers.len(), r.turns.len(), r.utterances.len(), t.elapsed().as_secs_f64(), r.folder_name);
    for x in &r.utterances {
        println!("[{:7.2}] {} : {}", x.start, r.speakers[x.speaker].label, x.text);
    }
    // La relecture qui suit n'est pas l'objet de cet essai : arrêtée, la réunion reste « à traiter »
    // après sa transcription.
    let _ = greffier.execute(Command::StopProcessing(id));
}
