//! Nommer et fusionner les locuteurs : les propositions du bilan, confirmer ou corriger un nom,
//! fusionner deux locuteurs qui sont la même voix (et défaire la fusion), et les fichiers qui en
//! résultent (fiche, transcription lisible, transcription structurée).

mod bench;

use bench::{Bench, word, turn};
use greffier_core::adapters::{Adapters, SummarySpeaker, ReviewResult, SummaryResult};
use greffier_core::fake::{FakeAgent, FakeDiarizer, FakeTranscriber};
use greffier_core::{Command, MeetingId, Role, Status};

/// Trois répliques : SPEAKER_00 salue, SPEAKER_01 répond, SPEAKER_02 (en fait la voix de
/// SPEAKER_00) relance. L'agent du bilan reconnaît Alexandre et Khalid, voit que SPEAKER_02 est
/// sans doute Alexandre, et ne sait pas qui est SPEAKER_03, qui ne dit qu'un mot.
fn adapters() -> Adapters {
    let words = vec![
        word(0.0, 0.5, "Bonjour"),
        word(0.5, 0.8, "à"),
        word(0.8, 1.5, "tous."),
        word(3.0, 3.4, "Salut"),
        word(3.4, 4.0, "Alexandre."),
        word(6.0, 6.5, "On"),
        word(6.5, 7.0, "commence ?"),
        word(9.0, 10.0, "Oui."),
    ];
    let turns = vec![turn(0.0, 1.6, "SPEAKER_00"), turn(2.9, 4.1, "SPEAKER_01"), turn(5.9, 7.1, "SPEAKER_02"), turn(8.9, 10.1, "SPEAKER_03")];
    let speakers = vec![
        SummarySpeaker { speaker: "SPEAKER_00".into(), proposed_name: "Alexandre".into(), justification: "Khalid l'appelle Alexandre à 0:00:03.".into(), same_voice_as: None },
        SummarySpeaker { speaker: "SPEAKER_01".into(), proposed_name: "Khalid".into(), justification: "Répond à Alexandre à 0:00:03.".into(), same_voice_as: None },
        SummarySpeaker { speaker: "SPEAKER_02".into(), proposed_name: "Alexandre".into(), justification: "Même voix qu'Alexandre, qui relance à 0:00:06.".into(), same_voice_as: Some("SPEAKER_00".into()) },
        SummarySpeaker { speaker: "SPEAKER_03".into(), proposed_name: String::new(), justification: "Ne dit que « Oui » à 0:00:09 ; personne ne le nomme.".into(), same_voice_as: None },
    ];
    let mut a = bench::adapters();
    a.transcriber = Box::new(FakeTranscriber::fixed(bench::STAGE_DURATION, words));
    a.diarizer = Box::new(FakeDiarizer::fixed(bench::STAGE_DURATION, turns));
    a.agent = Box::new(FakeAgent::new(
        bench::STAGE_DURATION,
        |_| Ok(ReviewResult::default()),
        bench::STAGE_DURATION,
        move |_| Ok((bench::SUMMARY.to_string(), SummaryResult { speakers: speakers.clone(), ..bench::summary_result() })),
    ));
    a
}

fn done_meeting() -> (Bench, MeetingId) {
    let mut bench = Bench::with(adapters());
    let id = bench.meeting_to_process();
    bench.execute(Command::Process(id));
    bench.advance_until(id, |s| *s == Status::Done);
    (bench, id)
}

fn file(bench: &Bench, id: MeetingId, role: Role) -> String {
    let name = bench.meeting(id).folder_name.clone();
    bench.read(&bench.greffier.folder().file(&name, role))
}

/// Une table du frontmatter de la fiche : la ligne `cle:` et ses lignes en retrait.
fn block(sheet: &str, key: &str) -> String {
    let mut lines = sheet.lines().skip_while(|l| *l != format!("{key}:"));
    let first = lines.next().unwrap_or_else(|| panic!("pas de {key} dans la fiche :\n{sheet}"));
    std::iter::once(first).chain(lines.take_while(|l| l.starts_with("  "))).map(|l| format!("{l}\n")).collect()
}

