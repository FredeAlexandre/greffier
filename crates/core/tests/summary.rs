//! The summary by the agent: the sheet receives title, brief, actions and session; the title
//! renames the folder and all its files, links included; changing the title in the app does the same.

mod bench;

use bench::Bench;
use greffier_core::adapters::{ReviewResult, SummaryResult};
use greffier_core::fake::FakeAgent;
use greffier_core::{Command, Stage, MeetingId, Role, Status};

/// An agent whose summary carries this title.
fn agent_with_title(title: &'static str) -> FakeAgent {
    FakeAgent::new(
        bench::STAGE_DURATION,
        |_| Ok(ReviewResult::default()),
        bench::STAGE_DURATION,
        move |_| Ok((bench::SUMMARY.to_string(), SummaryResult { title: title.into(), ..bench::summary_result() })),
    )
}

fn bench_with_title(title: &'static str) -> Bench {
    let mut a = bench::adapters();
    a.agent = Box::new(agent_with_title(title));
    Bench::with(a)
}

fn done_meeting(bench: &mut Bench) -> MeetingId {
    let id = bench.meeting_to_process();
    bench.execute(Command::Process(id));
    bench.advance_until(id, |s| *s == Status::Done);
    id
}

/// The names of a folder's files, sorted.
fn files(bench: &Bench, name: &str) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(bench.folder.join(name)).expect("meeting folder").flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    v.sort();
    v
}

#[test]
fn summary_title_renames_the_folder_and_its_files() {
    let mut bench = bench_with_title("Revue : budget/planning ?");
    let id = done_meeting(&mut bench);

    let name = "2026-11-21 14h02 - Revue - budget-planning -";
    assert_eq!(bench.meeting(id).title, "Revue : budget/planning ?");
    assert_eq!(bench.meeting(id).folder_name, name);
    assert!(!bench.folder.join("2026-11-21 14h02 - Réunion").exists());
    assert_eq!(
        files(&bench, name),
        [
            format!("{name} - bilan.md"),
            format!("{name} - enregistrement.mp4"),
            format!("{name} - réunion.md"),
            format!("{name} - transcription.json"),
            format!("{name} - transcription.md"),
        ]
    );
    assert_eq!(bench.read(&bench.greffier.folder().file(name, Role::Summary)), bench::SUMMARY);
}

#[test]
fn after_renaming_links_point_to_the_new_files() {
    // The agent's summary links to the sheet by its name (Obsidian link).
    let summary: &'static str = "---\nréunion: \"[[2026-11-21 14h02 - Réunion - réunion]]\"\n---\n\nVoir [la transcription](<2026-11-21 14h02 - Réunion - transcription.md>).\n\n## Sujets abordés\n\n### Budget (0:00:00)\n\n## Décisions\n\n- (0:00:03) On garde le budget.\n\n## Actions\n\n- [ ] Odile — envoyer le devis\n";
    let mut a = bench::adapters();
    a.agent = Box::new(FakeAgent::new(
        bench::STAGE_DURATION,
        |_| Ok(ReviewResult::default()),
        bench::STAGE_DURATION,
        move |_| Ok((summary.to_string(), SummaryResult { title: "Budget".into(), ..bench::summary_result() })),
    ));
    let mut bench = Bench::with(a);
    let id = done_meeting(&mut bench);

    let name = "2026-11-21 14h02 - Budget";
    let folder = bench.greffier.folder().clone();
    let sheet = bench.read(&folder.file(name, Role::Sheet));
    for role in [Role::Summary, Role::ReadableTranscription, Role::StructuredTranscription, Role::Recording] {
        let link = format!("(<{name} - {}>)", role.suffix());
        assert!(sheet.contains(&link), "{link} missing from the sheet:\n{sheet}");
        assert!(folder.file(name, role).is_file(), "{role:?}");
    }
    let md = bench.read(&folder.file(name, Role::Summary));
    assert!(md.contains("[[2026-11-21 14h02 - Budget - réunion]]"), "{md}");
    assert!(md.contains("(<2026-11-21 14h02 - Budget - transcription.md>)"), "{md}");
    assert_eq!(bench.meeting(id).summary, md);

    // Greffier renamed it itself: nothing to rescan, the meeting keeps its identity.
    assert_eq!(bench.greffier.rescan_folder(), []);
    bench.reopen();
    assert_eq!(bench.named_meeting(name).title, "Budget");
    assert_eq!(bench.named_meeting(name).status, Status::Done);
}

