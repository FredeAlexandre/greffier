//! Manual trial of the real summary by the `claude` CLI (opus model, a few minutes, paid) on a
//! made-up meeting: fake recording, hand-written transcription, a review that changes nothing,
//! then the real summary. The given meetings folder is created if missing.
//!
//! `cargo run -p greffier-tools --example summary -- <meetings folder> [<context folder>…]`

use std::path::PathBuf;
use std::time::{Duration, Instant};

use greffier_core::adapters::*;
use greffier_tools::claude::{AgentClaude, MacClaude, review::Relisten};
use greffier_core::fake::{FakeCapture, FakeClock, FakeTask, FakeResources, FakeDiarizer, FakeTranscriber};
use greffier_core::{Command, DateTime, Event, Greffier, Word, Settings, Status, SpeakerTurn};

/// The real summary, with a review that changes nothing.
struct SummaryOnly(AgentClaude);

/// No relisten: the review is not run here.
struct NoRelisten;

impl Relisten for NoRelisten {
    fn relisten(&self, _: &std::path::Path, excerpts: &[(f64, f64)], _: &str, _: &std::sync::atomic::AtomicBool) -> Result<Vec<String>, String> {
        Ok(vec![String::new(); excerpts.len()])
    }
}

impl Agent for SummaryOnly {
    fn review(&mut self, _: ReviewRequest) -> BoxedTask<ReviewResult> {
        FakeTask::boxed(0.0, Ok(ReviewResult::default()))
    }
    fn summarize(&mut self, d: SummaryRequest) -> BoxedTask<SummaryResult> {
        self.0.summarize(d)
    }
}

const UTTERANCES: [(&str, &str); 8] = [
    ("SPEAKER_00", "Bonjour Bastien, on fait le point sur le salon de mars ?"),
    ("SPEAKER_01", "Oui Odile. Le stand est réservé, il reste à choisir le traiteur."),
    ("SPEAKER_00", "On avait deux devis, Les Halles Gourmandes à 2 400 euros et Saveurs du Port à 3 100."),
    ("SPEAKER_01", "Je propose Les Halles Gourmandes, la qualité était bonne l'an dernier."),
    ("SPEAKER_00", "D'accord, on prend Les Halles Gourmandes. Tu leur confirmes avant vendredi ?"),
    ("SPEAKER_01", "Oui, je les appelle jeudi. Par contre, pour les badges, on ne sait pas encore combien de visiteurs."),
    ("SPEAKER_00", "Je demande les chiffres à l'organisateur lundi et je te les envoie."),
    ("SPEAKER_01", "Parfait. Et on garde la date du 14 mars pour la répétition."),
];

fn main() {
    let mut args = std::env::args_os().skip(1).map(PathBuf::from);
    let Some(folder) = args.next() else {
        eprintln!("usage: summary <meetings folder> [<context folder>…]");
        std::process::exit(2);
    };
    let context: Vec<PathBuf> = args.collect();
    std::fs::create_dir_all(&folder).expect("meetings folder");

    let (mut words, mut turns) = (Vec::<Word>::new(), Vec::<SpeakerTurn>::new());
    let mut t = 0.0;
    for (speaker, text) in UTTERANCES {
        let start = t;
        for m in text.split(' ') {
            words.push(Word { start: t, end: t + 0.35, text: m.into(), confidence: 0.9, speaker: None });
            t += 0.4;
        }
        turns.push(SpeakerTurn { start, end: t, speaker: speaker.into() });
        t += 1.0;
    }
    let duration = t;
    let adapters = Adapters {
        capture: Box::new(FakeCapture::new(20.0, 0.0)),
        transcriber: Box::new(FakeTranscriber::fixed(0.1, words)),
        diarizer: Box::new(FakeDiarizer::fixed(0.1, turns)),
        agent: Box::new(SummaryOnly(AgentClaude::new(std::sync::Arc::new(MacClaude::default()), std::sync::Arc::new(NoRelisten)))),
        resources: Box::new(FakeResources::ready()),
        clock: Box::new(FakeClock(DateTime::new(2026, 11, 21, 16, 30))),
        trash: Box::new(greffier_core::fake::FakeTrash::new()),
        mp4_muxing: Box::new(greffier_core::fake::FakeMp4Muxer),
    };
    let mut g = Greffier::open(&folder, adapters);
    g.execute(Command::ChangeSettings(Settings { context_folders: context, ..Settings::default() })).expect("settings");
    g.execute(Command::StartRecording).expect("recording");
    while g.recording().is_some_and(|e| e.duration < duration) {
        g.advance(0.1);
    }
    let id = g.execute(Command::StopRecording).expect("stop").iter().find_map(|e| if let Event::MeetingRecorded(id) = e { Some(*id) } else { None }).expect("meeting");

    let start = Instant::now();
    loop {
        for e in g.advance(0.1) {
            println!("{e:?}");
        }
        let r = g.meeting(id).expect("meeting");
        match &r.status {
            Status::Done => break,
            Status::Failed { message, .. } => {
                eprintln!("failed: {message}");
                std::process::exit(1);
            }
            _ => std::thread::sleep(Duration::from_millis(100)),
        }
    }
    let r = g.meeting(id).expect("meeting");
    println!("\n{} s · folder: {}", start.elapsed().as_secs(), g.folder().folder(&r.folder_name).display());
    println!("title: {}\nbrief: {}\nactions: {:?}\nsession: {:?}", r.title, r.brief, r.key_actions, r.sessions.summary);
}