#[test]
fn summary_proposes_a_name_and_a_rationale_for_each_speaker() {
    let (bench, id) = done_meeting();
    let r = bench.meeting(id);
    let seen: Vec<(String, Option<String>, String)> = r.speakers.iter().map(|l| (l.label.clone(), l.proposal.clone(), l.justification.clone())).collect();
    assert_eq!(
        seen,
        [
            ("SPEAKER_00".into(), Some("Alexandre".into()), "Khalid l'appelle Alexandre à 0:00:03.".into()),
            ("SPEAKER_01".into(), Some("Khalid".into()), "Répond à Alexandre à 0:00:03.".into()),
            ("SPEAKER_02".into(), Some("Alexandre".into()), "Même voix qu'Alexandre, qui relance à 0:00:06.".into()),
            ("SPEAKER_03".into(), None, "Ne dit que « Oui » à 0:00:09 ; personne ne le nomme.".into()),
        ]
    );
    assert_eq!(r.speakers[2].suggested_merge, Some(0));
    assert!(r.speakers.iter().all(|l| l.name.is_none()), "une proposition n'est pas un nom confirmé");

    let sheet = file(&bench, id, Role::Sheet);
    assert_eq!(
        block(&sheet, "propositions"),
        "propositions:\n  SPEAKER_00:\n    nom: \"Alexandre\"\n    justification: \"Khalid l'appelle Alexandre à 0:00:03.\"\n  SPEAKER_01:\n    nom: \"Khalid\"\n    justification: \"Répond à Alexandre à 0:00:03.\"\n  SPEAKER_02:\n    nom: \"Alexandre\"\n    justification: \"Même voix qu'Alexandre, qui relance à 0:00:06.\"\n    meme_voix_que: SPEAKER_00\n  SPEAKER_03:\n    nom: \"\"\n    justification: \"Ne dit que « Oui » à 0:00:09 ; personne ne le nomme.\"\n",
    );
}

/// Les lignes de répliques de la transcription lisible.
fn readable_utterances(md: &str) -> Vec<String> {
    md.lines().filter(|l| l.starts_with('[')).map(String::from).collect()
}

fn json(bench: &Bench, id: MeetingId) -> serde_json::Value {
    serde_json::from_str(&file(bench, id, Role::StructuredTranscription)).expect("transcription structurée")
}

#[test]
fn confirming_then_correcting_a_name_writes_it_to_the_sheet_and_the_readable_transcript() {
    let (mut bench, id) = done_meeting();

    bench.execute(Command::NameSpeaker { meeting: id, speaker: 0, name: "Alexandre".into() });
    bench.execute(Command::NameSpeaker { meeting: id, speaker: 1, name: "Khalid".into() });
    assert_eq!(block(&file(&bench, id, Role::Sheet), "locuteurs"), "locuteurs:\n  SPEAKER_00: \"Alexandre\"\n  SPEAKER_01: \"Khalid\"\n");

    // Corriger : un autre nom que celui proposé, espaces autour retirés.
    bench.execute(Command::NameSpeaker { meeting: id, speaker: 1, name: "  Jérôme ".into() });
    let r = bench.meeting(id);
    assert_eq!(r.speakers[1].name.as_deref(), Some("Jérôme"));
    assert_eq!(r.speakers[1].display_name(), "Jérôme");
    assert_eq!(block(&file(&bench, id, Role::Sheet), "locuteurs"), "locuteurs:\n  SPEAKER_00: \"Alexandre\"\n  SPEAKER_01: \"Jérôme\"\n");
    assert_eq!(
        readable_utterances(&file(&bench, id, Role::ReadableTranscription)),
        ["[0:00:00] Alexandre : Bonjour à tous.", "[0:00:03] Jérôme : Salut Alexandre.", "[0:00:06] SPEAKER_02 : On commence ?", "[0:00:09] SPEAKER_03 : Oui."]
    );

    // La transcription structurée garde les étiquettes, et donne la correspondance.
    let j = json(&bench, id);
    assert_eq!(j["utterances"][1]["speaker"], "SPEAKER_01");
    assert_eq!(j["words"][3]["speaker"], "SPEAKER_01");
    assert_eq!(j["names"], serde_json::json!({ "SPEAKER_00": "Alexandre", "SPEAKER_01": "Jérôme" }));

    let rejection = bench.greffier.execute(Command::NameSpeaker { meeting: id, speaker: 1, name: "  ".into() });
    assert!(rejection.is_err(), "un nom vide");
    assert!(bench.greffier.execute(Command::NameSpeaker { meeting: id, speaker: 9, name: "Odile".into() }).is_err(), "locuteur inconnu");
}

