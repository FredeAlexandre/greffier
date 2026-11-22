//! The Transcription stage: the lexicon vocabulary given to the transcriber, bracketed noises
//! removed, a single speaker as long as diarization is not plugged in, splitting into utterances,
//! the files written, the queue (rank, progress, remaining time), stopping, failure and retry. The
//! names are made up.

mod bench;

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use bench::{Bench, word};
use greffier_core::adapters::{Adapters, SingleSpeaker};
use greffier_core::folder::Import;
use greffier_core::fake::{FakeDiarizer, FakeTranscriber};
use greffier_core::{Command, DateTime, Stage, Event, MeetingId, Word, Settings, Role, Status};

/// The fixtures' video (12 s).
fn video() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/meetings-folder/2026-11-19 15h30 - Réunion/2026-11-19 15h30 - Réunion - enregistrement.mp4")
}

/// The bench adapters, with this transcriber and without diarization.
fn with_transcriber(t: FakeTranscriber) -> Adapters {
    let mut a = bench::adapters();
    a.transcriber = Box::new(t);
    a.diarizer = Box::new(SingleSpeaker);
    a
}

fn import_media(bench: &mut Bench) -> MeetingId {
    let import = Import { recording: video(), transcription: None, summary: None, start: DateTime::new(2026, 10, 2, 9, 30), title: "Point Ormeau".into() };
    let ev = bench.execute(Command::Import(import));
    ev.iter().find_map(|e| if let Event::MeetingImported(id) = e { Some(*id) } else { None }).expect("imported meeting")
}

fn texts(bench: &Bench, id: MeetingId) -> Vec<String> {
    bench.meeting(id).utterances.iter().map(|r| r.text.clone()).collect()
}

fn file(bench: &Bench, id: MeetingId, role: Role) -> String {
    let r = bench.meeting(id);
    bench.read(&bench.greffier.folder().file(&r.folder_name, role))
}

fn write_lexicon(bench: &Bench, vocabulary: &str) -> PathBuf {
    let app = bench.folder.with_extension("app");
    std::fs::create_dir_all(&app).expect("app folder");
    let path = app.join("lexique.md");
    let md = format!("# Lexique\n\n## Personnes\n\n- Bérénice Ormeau : cheffe de projet\n\n## Mots justes\n\nLumion\n\n## Erreurs connues\n\n| Entendu | Correct |\n|---|---|\n| lumi on | Lumion |\n\n## Vocabulaire\n\n{vocabulary}\n");
    std::fs::write(&path, md).expect("lexicon");
    path
}

fn process_transcription(bench: &mut Bench, id: MeetingId) {
    bench.execute(Command::Process(id));
    bench.advance_until(id, |s| matches!(s, Status::Running { stage: Stage::Review, .. } | Status::Failed { .. } | Status::Done));
}

// --- Vocabulary ----------------------------------------------------------------------------

#[test]
fn lexicon_vocabulary_section_is_given_to_the_transcriber_on_every_transcription() {
    let seen = Rc::new(RefCell::new(Vec::<String>::new()));
    let v = seen.clone();
    let mut bench = Bench::with(with_transcriber(FakeTranscriber::new(1.0, move |d| {
        v.borrow_mut().push(d.vocabulary.clone());
        Ok(bench::words())
    })));
    let lexicon = write_lexicon(&bench, "Réunion Ormeau avec Bérénice\net Gaspard.  Projet Lumion.");
    bench.execute(Command::ChangeSettings(Settings { lexicon: Some(lexicon.clone()), ..bench.greffier.settings().clone() }));
    let id = import_media(&mut bench);

    process_transcription(&mut bench, id);
    assert_eq!(*seen.borrow(), ["Réunion Ormeau avec Bérénice et Gaspard. Projet Lumion."]);

    // The lexicon is reread: a change applies to the next transcription.
    bench.advance_until(id, |s| *s == Status::Done);
    write_lexicon(&bench, "Réunion Ormeau avec Gaspard.");
    bench.execute(Command::Redo { meeting: id, stage: Stage::Transcription });
    bench.advance_until(id, |s| matches!(s, Status::Running { stage: Stage::Review, .. }));
    assert_eq!(seen.borrow()[1], "Réunion Ormeau avec Gaspard.");
}

#[test]
fn without_lexicon_the_transcriber_has_no_vocabulary() {
    let seen = Rc::new(RefCell::new(Vec::<String>::new()));
    let v = seen.clone();
    let mut bench = Bench::with(with_transcriber(FakeTranscriber::new(1.0, move |d| {
        v.borrow_mut().push(d.vocabulary.clone());
        Ok(bench::words())
    })));
    let absent = bench.folder.with_extension("app").join("lexique.md");
    bench.execute(Command::ChangeSettings(Settings { lexicon: Some(absent), ..bench.greffier.settings().clone() }));
    let id = import_media(&mut bench);
    process_transcription(&mut bench, id);
    assert_eq!(*seen.borrow(), [""]);
}

