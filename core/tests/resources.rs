//! Les ressources téléchargées au premier lancement, vues par le cœur : modèles vérifiés par
//! sha256, téléchargements qui reprennent, Python privé et vérification de la CLI `claude`. Les
//! outils (téléchargement, empreinte, uv, `claude`) sont faux ; tout le reste est celui de l'app.

mod bench;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use bench::Bench;
use greffier_core::adapters::{ResourceState, Resource};
use greffier_core::fake::FakeTools;
use greffier_core::resources::{Catalogue, Element, ClaudeState, ResourceFile, AppResources};
use greffier_core::{Command, Stage, Event, Status};

const LARGE: &str = "poids du grand modèle, trente-deux octets";
const SHA_LARGE: &str = "97325706e5b8a9bc0cd158ac197ee74e20e19781defb99affe5a375866fca53a";
const URL_LARGE: &str = "https://exemple.org/grand.bin";
const SMALL: &str = "petit modèle";
const SHA_SMALL: &str = "f043ba3eb2682d2f4c03e0f655434803aedd1506facdc371a0544ae0ae568684";
const URL_SMALL: &str = "https://exemple.org/petit.bin";
const PYPROJECT: &str = "[project]\nname = \"locuteurs\"\n";
const LOCKFILE: &str = "version = 1\n";

/// Le dossier de l'app, dans un répertoire temporaire supprimé à la fin du test.
struct AppFolder(PathBuf);

