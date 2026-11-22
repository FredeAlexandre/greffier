//! Deciding passages to check and correcting the transcription by hand: the utterance, the
//! structured transcription and the readable transcription follow.

mod bench;

use bench::Bench;
use greffier_core::adapters::{FlaggedPassage, ReviewResult};
use greffier_core::folder::transcription::StructuredTranscription;
use greffier_core::fake::FakeAgent;
use greffier_core::{Command, MeetingId, Role, Status, PassageStatus, Resolution};

/// A done meeting whose review left two passages to check: « tous » without a hypothesis in
/// « Bonjour à tous. », « Alexandre » (hypothesis « Alexandra ») in « Salut Alexandre. ».
fn meeting_to_check() -> (Bench, MeetingId) {
    let mut a = bench::adapters();
    a.agent = Box::new(FakeAgent::new(
        bench::STAGE_DURATION,
        |_| {
            Ok(ReviewResult {
                passages_to_check: vec![
                    FlaggedPassage { utterance: 0, excerpt: "tous".into(), hypothesis: None, relisten: "Bonjour à tous.".into() },
                    FlaggedPassage { utterance: 1, excerpt: "Alexandre".into(), hypothesis: Some("Alexandra".into()), relisten: "Salut Alexandra.".into() },
                ],
                ..Default::default()
            })
        },
        bench::STAGE_DURATION,
        |_| Ok((bench::SUMMARY.to_string(), bench::summary_result())),
    ));
    let mut bench = Bench::with(a);
    let id = bench.meeting_to_process();
    bench.execute(Command::Process(id));
    bench.advance_until(id, |s| *s == Status::Done);
    (bench, id)
}

fn readable(bench: &Bench, id: MeetingId) -> String {
    let r = bench.meeting(id);
    bench.read(&bench.greffier.folder().file(&r.folder_name, Role::ReadableTranscription))
}

fn structured(bench: &Bench, id: MeetingId) -> StructuredTranscription {
    let r = bench.meeting(id);
    StructuredTranscription::read(&bench.read(&bench.greffier.folder().file(&r.folder_name, Role::StructuredTranscription))).expect("json")
}

fn texts(bench: &Bench, id: MeetingId) -> Vec<String> {
    bench.meeting(id).utterances.iter().map(|x| x.text.clone()).collect()
}

#[test]
fn accepting_the_hypothesis_replaces_the_excerpt_everywhere_and_clears_the_flag() {
    let (mut bench, id) = meeting_to_check();
    assert!(readable(&bench, id).contains("> À vérifier : « Alexandre » (hypothèse : « Alexandra »)"));

    bench.execute(Command::Resolve { meeting: id, passage: 1, resolution: Resolution::AcceptHypothesis });

    assert_eq!(texts(&bench, id), ["Bonjour à tous.", "Salut Alexandra."]);
    assert_eq!(bench.meeting(id).pending_passages(), 1);
    let json = structured(&bench, id);
    assert_eq!(json.utterances[1].text, "Salut Alexandra.");
    assert_eq!(json.passages[1].status, "hypothesis_accepted");
    let md = readable(&bench, id);
    assert!(md.contains("Salut Alexandra.\n"), "{md}");
    assert!(!md.contains("« Alexandre »"), "{md}");
    assert!(md.contains("> À vérifier : « tous »"), "{md}");
}

#[test]
fn keeping_the_original_leaves_the_text_and_removes_the_marking() {
    let (mut bench, id) = meeting_to_check();

    bench.execute(Command::Resolve { meeting: id, passage: 0, resolution: Resolution::KeepOriginal });

    assert_eq!(texts(&bench, id), ["Bonjour à tous.", "Salut Alexandre."]);
    assert_eq!(bench.meeting(id).passages[0].status, PassageStatus::OriginalKept);
    assert_eq!(structured(&bench, id).passages[0].status, "original_kept");
    let md = readable(&bench, id);
    assert!(!md.contains("« tous »"), "{md}");
    assert!(md.contains("Bonjour à tous.\n"), "{md}");
}