#[test]
fn transcriber_receives_the_imported_media_and_its_duration() {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let v = seen.clone();
    let mut bench = Bench::with(with_transcriber(FakeTranscriber::new(1.0, move |d| {
        v.borrow_mut().push((d.audio.clone(), d.duration));
        Ok(bench::words())
    })));
    let id = import_media(&mut bench);
    let name = bench.meeting(id).folder_name.clone();
    process_transcription(&mut bench, id);
    let (audio, duration) = seen.borrow()[0].clone();
    assert_eq!(audio, bench.greffier.folder().recording(&name).expect("imported recording"));
    assert!((duration - 12.0).abs() < 0.1, "{duration}");
}

// --- Words and utterances ------------------------------------------------------------------

#[test]
fn bracketed_noises_and_empty_words_are_removed() {
    let words = vec![
        word(0.0, 1.0, "[Musique]"),
        word(1.0, 1.4, "Bonjour"),
        word(1.4, 1.6, "(rires)"),
        word(1.6, 1.8, "Gaspard."),
        word(1.8, 1.9, "  "),
        word(2.0, 2.2, "[bruit"),
        word(2.2, 2.4, "de"),
        word(2.4, 2.6, "fond]"),
        word(2.6, 2.9, "On"),
        word(2.9, 3.3, "commence."),
        // A parenthesis that does not close within 7 words is not a noise.
        word(3.4, 3.6, "(enfin"),
        word(3.6, 3.8, "si"),
        word(3.8, 4.0, "tout"),
        word(4.0, 4.2, "le"),
        word(4.2, 4.4, "monde"),
        word(4.4, 4.6, "est"),
        word(4.6, 4.8, "là."),
        // Nor is one left open at the end.
        word(4.9, 5.0, "[applaudissements"),
    ];
    let mut bench = Bench::with(with_transcriber(FakeTranscriber::fixed(1.0, words)));
    let id = import_media(&mut bench);
    process_transcription(&mut bench, id);

    let r = bench.meeting(id);
    let kept: Vec<&str> = r.words.iter().map(|m| m.text.as_str()).collect();
    assert_eq!(kept, ["Bonjour", "Gaspard.", "On", "commence.", "(enfin", "si", "tout", "le", "monde", "est", "là.", "[applaudissements"]);
    assert_eq!(texts(&bench, id), ["Bonjour Gaspard. On commence. (enfin si tout le monde est là. [applaudissements"]);
}

#[test]
fn without_diarization_all_words_go_to_a_single_speaker() {
    let words = vec![word(0.0, 0.5, "Bonjour"), word(0.5, 1.0, "Bérénice."), word(1.2, 1.6, "Salut"), word(1.6, 2.0, "Gaspard.")];
    let mut bench = Bench::with(with_transcriber(FakeTranscriber::fixed(1.0, words)));
    let id = import_media(&mut bench);
    process_transcription(&mut bench, id);

    let r = bench.meeting(id);
    let labels: Vec<&str> = r.speakers.iter().map(|l| l.label.as_str()).collect();
    assert_eq!(labels, ["SPEAKER_00"]);
    assert!(r.utterances.iter().all(|x| x.speaker == 0));
    assert!(r.turns.is_empty());
}

#[test]
fn an_utterance_starts_after_a_2_s_pause() {
    let words = vec![
        word(0.0, 0.5, "On"),
        word(0.5, 1.0, "démarre."),
        word(2.9, 3.2, "Lumion"), // 1.9 s pause: same utterance
        word(3.2, 3.5, "avance."),
        word(5.5, 6.0, "Ensuite"), // 2 s pause: new utterance
        word(6.0, 6.5, "Ormeau."),
    ];
    let mut bench = Bench::with(with_transcriber(FakeTranscriber::fixed(1.0, words)));
    let id = import_media(&mut bench);
    process_transcription(&mut bench, id);

    assert_eq!(texts(&bench, id), ["On démarre. Lumion avance.", "Ensuite Ormeau."]);
    let r = bench.meeting(id);
    assert_eq!((r.utterances[0].start, r.utterances[0].end), (0.0, 3.5));
    assert_eq!((r.utterances[1].start, r.utterances[1].end), (5.5, 6.5));
}

