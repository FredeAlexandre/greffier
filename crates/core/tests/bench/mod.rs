//! The core's test bench: a meetings folder in a temporary directory, fake adapters fed with
//! hand-written data, commands, then we look at the state and the files. No test depends on the
//! core's internal structure.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use greffier_core::adapters::{Adapters, SummarySpeaker, ReviewResult, SummaryResult};
use greffier_core::fake::{FakeCapture, FakeTrash, FakeClock, FakeResources, FakeAgent, FakeDiarizer, FakeTranscriber};
use greffier_core::{Command, DateTime, Stage, Event, Greffier, MeetingId, Word, Meeting, Status, SpeakerTurn};

/// Simulated duration of each stage of the fake adapters, in seconds.
pub const STAGE_DURATION: f64 = 1.0;

pub const SUMMARY: &str = "## En bref\n\nAlexandre salue l'équipe.\n\n## Participants\n\n- **SPEAKER_00 — Alexandre**\n- **SPEAKER_01 — Khalid**\n\n## Sujets abordés\n\n### Salutations (0:00:00)\n\n- Bonjour.\n\n## Décisions\n\n- (0:00:03) On se revoit demain.\n\n## Actions\n\n- [ ] **Alexandre** — préparer la démo — lundi\n- [ ] **Khalid** — relire la PR\n\n## Questions ouvertes\n\n- Aucune.\n";

pub fn word(start: f64, end: f64, text: &str) -> Word {
    Word { start, end, text: text.into(), confidence: 0.9, speaker: None }
}

pub fn turn(start: f64, end: f64, speaker: &str) -> SpeakerTurn {
    SpeakerTurn { start, end, speaker: speaker.into() }
}

/// Two utterances: SPEAKER_00 says hello, SPEAKER_01 answers.
pub fn words() -> Vec<Word> {
    vec![word(0.0, 0.5, "Bonjour"), word(0.5, 0.8, "à"), word(0.8, 1.5, "tous."), word(3.0, 3.4, "Salut"), word(3.4, 4.0, "Alexandre.")]
}

pub fn turns() -> Vec<SpeakerTurn> {
    vec![turn(0.0, 1.6, "SPEAKER_00"), turn(2.9, 4.1, "SPEAKER_01")]
}

pub fn summary_result() -> SummaryResult {
    SummaryResult {
        session: Some("session-bilan".into()),
        title: "Point d'équipe : démo".into(),
        brief: "Alexandre salue l'équipe.".into(),
        actions: vec!["Alexandre — préparer la démo — lundi".into(), "Khalid — relire la PR".into()],
        speakers: vec![
            SummarySpeaker { speaker: "SPEAKER_00".into(), proposed_name: "Alexandre".into(), justification: "Khalid l'appelle Alexandre à 0:00:03.".into(), same_voice_as: None },
            SummarySpeaker { speaker: "SPEAKER_01".into(), proposed_name: "Khalid".into(), justification: "Répond à Alexandre.".into(), same_voice_as: None },
        ],
    }
}

/// An agent that corrects nothing and writes the given summary.
pub fn agent(summary: &'static str) -> FakeAgent {
    FakeAgent::new(
        STAGE_DURATION,
        |_| Ok(ReviewResult::default()),
        STAGE_DURATION,
        move |_| Ok((summary.to_string(), summary_result())),
    )
}

pub fn adapters() -> Adapters {
    Adapters {
        capture: Box::new(FakeCapture::new(1.0, 0.0)),
        transcriber: Box::new(FakeTranscriber::fixed(STAGE_DURATION, words())),
        diarizer: Box::new(FakeDiarizer::fixed(STAGE_DURATION, turns())),
        agent: Box::new(agent(SUMMARY)),
        resources: Box::new(FakeResources::ready()),
        clock: Box::new(FakeClock(DateTime::new(2026, 11, 21, 14, 2))),
        trash: Box::new(FakeTrash::new()),
        mp4_muxing: Box::new(greffier_core::fake::FakeMp4Muxer),
    }
}

pub struct Bench {
    pub greffier: Greffier,
    pub folder: PathBuf,
}

impl Bench {
    pub fn new() -> Bench {
        Bench::with(adapters())
    }