#[test]
fn merging_two_speakers_joins_them_everywhere() {
    let (mut bench, id) = done_meeting();
    bench.execute(Command::NameSpeaker { meeting: id, speaker: 0, name: "Alexandre".into() });
    let (alexandre, speaker_02) = (bench.meeting(id).speaking_time(0), bench.meeting(id).speaking_time(2));
    assert!(alexandre > 0.0 && speaker_02 > 0.0);

    bench.execute(Command::MergeSpeakers { meeting: id, speaker: 2, into: 0 });
    let r = bench.meeting(id);
    // Timeline et liste des locuteurs : une piste de moins, la réplique de SPEAKER_02 va à Alexandre.
    assert_eq!(r.active_speakers().collect::<Vec<_>>(), [0, 1, 3]);
    let displayed: Vec<usize> = r.utterances.iter().map(|x| r.displayed_speaker(x.speaker)).collect();
    assert_eq!(displayed, [0, 1, 0, 3]);
    // Temps de parole : celui de SPEAKER_02 s'ajoute à celui d'Alexandre.
    assert!((r.speaking_time(0) - (alexandre + speaker_02)).abs() < 1e-9, "le temps de parole de SPEAKER_02 va à Alexandre");
    let total: f64 = r.active_speakers().map(|i| r.speaking_time(i)).sum();
    assert!((total - 1.0).abs() < 1e-9, "les parts des locuteurs restants font 100 % ({total})");
    assert_eq!(r.speakers[2].suggested_merge, None, "la fusion proposée est faite");

    assert_eq!(
        readable_utterances(&file(&bench, id, Role::ReadableTranscription)),
        ["[0:00:00] Alexandre : Bonjour à tous.", "[0:00:03] SPEAKER_01 : Salut Alexandre.", "[0:00:06] Alexandre : On commence ?", "[0:00:09] SPEAKER_03 : Oui."]
    );
    let sheet = file(&bench, id, Role::Sheet);
    assert_eq!(block(&sheet, "locuteurs"), "locuteurs:\n  SPEAKER_00: \"Alexandre\"\n  SPEAKER_02: \"Alexandre\"\n", "les noms, fusions comprises");
    assert_eq!(block(&sheet, "fusions"), "fusions:\n  SPEAKER_02: SPEAKER_00\n");
    let j = json(&bench, id);
    assert_eq!(j["utterances"][2]["speaker"], "SPEAKER_02", "la transcription structurée garde l'étiquette d'origine");
    assert_eq!(j["speakers"], serde_json::json!(["SPEAKER_00", "SPEAKER_01", "SPEAKER_02", "SPEAKER_03"]));
    assert_eq!(j["merges"], serde_json::json!({ "SPEAKER_02": "SPEAKER_00" }));
    assert_eq!(j["names"], serde_json::json!({ "SPEAKER_00": "Alexandre", "SPEAKER_02": "Alexandre" }));
}

#[test]
fn a_named_speaker_merged_into_an_unnamed_speaker_gives_it_its_name() {
    let (mut bench, id) = done_meeting();
    bench.execute(Command::NameSpeaker { meeting: id, speaker: 0, name: "Alexandre".into() });
    bench.execute(Command::MergeSpeakers { meeting: id, speaker: 0, into: 2 });
    let r = bench.meeting(id);
    assert_eq!(r.active_speakers().collect::<Vec<_>>(), [1, 2, 3]);
    assert_eq!(r.speakers[2].name.as_deref(), Some("Alexandre"));
    assert_eq!(r.speakers[0].name, None, "le nom va à la voix qui reste");
    assert_eq!(readable_utterances(&file(&bench, id, Role::ReadableTranscription))[0], "[0:00:00] Alexandre : Bonjour à tous.");

    // Nommer un locuteur fusionné nomme celui qui le remplace.
    bench.execute(Command::NameSpeaker { meeting: id, speaker: 0, name: "Alex".into() });
    assert_eq!(bench.meeting(id).speakers[2].name.as_deref(), Some("Alex"));
    assert_eq!(block(&file(&bench, id, Role::Sheet), "locuteurs"), "locuteurs:\n  SPEAKER_00: \"Alex\"\n  SPEAKER_02: \"Alex\"\n");
}