#[test]
fn an_utterance_stops_at_the_first_sentence_end_past_220_characters() {
    // « Bérénice » + 25 × « explique » = 233 characters, without a sentence end.
    let mut words = vec![word(0.0, 0.2, "Bérénice")];
    let mut t = 0.2;
    let mut say = |text: &str, words: &mut Vec<Word>| {
        words.push(word(t, t + 0.2, text));
        t += 0.2;
    };
    for _ in 0..25 {
        say("explique", &mut words);
    }
    say("tout.", &mut words); // sentence end past 220 characters: the utterance stops after it
    say("Gaspard", &mut words);
    say("répond.", &mut words); // sentence end, but short utterance: keep going
    say("Puis", &mut words);
    say("Lumion.", &mut words);
    let mut bench = Bench::with(with_transcriber(FakeTranscriber::fixed(1.0, words)));
    let id = import_media(&mut bench);
    process_transcription(&mut bench, id);

    let t = texts(&bench, id);
    assert_eq!(t.len(), 2, "{t:?}");
    assert!(t[0].starts_with("Bérénice explique") && t[0].ends_with("explique tout."), "{}", t[0]);
    assert_eq!(t[1], "Gaspard répond. Puis Lumion.");
}

#[test]
fn lone_punctuation_stays_in_the_previous_utterance() {
    let words = vec![word(0.0, 0.4, "Ça"), word(0.4, 0.8, "va"), word(0.8, 0.9, "?"), word(1.5, 1.9, "Oui.")];
    let turns = vec![bench::turn(0.0, 0.85, "SPEAKER_00"), bench::turn(0.85, 2.0, "SPEAKER_01")];
    let mut a = with_transcriber(FakeTranscriber::fixed(1.0, words));
    a.diarizer = Box::new(FakeDiarizer::fixed(1.0, turns));
    let mut bench = Bench::with(a);
    let id = import_media(&mut bench);
    process_transcription(&mut bench, id);
    assert_eq!(texts(&bench, id), ["Ça va ?", "Oui."]);
}

// --- Files ---------------------------------------------------------------------------------

#[test]
fn a_media_imported_alone_produces_both_transcriptions_and_the_sheet_tells_its_progress() {
    let words = vec![Word { start: 0.5, end: 0.9, text: "Bonjour".into(), confidence: 0.873, speaker: None }, word(0.9, 1.4, "Bérénice."), word(4.0, 4.5, "Lumion"), word(4.5, 5.0, "démarre.")];
    let mut bench = Bench::with(with_transcriber(FakeTranscriber::fixed(1.0, words)));
    let id = import_media(&mut bench);
    process_transcription(&mut bench, id);

    let json: serde_json::Value = serde_json::from_str(&file(&bench, id, Role::StructuredTranscription)).expect("JSON");
    assert_eq!(json["speakers"], serde_json::json!(["SPEAKER_00"]));
    let first = &json["words"][0];
    assert_eq!((first["start"].as_f64(), first["end"].as_f64(), first["text"].as_str()), (Some(0.5), Some(0.9), Some("Bonjour")));
    assert!((first["confidence"].as_f64().unwrap_or(0.0) - 0.873).abs() < 1e-6, "{first}");
    assert_eq!(first["speaker"], "SPEAKER_00");
    assert_eq!(json["words"].as_array().map(Vec::len), Some(4));
    assert_eq!(json["utterances"][0]["text"], "Bonjour Bérénice.");
    assert_eq!(json["utterances"][1]["text"], "Lumion démarre.");
    assert!(json.get("review").is_none(), "not reviewed yet");

    let md = file(&bench, id, Role::ReadableTranscription);
    assert!(md.contains("[0:00:00] SPEAKER_00 : Bonjour Bérénice."), "{md}");
    assert!(md.contains("[0:00:04] SPEAKER_00 : Lumion démarre."), "{md}");

    let sheet = file(&bench, id, Role::Sheet);
    assert!(sheet.contains("statut: en_cours\n"), "{sheet}");
    assert!(sheet.contains("etape: relecture\n"), "{sheet}");
    assert!(sheet.contains("etape_faite: transcription\n"), "{sheet}");
}

#[test]
fn during_transcription_the_sheet_says_transcription_running() {
    let mut bench = Bench::with(with_transcriber(FakeTranscriber::fixed(3.0, bench::words())));
    let id = import_media(&mut bench);
    bench.execute(Command::Process(id));
    bench.advance(1.0);
    let sheet = file(&bench, id, Role::Sheet);
    assert!(sheet.contains("statut: en_cours\netape: transcription\n"), "{sheet}");
}

// --- Queue, stop, failure ------------------------------------------------------------------

