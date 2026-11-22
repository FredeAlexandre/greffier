//! Le banc de test du cœur : un dossier de réunions dans un répertoire temporaire, des faux
//! adaptateurs nourris de données écrites à la main, des commandes, puis on regarde l'état et les
//! fichiers. Aucun test ne dépend de la structure interne du cœur.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use greffier_core::adapters::{Adapters, SummarySpeaker, ReviewResult, SummaryResult};
use greffier_core::fake::{FakeCapture, FakeTrash, FakeClock, FakeResources, FakeAgent, FakeDiarizer, FakeTranscriber};
use greffier_core::{Command, DateTime, Stage, Event, Greffier, MeetingId, Word, Meeting, Status, SpeakerTurn};

/// Durée simulée de chaque étape des faux adaptateurs, en secondes.
pub const STAGE_DURATION: f64 = 1.0;

pub const SUMMARY: &str = "## En bref\n\nAlexandre salue l'équipe.\n\n## Participants\n\n- **SPEAKER_00 — Alexandre**\n- **SPEAKER_01 — Khalid**\n\n## Sujets abordés\n\n### Salutations (0:00:00)\n\n- Bonjour.\n\n## Décisions\n\n- (0:00:03) On se revoit demain.\n\n## Actions\n\n- [ ] **Alexandre** — préparer la démo — lundi\n- [ ] **Khalid** — relire la PR\n\n## Questions ouvertes\n\n- Aucune.\n";

pub fn word(start: f64, end: f64, text: &str) -> Word {
    Word { start, end, text: text.into(), confidence: 0.9, speaker: None }
}

pub fn turn(start: f64, end: f64, speaker: &str) -> SpeakerTurn {
    SpeakerTurn { start, end, speaker: speaker.into() }
}

/// Deux répliques : SPEAKER_00 dit bonjour, SPEAKER_01 répond.
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

/// Un agent qui ne corrige rien et écrit le bilan donné.
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

    /// Le cœur sur une copie du dossier de réunions d'exemple (`tests/fixtures/meetings-folder`).
    pub fn on_fixtures() -> Bench {
        let folder = temp_folder();
        copy_to(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/meetings-folder"), &folder);
        let greffier = Greffier::open(&folder, adapters());
        Bench { greffier, folder }
    }

    /// Relance le cœur sur le même dossier de réunions, comme au lancement suivant de l'app.
    pub fn reopen(&mut self) {
        self.greffier = Greffier::open(&self.folder, adapters());
    }

    /// La réunion dont le dossier porte ce nom.
    pub fn named_meeting(&self, name: &str) -> &Meeting {
        let names: Vec<&str> = self.greffier.meetings().iter().map(|r| r.folder_name.as_str()).collect();
        self.greffier.meetings().iter().find(|r| r.folder_name == name).unwrap_or_else(|| panic!("pas de réunion « {name} » parmi {names:?}"))
    }

    pub fn execute(&mut self, command: Command) -> Vec<Event> {
        let name = format!("{command:?}");
        self.greffier.execute(command).unwrap_or_else(|e| panic!("{name} refusée : {e}"))
    }

    /// Avance le temps par pas de 0,1 s, comme le ferait l'interface.
    pub fn advance(&mut self, seconds: f64) -> Vec<Event> {
        let mut events = Vec::new();
        let step = (seconds / 0.1).round() as usize;
        for _ in 0..step {
            events.extend(self.greffier.advance(0.1));
        }
        events
    }

    /// Avance jusqu'à ce que `fin` soit vraie (au plus 60 s), en notant les étapes traversées.
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
        panic!("toujours {:?} après 60 s", self.meeting(id).status);
    }

    /// Enregistre une réunion de `duree` secondes (fausse capture au temps réel), sans la traiter.
    pub fn record(&mut self, duration: f64) -> MeetingId {
        self.execute(Command::StartRecording);
        self.advance(duration);
        self.finish_recording()
    }

    /// Arrête l'enregistrement en cours ; rend la réunion enregistrée.
    pub fn finish_recording(&mut self) -> MeetingId {
        let events = self.execute(Command::StopRecording);
        events
            .iter()
            .find_map(|e| match e {
                Event::MeetingRecorded(id) => Some(*id),
                _ => None,
            })
            .expect("la réunion enregistrée")
    }

    /// Une réunion enregistrée et pas encore traitée.
    pub fn meeting_to_process(&mut self) -> MeetingId {
        self.execute(Command::ChangeSettings(greffier_core::Settings { process_on_stop: false, ..self.greffier.settings().clone() }));
        self.record(5.0)
    }

    pub fn meeting(&self, id: MeetingId) -> &Meeting {
        self.greffier.meeting(id).expect("réunion connue")
    }

    /// Le cache de recherche à côté du dossier de réunions (supprimé avec lui), donné au cœur
    /// comme l'app le fait au lancement avec celui de son dossier.
    pub fn enable_search_cache(&mut self) -> PathBuf {
        let cache = self.folder.with_extension("search");
        self.execute(Command::ChangeSettings(greffier_core::Settings { search_cache: Some(cache.clone()), ..self.greffier.settings().clone() }));
        cache
    }

    pub fn read(&self, path: &Path) -> String {
        std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{} : {e}", path.display()))
    }
}

fn temp_folder() -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let folder = std::env::temp_dir().join(format!("greffier-bench-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).expect("dossier temporaire");
    folder
}

fn copy_to(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("copie des fixtures");
    for input in std::fs::read_dir(from).expect("fixtures") {
        let input = input.expect("fixture");
        let target = to.join(input.file_name());
        if input.file_type().expect("type").is_dir() {
            copy_to(&input.path(), &target);
        } else {
            std::fs::copy(input.path(), target).expect("copie d'une fixture");
        }
    }
}

impl Drop for Bench {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.folder);
        let _ = std::fs::remove_dir_all(self.folder.with_extension("search"));
    }
}
