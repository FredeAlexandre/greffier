//! Smoke test of the real agent on this Mac, run on purpose with `cargo xtask smoke --with-claude`
//! (`#[ignore]`d by `cargo test`; it costs Claude tokens and takes about a minute): the core, on
//! fake capture and engines, sends a tiny meeting through the review then the summary by the
//! `claude` CLI. Skipped when the CLI is missing or not signed in. Only the contract is checked
//! (structured answers, the summary file and what the app reads from it), never the wording.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use greffier_core::adapters::Adapters;
use greffier_core::fake::{FakeCapture, FakeClock, FakeDiarizer, FakeMp4Muxer, FakeResources, FakeTranscriber, FakeTrash};
use greffier_core::folder::Role;
use greffier_core::resources::{ClaudeState, Tools};
use greffier_core::{Command, DateTime, Event, Greffier, SpeakerTurn, Status, Word, summary};
use greffier_tools::claude::review::Relisten;
use greffier_tools::claude::{AgentClaude, MacClaude};
use greffier_tools::mac::MacTools;

/// A short meeting between two people, with a decision, actions and a homophone to correct.
const UTTERANCES: [(&str, &str); 6] = [
    ("SPEAKER_00", "Bonjour Bastien, on fait le point sur le salon de mars ?"),
    ("SPEAKER_01", "Oui Odile. Le stand est réservé, il reste à choisir le traiteur."),
    ("SPEAKER_00", "On avait deux devis, Les Halles Gourmandes à 2 400 euros et Saveurs du Port à 3 100."),
    ("SPEAKER_01", "Je propose Les Halles Gourmandes, la qualité était bonne l'an dernier."),
    ("SPEAKER_00", "D'accord, on prend Les Halles Gourmandes. Tu leur confirmes avant vendredi ?"),
    ("SPEAKER_01", "Oui, je les appelle jeudi. Et je fais la recète des badges lundi."),
];

/// No relisten: there is no real recording to listen to again.
struct NoRelisten;

impl Relisten for NoRelisten {
    fn relisten(&self, _: &Path, excerpts: &[(f64, f64)], _: &str, _: &AtomicBool) -> Result<Vec<String>, String> {
        Ok(vec![String::new(); excerpts.len()])
    }
}

#[test]
#[ignore = "real Mac, costs Claude tokens: review and summary by the claude CLI (cargo xtask smoke --with-claude)"]
fn claude_reviews_then_writes_the_summary_of_a_tiny_meeting() {
    match claude_state() {
        ClaudeState::Connected { version, account } => eprintln!("claude {version} ({account})"),
        ClaudeState::Missing => return skip("the claude CLI is not installed"),
        ClaudeState::NotConnected { .. } => return skip("the claude CLI is not signed in (run `claude` in a terminal)"),
        ClaudeState::Failed(e) => panic!("checking the claude CLI: {e}"),
    }

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
    let length = t;
    // A recording shorter than a minute is a false start, which the core does not process.
    let recorded = length.max(greffier_core::MIN_DURATION + 1.0);

    let folder = std::env::temp_dir().join(format!("greffier-smoke-claude-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).expect("meetings folder");
    let adapters = Adapters {
        capture: Box::new(FakeCapture::new(20.0, 0.0)),
        transcriber: Box::new(FakeTranscriber::fixed(0.1, words)),
        diarizer: Box::new(FakeDiarizer::fixed(0.1, turns)),
        agent: Box::new(AgentClaude::new(Arc::new(MacClaude::default()), Arc::new(NoRelisten))),
        resources: Box::new(FakeResources::ready()),
        clock: Box::new(FakeClock(DateTime::new(2026, 11, 21, 16, 30))),
        trash: Box::new(FakeTrash::new()),
        mp4_muxing: Box::new(FakeMp4Muxer),
    };
    let mut g = Greffier::open(&folder, adapters);
    g.execute(Command::StartRecording).expect("recording");
    while g.recording().is_some_and(|r| r.duration < recorded) {
        g.advance(0.1);
    }
    let stopped = g.execute(Command::StopRecording).expect("stop");
    let id = stopped.iter().find_map(|e| if let Event::MeetingRecorded(id) = e { Some(*id) } else { None }).expect("meeting recorded");

    // Transcription (fake), review then summary (claude): the meeting ends up done.
    let start = Instant::now();
    loop {
        g.advance(0.1);
        let r = g.meeting(id).expect("meeting");
        match &r.status {
            Status::Done => break,
            Status::Failed { message, .. } => panic!("the processing failed: {message}"),
            Status::ToProcess if start.elapsed() > Duration::from_secs(5) => panic!("the meeting was not queued for processing"),
            _ => {}
        }
        assert!(start.elapsed() < Duration::from_secs(20 * 60), "still {:?} after 20 min", r.status);
        std::thread::sleep(Duration::from_millis(100));
    }
    eprintln!("review and summary in {:.0} s", start.elapsed().as_secs_f64());

    let r = g.meeting(id).expect("meeting");
    assert!(r.sessions.review.is_some(), "the review's structured answer came back with its session");
    assert!(r.sessions.summary.is_some(), "the summary's structured answer came back with its session");
    assert!(!r.title.trim().is_empty() && !r.brief.trim().is_empty(), "title « {} », brief « {} »", r.title, r.brief);
    assert!(r.key_actions.len() <= 3, "{:?}: 3 key actions at most", r.key_actions);

    // The summary file, written by the agent, holds what the app reads from it.
    let file = g.folder().file(&r.folder_name, Role::Summary);
    let md = std::fs::read_to_string(&file).unwrap_or_else(|e| panic!("{}: {e}", file.display()));
    eprintln!("{}:\n{md}", file.display());
    assert_eq!(summary::missing_parts(&md), None, "the timeline needs timestamped topics");
    assert!(summary::topics(&md).iter().all(|t| (0.0..=recorded).contains(&t.start)), "topics outside the {recorded:.0} s meeting: {:?}", summary::topics(&md));
    assert!(!summary::brief(&md).is_empty(), "no « En bref » paragraph");
    let _ = std::fs::remove_dir_all(&folder);
}

/// Whether the `claude` CLI is installed and signed in, as the first launch checks it.
fn claude_state() -> ClaudeState {
    let mut task = MacTools::new().check_claude();
    loop {
        match task.poll(0.1) {
            greffier_core::adapters::TaskProgress::Done(state) => return state,
            greffier_core::adapters::TaskProgress::Failed(e) => return ClaudeState::Failed(e),
            greffier_core::adapters::TaskProgress::Running(_) => std::thread::sleep(Duration::from_millis(100)),
        }
    }
}

/// Says why the test checks nothing on this Mac (`cargo xtask smoke` lists these lines).
fn skip(why: &str) {
    eprintln!("SKIPPED: {why}");
}