impl AppFolder {
    fn new() -> AppFolder {
        static N: AtomicU32 = AtomicU32::new(0);
        let d = std::env::temp_dir().join(format!("greffier-app-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        let _ = std::fs::remove_dir_all(&d);
        AppFolder(d)
    }
}

impl Drop for AppFolder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn catalogue(lock: &str) -> Catalogue {
    Catalogue(vec![
        Element::files("Grand modèle", "exemple.org", &[Stage::Transcription], vec![ResourceFile::new("modeles/grand.bin", URL_LARGE, LARGE.len() as u64, SHA_LARGE)]),
        Element::files("Petit modèle", "exemple.org", &[Stage::Transcription], vec![ResourceFile::new("modeles/petit/a.bin", URL_SMALL, SMALL.len() as u64, SHA_SMALL)]),
        Element::python("Python privé", "uv", PYPROJECT, lock),
        Element::claude("Claude Code"),
    ])
}

/// 10 octets par seconde ; une empreinte prend 0,5 s, le Python 2 s, `claude` répond aussitôt.
fn tools() -> FakeTools {
    FakeTools::new(10.0).serve(URL_LARGE, LARGE.as_bytes()).serve(URL_SMALL, SMALL.as_bytes()).fingerprint_duration(0.5).python(2.0, Ok(()))
}

fn bench_on(app: &Path, tools: FakeTools) -> Bench {
    let mut a = bench::adapters();
    a.resources = Box::new(AppResources::new(app, catalogue(LOCKFILE), tools));
    Bench::with(a)
}

fn state(bench: &Bench, name: &str) -> ResourceState {
    resource(bench, name).state
}

fn resource(bench: &Bench, name: &str) -> Resource {
    bench.greffier.resources().iter().find(|r| r.name == name).unwrap_or_else(|| panic!("ressource {name}")).clone()
}

fn install_to_completion(bench: &mut Bench) -> Vec<Event> {
    bench.execute(Command::InstallResources);
    let mut events = Vec::new();
    for _ in 0..300 {
        events.extend(bench.advance(0.1));
        if !bench.greffier.busy() {
            return events;
        }
    }
    panic!("installation toujours en cours après 30 s");
}

#[test]
fn first_launch_shows_each_resource_state_until_they_are_ready() {
    let app = AppFolder::new();
    let mut bench = bench_on(&app.0, tools());
    let names: Vec<String> = bench.greffier.resources().iter().map(|r| r.name.clone()).collect();
    assert_eq!(names, ["Grand modèle", "Petit modèle", "Python privé", "Claude Code"]);
    assert_eq!(state(&bench, "Grand modèle"), ResourceState::Missing);
    assert_eq!(state(&bench, "Python privé"), ResourceState::Missing);

    bench.execute(Command::InstallResources);
    bench.advance(0.3);
    // Les plus petits fichiers d'abord ; le Python s'installe en même temps.
    assert_eq!(state(&bench, "Petit modèle"), ResourceState::Downloading { received: 3, total: 13 });
    assert_eq!(state(&bench, "Grand modèle"), ResourceState::Missing);
    assert_eq!(state(&bench, "Python privé"), ResourceState::Installing);
    assert_eq!(state(&bench, "Claude Code"), ResourceState::Ready);

    bench.advance(1.2);
    assert_eq!(state(&bench, "Petit modèle"), ResourceState::Verifying);
    bench.advance(0.6);
    assert_eq!(state(&bench, "Petit modèle"), ResourceState::Ready);
    assert!(matches!(state(&bench, "Grand modèle"), ResourceState::Downloading { received, total: 42 } if received > 0));

    let events = install_to_completion(&mut bench);
    assert!(events.contains(&Event::ResourcesInstalled), "{events:?}");
    for r in bench.greffier.resources() {
        assert_eq!(r.state, ResourceState::Ready, "{}", r.name);
    }
    assert_eq!(bench.read(&app.0.join("modeles/grand.bin")), LARGE);
    assert_eq!(bench.read(&app.0.join("modeles/petit/a.bin")), SMALL);
    assert_eq!(resource(&bench, "Grand modèle").size, 42);
}

#[test]
fn settings_see_the_disk_space_used_by_each_resource() {
    let app = AppFolder::new();
    let mut bench = bench_on(&app.0, tools());
    install_to_completion(&mut bench);
    assert_eq!(resource(&bench, "Grand modèle").size, LARGE.len() as u64);
    assert_eq!(resource(&bench, "Petit modèle").size, SMALL.len() as u64);
    assert!(resource(&bench, "Python privé").size >= (PYPROJECT.len() + LOCKFILE.len()) as u64);
    assert_eq!(resource(&bench, "Claude Code").size, 0);
}

#[test]
fn a_wrong_sha256_fails_the_resource_then_the_file_is_redownloaded() {
    let app = AppFolder::new();
    let mut bench = bench_on(&app.0, tools().corrupt_once(URL_LARGE));

    let events = install_to_completion(&mut bench);
    let ResourceState::Failed(message) = state(&bench, "Grand modèle") else { panic!("{:?}", state(&bench, "Grand modèle")) };
    assert!(message.contains("sha256"), "{message}");
    assert!(events.iter().any(|e| matches!(e, Event::InstallationFailed(m) if m.contains("Grand modèle"))), "{events:?}");
    assert_eq!(state(&bench, "Petit modèle"), ResourceState::Ready);
    // Rien de faux ne reste sur le disque.
    assert_eq!(resource(&bench, "Grand modèle").size, 0);
    assert!(!app.0.join("modeles/grand.bin").exists());

    let events = install_to_completion(&mut bench);
    assert!(events.contains(&Event::ResourcesInstalled), "{events:?}");
    assert_eq!(state(&bench, "Grand modèle"), ResourceState::Ready);
    assert_eq!(bench.read(&app.0.join("modeles/grand.bin")), LARGE);
}

#[test]
fn an_interrupted_download_resumes_where_it_stopped_at_next_launch() {
    let app = AppFolder::new();
    {
        let mut bench = bench_on(&app.0, tools().cut_once(URL_LARGE, 20));
        let events = install_to_completion(&mut bench);
        assert!(events.iter().any(|e| matches!(e, Event::InstallationFailed(_))), "{events:?}");
        assert!(matches!(state(&bench, "Grand modèle"), ResourceState::Failed(m) if m.contains("reprendra")));
    }

    // L'app est relancée : le téléchargement est vu comme interrompu, et reprend à 20 octets.
    let mut bench = bench_on(&app.0, tools());
    assert_eq!(state(&bench, "Grand modèle"), ResourceState::Interrupted { received: 20, total: 42 });
    assert_eq!(state(&bench, "Petit modèle"), ResourceState::Ready);
    bench.execute(Command::InstallResources);
    bench.advance(0.1);
    assert_eq!(state(&bench, "Grand modèle"), ResourceState::Downloading { received: 21, total: 42 });

    install_to_completion(&mut bench);
    assert_eq!(state(&bench, "Grand modèle"), ResourceState::Ready);
    assert_eq!(bench.read(&app.0.join("modeles/grand.bin")), LARGE);
}

#[test]
fn private_python_installs_in_the_app_folder_and_reinstalls_if_the_lock_changes() {
    let app = AppFolder::new();
    {
        let mut bench = bench_on(&app.0, tools());
        install_to_completion(&mut bench);
        assert_eq!(state(&bench, "Python privé"), ResourceState::Ready);
        assert_eq!(bench.read(&app.0.join("python/project/uv.lock")), LOCKFILE);
        assert_eq!(bench.read(&app.0.join("python/project/pyproject.toml")), PYPROJECT);
    }
    {
        let bench = bench_on(&app.0, tools());
        assert_eq!(state(&bench, "Python privé"), ResourceState::Ready);
    }
    let mut a = bench::adapters();
    a.resources = Box::new(AppResources::new(&app.0, catalogue("version = 2\n"), tools()));
    let bench = Bench::with(a);
    assert_eq!(state(&bench, "Python privé"), ResourceState::Missing);
}

#[test]
fn python_install_failure_is_shown_with_its_message() {
    let app = AppFolder::new();
    let mut bench = bench_on(&app.0, tools().python(1.0, Err("import pyannote.audio a échoué".into())));
    install_to_completion(&mut bench);
    assert_eq!(state(&bench, "Python privé"), ResourceState::Failed("import pyannote.audio a échoué".into()));
    assert_eq!(state(&bench, "Grand modèle"), ResourceState::Ready);
}

#[test]
fn missing_or_disconnected_claude_is_reported_with_what_to_do() {
    let app = AppFolder::new();
    let mut bench = bench_on(&app.0, tools().claude(ClaudeState::Missing));
    bench.execute(Command::CheckResources);
    bench.advance(0.1);
    let claude = resource(&bench, "Claude Code");
    assert_eq!(claude.state, ResourceState::Missing);
    assert!(claude.help.as_deref().is_some_and(|a| a.contains("claude")), "{claude:?}");

    let mut bench = bench_on(&app.0, tools().claude(ClaudeState::NotConnected { version: "2.1.329".into() }));
    bench.execute(Command::CheckResources);
    bench.advance(0.1);
    let claude = resource(&bench, "Claude Code");
    assert!(matches!(&claude.state, ResourceState::Failed(m) if m.contains("connectée")), "{claude:?}");
    assert!(claude.help.as_deref().is_some_and(|a| a.contains("/login")), "{claude:?}");

    let mut bench = bench_on(&app.0, tools().claude(ClaudeState::Connected { version: "2.1.329".into(), account: "claude.ai".into() }));
    bench.execute(Command::CheckResources);
    bench.advance(0.1);
    let claude = resource(&bench, "Claude Code");
    assert_eq!(claude.state, ResourceState::Ready);
    assert!(claude.detail.contains("2.1.329"), "{claude:?}");
    assert_eq!(claude.help, None);
}

#[test]
fn without_claude_transcription_proceeds_but_review_waits() {
    let app = AppFolder::new();
    let mut bench = bench_on(&app.0, tools().claude(ClaudeState::Missing));
    install_to_completion(&mut bench);
    let id = bench.meeting_to_process();
    bench.execute(Command::Process(id));
    let stages = bench.advance_until(id, |s| matches!(s, Status::Failed { .. }));
    assert_eq!(stages, [Stage::Transcription]);
    assert_eq!(bench.meeting(id).status, Status::Failed { stage: Stage::Review, message: "Ressources manquantes : Claude Code.".into() });
}
