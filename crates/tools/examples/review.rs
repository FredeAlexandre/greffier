//! Manual trial of the review by the real `claude` CLI, on a small made-up transcription, without
//! relisten (the excerpts return "nothing usable").
//!
//! `cargo run -p greffier-tools --example review [-- <working directory>]`

use std::path::Path;
use std::sync::atomic::AtomicBool;

use greffier_core::DateTime;
use greffier_core::adapters::{ReviewRequest, AgentUtterance};
use greffier_tools::claude::MacClaude;
use greffier_tools::claude::review::{self, Relisten};

struct NoRelisten;

impl Relisten for NoRelisten {
    fn relisten(&self, _: &Path, excerpts: &[(f64, f64)], _: &str, _: &AtomicBool) -> Result<Vec<String>, String> {
        eprintln!("relisten requested: {excerpts:?}");
        Ok(vec![String::new(); excerpts.len()])
    }
}

fn main() {
    let directory = std::env::args().nth(1).map(Into::into).unwrap_or_else(std::env::temp_dir);
    let r = |number, speaker: &str, start, end, text: &str| AgentUtterance { number, speaker: speaker.into(), name: None, start, end, text: text.into() };
    let request = ReviewRequest {
        recording: "/nowhere.mp4".into(),
        meetings_folder: directory,
        folder_name: "2026-11-21 14h02 - Point d'équipe".into(),
        title: "Point d'équipe".into(),
        start: DateTime::new(2026, 11, 21, 14, 2),
        speaker_count: Some(2),
        utterances: vec![
            r(0, "SPEAKER_00", 0.0, 4.0, "Bonjour Bob, on fait le point sur la mise en prod de Zorglube ?"),
            r(1, "SPEAKER_01", 4.5, 9.0, "Oui, la recète est terminée, il reste le ticket sur le cache redisse."),
            r(2, "SPEAKER_01", 9.5, 12.0, "Et le, euh, le flux de paie mangue chez le client ?"),
            r(3, "SPEAKER_01", 12.5, 15.0, "Il est bloqué, on attend leur retour jeudi."),
        ],
        lexicon: greffier_core::lexicon::INITIAL_LEXICON.into(),
        vocabulary: String::new(),
    };
    let mut progress = |p: f64| eprintln!("milestone {p}");
    match review::review(&MacClaude::default(), &NoRelisten, &request, &AtomicBool::new(false), &mut progress) {
        Ok(result) => println!("{result:#?}"),
        Err(e) => {
            eprintln!("failed: {e}");
            std::process::exit(1);
        }
    }
}