#[test]
fn correcting_by_hand_replaces_the_excerpt_with_the_correction() {
    let (mut bench, id) = meeting_to_check();

    bench.execute(Command::Resolve { meeting: id, passage: 0, resolution: Resolution::Correct(" toutes et tous ".into()) });

    assert_eq!(texts(&bench, id), ["Bonjour à toutes et tous.", "Salut Alexandre."]);
    assert_eq!(bench.meeting(id).passages[0].status, PassageStatus::CorrectedManually("toutes et tous".into()));
    let json = structured(&bench, id);
    assert_eq!(json.utterances[0].text, "Bonjour à toutes et tous.");
    assert_eq!((json.passages[0].status.as_str(), json.passages[0].correction.as_deref()), ("corrected_manually", Some("toutes et tous")));
    let md = readable(&bench, id);
    assert!(md.contains("Bonjour à toutes et tous.\n"), "{md}");
    assert!(!md.contains("« tous »"), "{md}");
}

#[test]
fn an_empty_correction_is_rejected() {
    let (mut bench, id) = meeting_to_check();
    assert!(bench.greffier.execute(Command::Resolve { meeting: id, passage: 0, resolution: Resolution::Correct("  ".into()) }).is_err());
    assert!(bench.greffier.execute(Command::Resolve { meeting: id, passage: 0, resolution: Resolution::AcceptHypothesis }).is_err(), "no hypothesis");
    assert_eq!(bench.meeting(id).pending_passages(), 2);
}

#[test]
fn reopening_a_passage_restores_the_excerpt_and_the_marking() {
    let (mut bench, id) = meeting_to_check();
    bench.execute(Command::Resolve { meeting: id, passage: 1, resolution: Resolution::AcceptHypothesis });
    bench.execute(Command::Resolve { meeting: id, passage: 1, resolution: Resolution::Correct("Alexandrine".into()) });
    assert_eq!(texts(&bench, id)[1], "Salut Alexandrine.");

    bench.execute(Command::Resolve { meeting: id, passage: 1, resolution: Resolution::Reopen });

    assert_eq!(texts(&bench, id)[1], "Salut Alexandre.");
    assert_eq!(bench.meeting(id).pending_passages(), 2);
    assert!(readable(&bench, id).contains("> À vérifier : « Alexandre » (hypothèse : « Alexandra »)"));
}

#[test]
fn decided_passages_persist_across_launches() {
    let (mut bench, id) = meeting_to_check();
    bench.execute(Command::Resolve { meeting: id, passage: 0, resolution: Resolution::Correct("vous".into()) });
    bench.execute(Command::Resolve { meeting: id, passage: 1, resolution: Resolution::AcceptHypothesis });
    let name = bench.meeting(id).folder_name.clone();

    bench.reopen();

    let r = bench.named_meeting(&name);
    let texts: Vec<&str> = r.utterances.iter().map(|x| x.text.as_str()).collect();
    assert_eq!(texts, ["Bonjour à vous.", "Salut Alexandra."]);
    assert_eq!(r.pending_passages(), 0);
    assert_eq!(r.passages[0].status, PassageStatus::CorrectedManually("vous".into()));
    let (id, passage) = (r.id, 0);
    bench.execute(Command::Resolve { meeting: id, passage, resolution: Resolution::Reopen });
    assert_eq!(bench.meeting(id).utterances[0].text, "Bonjour à tous.");
}

#[test]
fn correcting_a_word_records_it_as_a_manual_correction() {
    let (mut bench, id) = meeting_to_check();

    bench.execute(Command::CorrectUtterance { meeting: id, utterance: 0, text: "Bonsoir à tous.".into() });

    assert_eq!(texts(&bench, id), ["Bonsoir à tous.", "Salut Alexandre."]);
    let json = structured(&bench, id);
    let corrections: Vec<(usize, &str, &str)> = json.manual_corrections.iter().map(|c| (c.utterance, c.before.as_str(), c.after.as_str())).collect();
    assert_eq!(corrections, [(0, "Bonjour", "Bonsoir")]);
    assert_eq!(json.utterances[0].text, "Bonsoir à tous.");
    let md = readable(&bench, id);
    assert!(md.contains("Bonsoir à tous.\n"), "{md}");
    // The utterance's passage to check is still there, and can still be decided.
    assert!(md.contains("> À vérifier : « tous »"), "{md}");
    bench.execute(Command::Resolve { meeting: id, passage: 0, resolution: Resolution::Correct("vous".into()) });
    assert_eq!(texts(&bench, id)[0], "Bonsoir à vous.");
}