#[test]
fn changing_the_title_in_the_app_also_renames_the_folder() {
    let mut bench = Bench::new();
    let id = done_meeting(&mut bench);
    let old = bench.meeting(id).folder_name.clone();

    bench.execute(Command::ChangeTitle { meeting: id, title: "  Rétro * sprint 12  ".into() });

    let name = "2026-11-21 14h02 - Rétro - sprint 12";
    let r = bench.meeting(id);
    assert_eq!(r.title, "Rétro * sprint 12");
    assert_eq!(r.folder_name, name);
    assert!(!bench.folder.join(&old).exists());
    let sheet = bench.read(&bench.greffier.folder().file(name, Role::Sheet));
    assert!(sheet.starts_with("---\ntitre: \"Rétro * sprint 12\"\n"), "{sheet}");
    assert!(sheet.contains(&format!("- [Bilan](<{name} - bilan.md>)")), "{sheet}");
    assert_eq!(bench.greffier.rescan_folder(), []);
}

#[test]
fn a_title_already_taken_the_same_day_gets_a_number() {
    let mut bench = Bench::new();
    let a = bench.meeting_to_process();
    let b = bench.meeting_to_process();
    bench.execute(Command::ChangeTitle { meeting: a, title: "Daily".into() });
    bench.execute(Command::ChangeTitle { meeting: b, title: "Daily".into() });
    assert_eq!(bench.meeting(a).folder_name, "2026-11-21 14h02 - Daily");
    assert_eq!(bench.meeting(b).folder_name, "2026-11-21 14h02 - Daily (2)");

    // Setting the same title again changes nothing, even with its number.
    bench.execute(Command::ChangeTitle { meeting: b, title: "Daily".into() });
    assert_eq!(bench.meeting(b).folder_name, "2026-11-21 14h02 - Daily (2)");
    // A meeting to process without a transcription still has its sheet and its recording.
    assert!(bench.greffier.folder().file("2026-11-21 14h02 - Daily (2)", Role::Recording).is_file());
}

#[test]
fn title_does_not_change_during_a_stage_or_when_empty() {
    let mut bench = Bench::new();
    let id = bench.meeting_to_process();
    assert!(bench.greffier.execute(Command::ChangeTitle { meeting: id, title: "   ".into() }).is_err());

    bench.execute(Command::Process(id));
    bench.advance(0.2);
    assert!(matches!(bench.meeting(id).status, Status::Running { stage: Stage::Transcription, .. }));
    assert!(bench.greffier.execute(Command::ChangeTitle { meeting: id, title: "Plus tard".into() }).is_err());
    assert_eq!(bench.meeting(id).title, "Réunion");
}

#[test]
fn agent_receives_context_folders_and_meeting_files() {
    let requests = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let seen = requests.clone();
    let mut a = bench::adapters();
    a.agent = Box::new(FakeAgent::new(
        bench::STAGE_DURATION,
        |_| Ok(ReviewResult::default()),
        bench::STAGE_DURATION,
        move |d| {
            // The meeting's files exist when the agent starts.
            assert!(d.transcription.is_file() && d.sheet.is_file(), "{d:?}");
            seen.borrow_mut().push(d.clone());
            Ok((bench::SUMMARY.to_string(), bench::summary_result()))
        },
    ));
    let mut bench = Bench::with(a);
    let context = vec![std::path::PathBuf::from("/Users/odile/Notes/Projets"), std::path::PathBuf::from("/Users/odile/Clients")];
    let settings = greffier_core::Settings { context_folders: context.clone(), ..bench.greffier.settings().clone() };
    bench.execute(Command::ChangeSettings(settings));
    done_meeting(&mut bench);

    let d = requests.borrow()[0].clone();
    let name = "2026-11-21 14h02 - Réunion";
    let folder = bench.greffier.folder();
    assert_eq!(d.context_folders, context);
    assert_eq!(d.meetings_folder, bench.folder);
    assert_eq!(d.summary_file, folder.file(name, Role::Summary));
    assert_eq!(d.sheet, folder.file(name, Role::Sheet));
    assert_eq!(d.transcription, folder.file(name, Role::ReadableTranscription));
}
