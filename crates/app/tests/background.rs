//! The Greffier process the command line talks to (ADR 0006): a request answered on the running
//! app changes what its window shows, as the user's own action would, and a process started in
//! the background (`--background`) quits on its own once idle, unless its window was shown.

mod support;

use std::path::PathBuf;

use greffier::{App, Launch, Page};
use greffier_cli::protocol::{Action, Request, Response};
use greffier_core::fake::FakeTrash;
use greffier_core::{Greffier, MeetingId, Stage, Status, demo};
use rune::{Env, Runtime, Shell};
use support::*;

/// The sample meeting `2026-11-20 10h00 - Revue de release`, as the command line names it.
const REVIEW: &str = "2026-11-20-1000";

/// Sends `request` to the app as its socket does, at the app's current time.
fn ask(rt: &mut Runtime<App>, request: Request) -> Response {
    let mut out = None;
    rt.update(|s, tx| out = Some(s.answer(request, tx.clock())));
    settle(rt);
    out.expect("update ran")
}

fn execute(rt: &mut Runtime<App>, action: Action) -> Response {
    let response = ask(rt, Request::Execute(action));
    assert!(matches!(response, Response::Ok(_)), "{response:?}");
    response
}

/// The meeting the command line names `id`.
fn meeting_named(rt: &mut Runtime<App>, folder_name_start: &str) -> MeetingId {
    state(rt, |s| s.core.meetings().iter().find(|m| m.folder_name.starts_with(folder_name_start)).map(|m| m.id).expect("the meeting"))
}

/// Everything written inside the element keyed `key`.
fn texts_in(rt: &Runtime<App>, key: &str) -> Vec<String> {
    let elements = rt.elements();
    let Some(root) = elements.iter().find(|e| !e.exiting && e.key.as_deref() == Some(key)) else { return vec![] };
    let path = root.path.clone();
    elements.iter().filter(|e| !e.exiting && e.path.starts_with(&path)).filter_map(|e| e.text.clone().or_else(|| e.label.clone())).collect()
}

/// The sample meetings on disk, with this trash.
fn meetings_on_disk(trash: &FakeTrash) -> Runtime<App> {
    let trash = trash.clone();
    app_on(Greffier::open(fixtures_folder(), adapters_with(|a| a.trash = Box::new(trash))))
}

// --- Requests act on the window ------------------------------------------------------------------

#[test]
fn a_rename_from_the_command_line_shows_in_the_sidebar_and_on_the_open_page() {
    let mut rt = app_on(Greffier::open(fixtures_folder(), demo::adapters()));
    let id = meeting_named(&mut rt, "2026-11-20");
    tap(&mut rt, &format!("m{id}"));

    execute(&mut rt, Action::Rename { meeting: REVIEW.into(), title: "Titre de test".into() });

    assert_eq!(state(&mut rt, |s| s.core.meeting(id).map(|m| m.title.clone())), Some("Titre de test".into()));
    assert!(texts_in(&rt, &format!("m{id}")).iter().any(|t| t.contains("Titre de test")), "the sidebar shows the new title: {:?}", texts_in(&rt, &format!("m{id}")));
    assert!(texts_in(&rt, "meeting-title").iter().any(|t| t.contains("Titre de test")), "so does the open page");
    assert!(state(&mut rt, |s| s.page == Page::Meeting(id)), "the page stays open");
}

#[test]
fn deleting_the_open_meeting_from_the_command_line_leaves_its_page() {
    let trash = FakeTrash::new();
    let mut rt = meetings_on_disk(&trash);
    let id = meeting_named(&mut rt, "2026-11-20");
    let folder = state(&mut rt, |s| s.core.meeting_folder(id).expect("its folder"));
    tap(&mut rt, &format!("m{id}"));
    rt.update(|s, _| s.start_rename(id));

    execute(&mut rt, Action::Delete { meeting: REVIEW.into() });

    assert_eq!(trash.trashed(), [folder]);
    state(&mut rt, |s| {
        assert!(s.core.meeting(id).is_none());
        assert!(s.page == Page::Overview, "the deleted meeting's page is left");
        assert!(s.video.is_none(), "its video is forgotten");
        assert!(s.rename.is_none(), "and the window renaming it closes");
    });
    assert!(!shown(&rt, &format!("m{id}")), "gone from the sidebar");
}