    pub fn with(adapters: Adapters) -> Bench {
        let folder = temp_folder();
        let greffier = Greffier::open(&folder, adapters);
        Bench { greffier, folder }
    }

    /// The core on a copy of the sample meetings folder (`tests/fixtures/meetings-folder`).
    pub fn on_fixtures() -> Bench {
        let folder = temp_folder();
        copy_to(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/meetings-folder"), &folder);
        let greffier = Greffier::open(&folder, adapters());
        Bench { greffier, folder }
    }

    /// Restarts the core on the same meetings folder, as at the app's next launch.
    pub fn reopen(&mut self) {
        self.greffier = Greffier::open(&self.folder, adapters());
    }

    /// The meeting whose folder has this name.
    pub fn named_meeting(&self, name: &str) -> &Meeting {
        let names: Vec<&str> = self.greffier.meetings().iter().map(|r| r.folder_name.as_str()).collect();
        self.greffier.meetings().iter().find(|r| r.folder_name == name).unwrap_or_else(|| panic!("no meeting « {name} » among {names:?}"))
    }

    pub fn execute(&mut self, command: Command) -> Vec<Event> {
        let name = format!("{command:?}");
        self.greffier.execute(command).unwrap_or_else(|e| panic!("{name} rejected: {e}"))
    }

    /// Advances time in 0.1 s steps, as the UI would.
    pub fn advance(&mut self, seconds: f64) -> Vec<Event> {
        let mut events = Vec::new();
        let step = (seconds / 0.1).round() as usize;
        for _ in 0..step {
            events.extend(self.greffier.advance(0.1));
        }
        events
    }

    /// Advances until `end` is true (at most 60 s), noting the stages gone through.
    pub fn advance_until(&mut self, id: MeetingId, end: impl Fn(&Status) -> bool) -> Vec<Stage> {
        let mut stages = Vec::new();
        for _ in 0..600 {
            if let Status::Running { stage, .. } = self.meeting(id).status
                && stages.last() != Some(&stage)
            {
                stages.push(stage);
            }
            if end(&self.meeting(id).status) {
                return stages;
            }
            self.greffier.advance(0.1);
        }
        panic!("still {:?} after 60 s", self.meeting(id).status);
    }

    /// Records a meeting of `duration` seconds (fake capture in real time), without processing it.
    pub fn record(&mut self, duration: f64) -> MeetingId {
        self.execute(Command::StartRecording);
        self.advance(duration);
        self.finish_recording()
    }

    /// Stops the running recording; returns the recorded meeting.
    pub fn finish_recording(&mut self) -> MeetingId {
        let events = self.execute(Command::StopRecording);
        events
            .iter()
            .find_map(|e| match e {
                Event::MeetingRecorded(id) => Some(*id),
                _ => None,
            })
            .expect("the recorded meeting")
    }

    /// A meeting recorded and not yet processed.
    pub fn meeting_to_process(&mut self) -> MeetingId {
        self.execute(Command::ChangeSettings(greffier_core::Settings { process_on_stop: false, ..self.greffier.settings().clone() }));
        self.record(5.0)
    }

    pub fn meeting(&self, id: MeetingId) -> &Meeting {
        self.greffier.meeting(id).expect("known meeting")
    }

    /// The search cache next to the meetings folder (deleted with it), given to the core as the
    /// app does at launch with the one in its folder.
    pub fn enable_search_cache(&mut self) -> PathBuf {
        let cache = self.folder.with_extension("search");
        self.execute(Command::ChangeSettings(greffier_core::Settings { search_cache: Some(cache.clone()), ..self.greffier.settings().clone() }));
        cache
    }

    pub fn read(&self, path: &Path) -> String {
        std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    }
}

fn temp_folder() -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let folder = std::env::temp_dir().join(format!("greffier-bench-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).expect("temporary folder");
    folder
}

fn copy_to(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("copying fixtures");
    for input in std::fs::read_dir(from).expect("fixtures") {
        let input = input.expect("fixture");
        let target = to.join(input.file_name());
        if input.file_type().expect("type").is_dir() {
            copy_to(&input.path(), &target);
        } else {
            std::fs::copy(input.path(), target).expect("copying a fixture");
        }
    }
}

impl Drop for Bench {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.folder);
        let _ = std::fs::remove_dir_all(self.folder.with_extension("search"));
    }
}