#[test]
fn transcription_progress_is_that_of_the_slowest_half() {
    let mut a = with_transcriber(FakeTranscriber::fixed(4.0, bench::words()));
    a.diarizer = Box::new(FakeDiarizer::fixed(1.0, bench::turns()));
    let mut bench = Bench::with(a);
    let id = import_media(&mut bench);
    bench.execute(Command::Process(id));
    bench.advance(2.0);
    let p = bench.meeting(id).status.progress();
    assert!((p - 0.5).abs() < 0.05, "{p}");
}

#[test]
fn remaining_time_counts_meetings_ahead_in_line() {
    let mut bench = Bench::with(with_transcriber(FakeTranscriber::fixed(1.0, bench::words())));
    let first = import_media(&mut bench);
    let second = import_media(&mut bench);
    bench.execute(Command::Process(first));
    bench.execute(Command::Process(second));
    bench.advance(0.1);

    assert_eq!((bench.greffier.rank(first), bench.greffier.rank(second)), (Some(0), Some(1)));
    let own = |id| bench.meeting(id).estimated_time_remaining().expect("in the queue");
    let (a, b) = (bench.greffier.time_remaining(first).expect("in progress"), bench.greffier.time_remaining(second).expect("waiting"));
    assert_eq!(a, own(first));
    assert!((b - (a + own(second))).abs() < 1e-9, "{b} ≠ {a} + {}", own(second));
}

#[test]
fn a_meeting_waiting_at_a_later_stage_has_less_to_wait() {
    let mut bench = Bench::new();
    let done = bench.meeting_to_process();
    bench.execute(Command::Process(done));
    bench.advance_until(done, |s| *s == Status::Done);
    let fresh = bench.meeting_to_process();
    // During a recording, the queue waits: both stay waiting.
    bench.execute(Command::StartRecording);
    bench.execute(Command::Redo { meeting: done, stage: Stage::Summary });
    bench.execute(Command::Process(fresh));
    assert_eq!(bench.meeting(done).status, Status::Pending(Stage::Summary));
    assert_eq!(bench.meeting(fresh).status, Status::Pending(Stage::Transcription));

    let summary_only = bench.meeting(done).estimated_time_remaining().expect("waiting");
    let all = bench.meeting(fresh).estimated_time_remaining().expect("waiting");
    assert!(summary_only < all / 2.0, "{summary_only} / {all}");
}

#[test]
fn stopping_transcription_stops_the_transcriber_and_puts_it_back_to_process() {
    let mut bench = Bench::with(with_transcriber(FakeTranscriber::fixed(3.0, bench::words())));
    let id = import_media(&mut bench);
    bench.execute(Command::Process(id));
    bench.advance(1.0);
    bench.execute(Command::StopProcessing(id));

    assert_eq!(bench.meeting(id).status, Status::ToProcess);
    assert_eq!(bench.meeting(id).completed_stage, None);
    assert!(!bench.greffier.busy());
    let sheet = file(&bench, id, Role::Sheet);
    assert!(sheet.contains("statut: a_traiter\n"), "{sheet}");
    assert!(!bench.greffier.folder().present_roles(&bench.meeting(id).folder_name).contains(&Role::StructuredTranscription));
}

#[test]
fn a_failed_transcription_keeps_its_message_and_processing_retries_it() {
    let attempts = Rc::new(RefCell::new(0));
    let n = attempts.clone();
    let mut bench = Bench::with(with_transcriber(FakeTranscriber::new(1.0, move |_| {
        *n.borrow_mut() += 1;
        if *n.borrow() == 1 { Err("Le média n'a pas de son.".into()) } else { Ok(vec![word(0.0, 0.5, "Lumion"), word(0.5, 1.0, "redémarre.")]) }
    })));
    let id = import_media(&mut bench);
    bench.execute(Command::Process(id));
    bench.advance_until(id, |s| matches!(s, Status::Failed { .. }));

    assert_eq!(bench.meeting(id).status, Status::Failed { stage: Stage::Transcription, message: "Le média n'a pas de son.".into() });
    let sheet = file(&bench, id, Role::Sheet);
    assert!(sheet.contains("statut: echec\netape: transcription\nerreur: \"Le média n'a pas de son.\"\n"), "{sheet}");

    bench.execute(Command::Process(id));
    assert_eq!(bench.meeting(id).status, Status::Pending(Stage::Transcription));
    bench.advance_until(id, |s| matches!(s, Status::Running { stage: Stage::Review, .. }));
    assert_eq!(*attempts.borrow(), 2);
    assert_eq!(bench.meeting(id).completed_stage, Some(Stage::Transcription));
    assert_eq!(texts(&bench, id), ["Lumion redémarre."]);
}