/// A sample meeting's recording and transcription, copied to a fresh folder.
fn files_to_import() -> Vec<PathBuf> {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../core/tests/fixtures/meetings-folder/2026-11-18 09h15 - Réunion");
    let folder = temp_folder();
    ["enregistrement.mp4", "transcription.json"]
        .map(|suffix| {
            let to = folder.join(format!("Point banc de test.{}", suffix.rsplit('.').next().expect("extension")));
            std::fs::copy(fixture.join(format!("2026-11-18 09h15 - Réunion - {suffix}")), &to).expect("copy");
            to
        })
        .to_vec()
}

fn new_meeting(rt: &mut Runtime<App>, before: &[MeetingId]) -> MeetingId {
    state(rt, |s| s.core.meetings().iter().map(|m| m.id).find(|id| !before.contains(id)).expect("a new meeting"))
}

#[test]
fn an_import_from_the_command_line_asks_nothing_and_leaves_the_page_alone() {
    let mut rt = app_on(demo_core());
    let before: Vec<MeetingId> = state(&mut rt, |s| s.core.meetings().iter().map(|m| m.id).collect());

    execute(&mut rt, Action::Import { files: files_to_import(), title: Some("Point banc".into()), speakers: None, process: false });

    let id = new_meeting(&mut rt, &before);
    state(&mut rt, |s| {
        assert_eq!(s.core.meeting(id).map(|m| m.status.clone()), Some(Status::ToProcess), "processed later, as asked");
        assert!(s.import.is_none(), "no import window");
        assert!(s.process_imported.is_none(), "no question about when to process it: the command line said");
        assert!(s.page == Page::Overview, "the page the user was on stays");
        assert_eq!(s.toast.as_ref().and_then(|t| t.meeting), Some(id), "a toast says it came in");
    });
    assert!(shown(&rt, &format!("m{id}")), "it is in the sidebar");
}

#[test]
fn an_import_processed_at_once_from_the_command_line_asks_nothing() {
    let mut rt = app_on(demo_core());
    let before: Vec<MeetingId> = state(&mut rt, |s| s.core.meetings().iter().map(|m| m.id).collect());

    execute(&mut rt, Action::Import { files: files_to_import(), title: None, speakers: None, process: true });

    let id = new_meeting(&mut rt, &before);
    state(&mut rt, |s| {
        assert_eq!(queued_at(&s.core.meeting(id).expect("the meeting").status), Some(Stage::Review));
        assert!(s.process_imported.is_none());
        assert!(s.page == Page::Overview);
    });
}

#[test]
fn processing_from_the_command_line_answers_the_question_left_open_after_an_import() {
    let mut rt = app_on(demo_core());
    let before: Vec<MeetingId> = state(&mut rt, |s| s.core.meetings().iter().map(|m| m.id).collect());
    let files = files_to_import();
    rt.update(move |s, _| {
        s.propose_import(&files);
        s.confirm_import();
    });
    let id = new_meeting(&mut rt, &before);
    assert_eq!(state(&mut rt, |s| s.process_imported), Some(id), "the window asks when to process it");
    let name = state(&mut rt, |s| s.core.meeting(id).expect("the meeting").folder_name.clone());

    execute(&mut rt, Action::Process { meeting: name, speakers: None });

    assert!(state(&mut rt, |s| s.process_imported.is_none()), "the question is moot");
}

#[test]
fn the_process_answers_with_the_meetings_folder_once_chosen() {
    let folder = fixtures_folder();
    let mut rt = app_on(Greffier::open(&folder, demo::adapters()));
    let Response::Hello(hello) = ask(&mut rt, Request::Hello) else { panic!("hello") };
    assert_eq!(hello.meetings_folder, Some(folder.canonicalize().expect("folder")));
    assert!(!hello.background);

    rt.update(|s, _| s.needs_folder = true);
    let Response::Hello(hello) = ask(&mut rt, Request::Hello) else { panic!("hello") };
    assert_eq!(hello.meetings_folder, None, "first launch: no folder chosen yet");
}

// --- The background process quits on its own ----------------------------------------------------

/// The app started by the command line (`--background`), quitting after `idle_quit` seconds idle.
fn in_background(core: Greffier, idle_quit: f64) -> Runtime<App> {
    let mut rt = rune::App::new(greffier::spec()).init(move || App::with_core(core).in_background(idle_quit)).headless_runtime(SIZE, Env::new(false, 1.0));
    rt.frame(0.0);
    rt
}

/// Lets `seconds` pass, a frame a second: nothing on screen moves.
fn idle_for(rt: &mut Runtime<App>, seconds: u32) {
    for _ in 0..seconds {
        rt.frame(1.0);
    }
}