#[test]
fn a_merge_never_makes_a_loop() {
    let (mut bench, id) = done_meeting();
    bench.execute(Command::MergeSpeakers { meeting: id, speaker: 1, into: 2 });
    bench.execute(Command::MergeSpeakers { meeting: id, speaker: 3, into: 1 });
    let r = bench.meeting(id);
    assert_eq!(r.active_speakers().collect::<Vec<_>>(), [0, 2]);
    assert_eq!(r.displayed_speaker(3), 2, "fusionné dans un locuteur déjà fusionné : va au bout");

    for (speaker, into) in [(1, 3), (2, 3), (3, 3), (1, 9)] {
        let rejection = bench.greffier.execute(Command::MergeSpeakers { meeting: id, speaker, into });
        assert!(rejection.is_err(), "{speaker} dans {into} : déjà la même voix ou inconnu");
    }
    let r = bench.meeting(id);
    assert_eq!(r.utterances.iter().map(|x| r.displayed_speaker(x.speaker)).collect::<Vec<_>>(), [0, 2, 2, 2]);
}

/// Nomme Alexandre et Khalid, fusionne SPEAKER_02 dans Alexandre.
fn named_and_merged_meeting() -> (Bench, MeetingId) {
    let (mut bench, id) = done_meeting();
    bench.execute(Command::NameSpeaker { meeting: id, speaker: 0, name: "Alexandre".into() });
    bench.execute(Command::NameSpeaker { meeting: id, speaker: 1, name: "Khalid".into() });
    bench.execute(Command::MergeSpeakers { meeting: id, speaker: 2, into: 0 });
    (bench, id)
}

#[test]
fn names_merges_and_proposals_persist_across_launches() {
    let (mut bench, id) = named_and_merged_meeting();
    let before = bench.meeting(id).speakers.clone();
    let name = bench.meeting(id).folder_name.clone();

    bench.reopen();
    assert_eq!(bench.named_meeting(&name).speakers, before);
}

#[test]
fn without_sheet_the_structured_transcription_is_enough_to_recover_names_and_merges() {
    let (mut bench, id) = named_and_merged_meeting();
    let name = bench.meeting(id).folder_name.clone();
    std::fs::remove_file(bench.greffier.folder().file(&name, Role::Sheet)).unwrap();

    bench.reopen();
    let r = bench.named_meeting(&name);
    let names: Vec<Option<&str>> = r.speakers.iter().map(|l| l.name.as_deref()).collect();
    assert_eq!(names, [Some("Alexandre"), Some("Khalid"), None, None]);
    assert_eq!(r.active_speakers().collect::<Vec<_>>(), [0, 1, 3]);
    assert_eq!(r.speaker_name(2), Some("Alexandre"));
}

#[test]
fn undoing_a_merge_gives_the_speaker_back_their_utterances() {
    let (mut bench, id) = named_and_merged_meeting();
    bench.execute(Command::SplitSpeaker { meeting: id, speaker: 2 });
    let r = bench.meeting(id);
    assert_eq!(r.active_speakers().collect::<Vec<_>>(), [0, 1, 2, 3]);
    assert_eq!(r.speakers[2].name, None);
    assert_eq!(r.speakers[2].proposal.as_deref(), Some("Alexandre"), "la proposition de l'agent reste");
    assert_eq!(readable_utterances(&file(&bench, id, Role::ReadableTranscription))[2], "[0:00:06] SPEAKER_02 : On commence ?");
    assert!(!file(&bench, id, Role::Sheet).contains("fusions:"));

    assert!(bench.greffier.execute(Command::SplitSpeaker { meeting: id, speaker: 2 }).is_err(), "il n'est plus fusionné");
}

#[test]
fn search_follows_names_and_merges() {
    let speaker = |bench: &Bench| bench.greffier.search("commence")[0].speaker.clone();
    let (mut bench, id) = done_meeting();
    assert_eq!(speaker(&bench).as_deref(), Some("Alexandre ?"), "la proposition de l'agent, à confirmer");

    bench.execute(Command::MergeSpeakers { meeting: id, speaker: 2, into: 1 });
    bench.execute(Command::NameSpeaker { meeting: id, speaker: 2, name: "Odile".into() });
    assert_eq!(speaker(&bench).as_deref(), Some("Odile"), "la voix qui le remplace, avec le nom donné");

    bench.execute(Command::SplitSpeaker { meeting: id, speaker: 2 });
    assert_eq!(speaker(&bench).as_deref(), Some("Alexandre ?"));
}