#[test]
fn a_correction_applies_to_whole_words() {
    let (mut bench, id) = meeting_to_check();

    bench.execute(Command::CorrectUtterance { meeting: id, utterance: 0, text: "Bonjou à tous.".into() });
    bench.execute(Command::CorrectUtterance { meeting: id, utterance: 0, text: "Bonjou, à tous.".into() });
    bench.execute(Command::CorrectUtterance { meeting: id, utterance: 0, text: "Bonjou, a tous.".into() });

    let c: Vec<(&str, &str)> = bench.meeting(id).manual_corrections.iter().map(|c| (c.before.as_str(), c.after.as_str())).collect();
    assert_eq!(c, [("Bonjour", "Bonjou"), ("Bonjou", "Bonjou,"), ("à", "a")]);
}

#[test]
fn a_correction_without_change_saves_nothing_and_an_empty_one_is_rejected() {
    let (mut bench, id) = meeting_to_check();
    bench.execute(Command::CorrectUtterance { meeting: id, utterance: 1, text: "Salut Alexandre.".into() });
    assert!(bench.meeting(id).manual_corrections.is_empty());
    assert!(bench.greffier.execute(Command::CorrectUtterance { meeting: id, utterance: 1, text: " ".into() }).is_err());
    assert!(bench.greffier.execute(Command::CorrectUtterance { meeting: id, utterance: 9, text: "Oui.".into() }).is_err());
}

#[test]
fn a_passage_to_check_is_resolved_in_its_tab_not_by_correcting_the_utterance() {
    let (mut bench, id) = meeting_to_check();
    assert!(bench.greffier.execute(Command::CorrectUtterance { meeting: id, utterance: 1, text: "Salut Alexandrine.".into() }).is_err());
    assert_eq!(texts(&bench, id)[1], "Salut Alexandre.");
}

#[test]
fn manual_corrections_survive_a_redone_review() {
    let (mut bench, id) = meeting_to_check();
    bench.execute(Command::CorrectUtterance { meeting: id, utterance: 0, text: "Bonsoir à tous.".into() });

    bench.execute(Command::Redo { meeting: id, stage: greffier_core::Stage::Review });
    bench.advance_until(id, |s| *s == Status::Done);

    assert_eq!(texts(&bench, id)[0], "Bonsoir à tous.");
    assert_eq!(bench.meeting(id).manual_corrections.len(), 1);
}

#[test]
fn transcription_cannot_be_corrected_during_review() {
    let (mut bench, id) = meeting_to_check();
    bench.execute(Command::Redo { meeting: id, stage: greffier_core::Stage::Review });

    assert!(bench.greffier.execute(Command::CorrectUtterance { meeting: id, utterance: 0, text: "Bonsoir à tous.".into() }).is_err());
    assert!(bench.greffier.execute(Command::Resolve { meeting: id, passage: 0, resolution: Resolution::KeepOriginal }).is_err());
}

#[test]
fn listening_to_a_passage_plays_its_utterance_within_1_5_s_like_relisten() {
    let (bench, id) = meeting_to_check();
    let r = bench.meeting(id);
    // « Salut Alexandre. » runs from 3.0 to 4.0 s; « Bonjour à tous. » starts at 0.
    assert_eq!(r.excerpt_to_listen(1), Some((1.5, 5.5)));
    assert_eq!(r.excerpt_to_listen(0), Some((0.0, 3.0)));
    assert_eq!(r.excerpt_to_listen(2), None);
}