fn quit_asked(rt: &Runtime<App>) -> bool {
    rt.shell_request == Some(Shell::Quit)
}

#[test]
fn a_background_process_quits_after_the_idle_delay() {
    let mut rt = in_background(demo_core(), 300.0);
    assert!(state(&mut rt, |s| s.background));
    idle_for(&mut rt, 295);
    assert!(!quit_asked(&rt), "not before the delay");
    idle_for(&mut rt, 10);
    assert!(quit_asked(&rt), "quits once idle for the delay");
}

#[test]
fn a_request_starts_the_idle_delay_again() {
    let mut rt = in_background(demo_core(), 300.0);
    idle_for(&mut rt, 200);
    let Response::Hello(hello) = ask(&mut rt, Request::Hello) else { panic!("hello") };
    assert!(hello.background);
    idle_for(&mut rt, 200);
    assert!(!quit_asked(&rt), "300 s from the request, not from the launch");
    idle_for(&mut rt, 110);
    assert!(quit_asked(&rt));
}

#[test]
fn a_background_process_never_quits_once_its_window_was_shown() {
    let mut rt = in_background(demo_core(), 300.0);
    idle_for(&mut rt, 10);
    assert!(matches!(ask(&mut rt, Request::OpenWindow), Response::Ok(_)));
    idle_for(&mut rt, 600);
    assert!(!quit_asked(&rt), "the user quits it now");
}

#[test]
fn reopening_the_app_shows_the_window_for_good() {
    let mut rt = in_background(demo_core(), 300.0);
    rt.update(|s, _| s.show_window());
    idle_for(&mut rt, 600);
    assert!(!quit_asked(&rt));
}

#[test]
fn a_background_process_waits_for_its_processing_to_end() {
    let mut rt = in_background(demo_core(), 300.0);
    let id = state(&mut rt, |s| s.core.meetings().iter().find(|m| m.status == Status::ToProcess).map(|m| m.id).expect("a meeting to process"));
    let name = state(&mut rt, |s| s.core.meeting(id).expect("the meeting").folder_name.clone());
    execute(&mut rt, Action::Process { meeting: name, speakers: None });
    let processing: f64 = [Stage::Transcription, Stage::Review, Stage::Summary].into_iter().map(demo::simulated_duration).sum();
    assert!(processing > 10.0, "the demo's processing takes time");
    // The core moves a quarter of a second per frame at most: frames of 1/4 s.
    let mut t = 0.0;
    while state(&mut rt, |s| s.core.busy()) {
        rt.frame(0.25);
        t += 0.25;
        assert!(t < 3600.0, "the processing ends");
        assert!(!quit_asked(&rt), "never while processing");
    }
    assert_eq!(state(&mut rt, |s| s.core.meeting(id).map(|m| m.status.clone())), Some(Status::Done));
    idle_for(&mut rt, 295);
    assert!(!quit_asked(&rt), "the delay counts from the end of the processing");
    idle_for(&mut rt, 10);
    assert!(quit_asked(&rt));
}

#[test]
fn a_background_process_that_cannot_listen_quits_at_once() {
    let mut rt = in_background(demo_core(), 300.0);
    rt.update(|s, tx| s.serve(Err("chemin du socket trop long".into()), tx));
    assert!(quit_asked(&rt), "nobody can reach it: it would idle for nothing");
}

#[test]
fn a_window_that_cannot_listen_stays_open() {
    let mut rt = app_on(demo_core());
    rt.update(|s, tx| s.serve(Err("chemin du socket trop long".into()), tx));
    idle_for(&mut rt, 10);
    assert!(!quit_asked(&rt));
}

#[test]
fn the_window_opened_normally_never_quits_on_its_own() {
    let mut rt = app_on(demo_core());
    idle_for(&mut rt, 900);
    assert!(!quit_asked(&rt));
}

// --- Launching ----------------------------------------------------------------------------------

#[test]
fn the_launch_reads_background_and_serves_only_a_live_window() {
    let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(Launch::new(&args(&[]), false), Launch { background: false, serve: true });
    assert_eq!(Launch::new(&args(&["--background"]), false), Launch { background: true, serve: true });
    assert_eq!(Launch::new(&args(&["--background"]), true), Launch { background: true, serve: false }, "headless never serves");
    assert_eq!(Launch::new(&args(&["--scene", "meeting"]), false), Launch { background: false, serve: false }, "nor does a scene");
}
