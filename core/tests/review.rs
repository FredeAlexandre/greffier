//! La relecture par l'agent : ce que Greffier lui envoie, et comment il applique lui-même son
//! résultat (corrections sûres, changements de locuteur, passages corrigés après réécoute ou à
//! vérifier, lexique enrichi).

mod bench;

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use bench::{Bench, word, turn};
use greffier_core::adapters::{Adapters, SpeakerChange, ConfidentCorrection, ReviewRequest, CorrectedPassage, FlaggedPassage, ReviewResult};
use greffier_core::fake::{FakeAgent, FakeDiarizer, FakeTranscriber};
use greffier_core::lexicon::LexiconAddition;
use greffier_core::{Command, Stage, MeetingId, Settings, Status};

const LEXICON: &str = "---\ntype: lexique\n---\n\n# Lexique\n\n## Personnes\n\n- Alice : cheffe de projet\n\n## Mots justes\n\nZorglub.\n\n## Erreurs connues\n\n| Entendu | Correct |\n|---|---|\n| Zorglube | Zorglub |\n\n## Vocabulaire\n\nRéunion sur Zorglub.\n";

/// Trois répliques : Alice ouvre, Bob répond, Alice reprend (mais la séparation l'a donnée à Bob).
fn adapters(review: impl FnMut(&ReviewRequest) -> Result<ReviewResult, String> + 'static) -> Adapters {
    let words = vec![
        word(0.0, 0.5, "Bonjour,"),
        word(0.5, 1.0, "on"),
        word(1.0, 1.4, "parle"),
        word(1.4, 1.8, "de"),
        word(1.8, 2.5, "Zorglube."),
        word(5.0, 5.4, "La"),
        word(5.4, 6.0, "recète"),
        word(6.0, 6.5, "est"),
        word(6.5, 7.0, "prête ?"),
        word(9.2, 9.6, "Pas"),
        word(9.6, 10.0, "encore,"),
        word(10.0, 10.4, "jeudi"),
        word(10.4, 11.0, "peut-être."),
    ];
    let turns = vec![turn(0.0, 2.6, "SPEAKER_00"), turn(4.9, 11.1, "SPEAKER_01")];
    let mut a = bench::adapters();
    a.transcriber = Box::new(FakeTranscriber::fixed(bench::STAGE_DURATION, words));
    a.diarizer = Box::new(FakeDiarizer::fixed(bench::STAGE_DURATION, turns));
    a.agent = Box::new(FakeAgent::new(bench::STAGE_DURATION, review, bench::STAGE_DURATION, |_| Ok((bench::SUMMARY.to_string(), bench::summary_result()))));
    a
}

/// Un lexique dans un fichier temporaire, branché dans les réglages.
fn with_lexicon(bench: &mut Bench, md: &str) -> PathBuf {
    let path = bench.folder.with_extension("lexique.md");
    std::fs::write(&path, md).expect("lexique");
    bench.execute(Command::ChangeSettings(Settings { lexicon: Some(path.clone()), ..bench.greffier.settings().clone() }));
    path
}

/// Traite une réunion jusqu'à la fin de la relecture.
fn reread(bench: &mut Bench) -> MeetingId {
    let id = bench.meeting_to_process();
    bench.execute(Command::SpeakerCount { meeting: id, number: Some(2) });
    bench.execute(Command::Process(id));
    bench.advance_until(id, |s| matches!(s, Status::Running { stage: Stage::Summary, .. } | Status::Failed { .. } | Status::Done));
    id
}

#[test]
fn agent_receives_utterances_lexicon_and_context() {
    let received = Rc::new(RefCell::new(None));
    let r = received.clone();
    let mut bench = Bench::with(adapters(move |d| {
        *r.borrow_mut() = Some(d.clone());
        Ok(ReviewResult::default())
    }));
    with_lexicon(&mut bench, LEXICON);
    let id = reread(&mut bench);

    let d = received.borrow().clone().expect("relecture demandée");
    let texts: Vec<(&str, &str)> = d.utterances.iter().map(|x| (x.speaker.as_str(), x.text.as_str())).collect();
    assert_eq!(texts, [("SPEAKER_00", "Bonjour, on parle de Zorglube."), ("SPEAKER_01", "La recète est prête ?"), ("SPEAKER_01", "Pas encore, jeudi peut-être.")]);
    assert_eq!(d.lexicon, LEXICON);
    assert_eq!(d.vocabulary, "Réunion sur Zorglub.");
    assert_eq!(d.speaker_count, Some(2));
    assert_eq!(d.meetings_folder, bench.folder);
    assert_eq!(d.folder_name, bench.meeting(id).folder_name);
    assert_eq!(d.start, bench.meeting(id).start);
    assert!(d.recording.ends_with("2026-11-21 14h02 - Réunion - enregistrement.mp4"), "{}", d.recording.display());
}

fn correction(utterance: usize, before: &str, after: &str) -> ConfidentCorrection {
    ConfidentCorrection { utterance, before: before.into(), after: after.into() }
}

fn change(utterance: usize, speaker: &str) -> SpeakerChange {
    SpeakerChange { utterance, speaker: speaker.into() }
}

#[test]
fn sure_corrections_and_proven_speaker_changes_are_applied() {
    let mut bench = Bench::with(adapters(|_| {
        Ok(ReviewResult {
            session: Some("relecture-1".into()),
            corrections: vec![correction(1, "recète", "recette"), correction(0, "", "rien"), correction(0, "Bonjour", "Bonjour"), correction(2, "absent", "x"), correction(9, "La", "Le"), correction(0, "Zorglube", "Zorglub")],
            speaker_changes: vec![change(2, "SPEAKER_00"), change(1, "SPEAKER_07"), change(0, "SPEAKER_00"), change(5, "SPEAKER_01")],
            ..Default::default()
        })
    }));
    let id = reread(&mut bench);
    let r = bench.meeting(id);

    let texts: Vec<(&str, &str)> = r.utterances.iter().map(|x| (r.speakers[x.speaker].label.as_str(), x.text.as_str())).collect();
    assert_eq!(texts, [("SPEAKER_00", "Bonjour, on parle de Zorglub."), ("SPEAKER_01", "La recette est prête ?"), ("SPEAKER_00", "Pas encore, jeudi peut-être.")]);
    assert_eq!(r.speakers.len(), 2, "un locuteur inconnu n'est pas créé");
    let log = r.review.as_ref().expect("journal de la relecture");
    assert_eq!(log.corrections, [correction(1, "recète", "recette"), correction(0, "Zorglube", "Zorglub")], "seules les corrections appliquées");
    assert_eq!(log.speaker_changes, [change(2, "SPEAKER_00")]);
    assert_eq!(r.corrections, 2);
    let raw: Vec<(usize, &str)> = r.raw.iter().map(|x| (x.speaker, x.text.as_str())).collect();
    assert_eq!(raw, [(0, "Bonjour, on parle de Zorglube."), (1, "La recète est prête ?"), (1, "Pas encore, jeudi peut-être.")]);
    assert_eq!(r.sessions.review.as_deref(), Some("relecture-1"));
}

fn addition(heard: &str, correct: &str) -> LexiconAddition {
    LexiconAddition { heard: heard.into(), correct: correct.into() }
}

/// Une relecture qui corrige un passage après réécoute et en laisse deux à vérifier.
fn review_with_passages(_: &ReviewRequest) -> Result<ReviewResult, String> {
    Ok(ReviewResult {
        session: Some("relecture-2".into()),
        corrections: vec![correction(1, "recète", "recette")],
        corrected_passages: vec![
            CorrectedPassage { utterance: 2, excerpt: "jeudi".into(), after: "mardi".into(), relisten: "pas encore mardi peut-être".into() },
            // Confirmé tel quel : rien à changer.
            CorrectedPassage { utterance: 0, excerpt: "on parle".into(), after: "on parle".into(), relisten: "on parle".into() },
            // L'extrait a disparu sous une correction sûre : il reste à vérifier.
            CorrectedPassage { utterance: 1, excerpt: "recète".into(), after: "requête".into(), relisten: "la requête est prête".into() },
        ],
        passages_to_check: vec![FlaggedPassage { utterance: 0, excerpt: "Zorglube".into(), hypothesis: Some("?".into()), relisten: "on parle de Zorglub".into() }],
        lexicon_additions: vec![addition("recète", "recette"), addition("Zorglube", "Zorglub")],
        ..Default::default()
    })
}

#[test]
fn doubtful_passages_are_corrected_after_relisten_or_left_to_check() {
    let mut bench = Bench::with(adapters(review_with_passages));
    let id = reread(&mut bench);
    let r = bench.meeting(id);

    let texts: Vec<&str> = r.utterances.iter().map(|x| x.text.as_str()).collect();
    assert_eq!(texts, ["Bonjour, on parle de Zorglube.", "La recette est prête ?", "Pas encore, mardi peut-être."]);
    let log = r.review.as_ref().expect("journal");
    assert_eq!(log.corrected_passages, [CorrectedPassage { utterance: 2, excerpt: "jeudi".into(), after: "mardi".into(), relisten: "pas encore mardi peut-être".into() }]);
    assert_eq!(r.corrections, 2, "une correction sûre et un passage corrigé");

    let passages: Vec<(usize, &str, Option<&str>, &str)> = r.passages.iter().map(|p| (p.utterance, p.excerpt.as_str(), p.hypothesis.as_deref(), p.relisten.as_str())).collect();
    assert_eq!(passages, [(0, "Zorglube", None, "on parle de Zorglub"), (1, "recète", Some("requête"), "la requête est prête")]);
    assert_eq!(r.pending_passages(), 2);
    assert_eq!((r.utterances[0].passages.as_slice(), r.utterances[1].passages.as_slice(), r.utterances[2].passages.as_slice()), (&[0][..], &[1][..], &[][..]));
}

#[test]
fn several_doubtful_passages_in_one_utterance_are_all_kept() {
    let mut bench = Bench::with(adapters(|_| {
        Ok(ReviewResult {
            passages_to_check: vec![
                FlaggedPassage { utterance: 2, excerpt: "Pas encore".into(), hypothesis: None, relisten: String::new() },
                FlaggedPassage { utterance: 2, excerpt: "jeudi".into(), hypothesis: Some("mardi".into()), relisten: String::new() },
                // Le même passage signalé deux fois n'en fait qu'un.
                FlaggedPassage { utterance: 2, excerpt: "jeudi".into(), hypothesis: Some("mardi".into()), relisten: String::new() },
            ],
            ..Default::default()
        })
    }));
    let id = reread(&mut bench);
    let r = bench.meeting(id);
    let passages: Vec<(usize, &str)> = r.passages.iter().map(|p| (p.utterance, p.excerpt.as_str())).collect();
    assert_eq!(passages, [(2, "Pas encore"), (2, "jeudi")]);
    assert_eq!(r.utterances[2].passages, [0, 1]);

    // Chacun se tranche, et les deux se retrouvent au lancement suivant.
    bench.execute(Command::Resolve { meeting: id, passage: 1, resolution: greffier_core::Resolution::AcceptHypothesis });
    assert_eq!(bench.meeting(id).utterances[2].text, "Pas encore, mardi peut-être.");
    let name = bench.meeting(id).folder_name.clone();
    bench.reopen();
    let r = bench.named_meeting(&name);
    assert_eq!(r.passages.len(), 2);
    assert_eq!(r.utterances[2].passages, [0, 1]);
    assert_eq!(r.pending_passages(), 1);
}

#[test]
fn review_writes_the_raw_transcript_log_passages_and_session() {
    let mut bench = Bench::with(adapters(review_with_passages));
    let id = reread(&mut bench);
    let name = bench.meeting(id).folder_name.clone();
    let file = |role| bench.greffier.folder().file(&name, role);

    let json: serde_json::Value = serde_json::from_str(&bench.read(&file(greffier_core::Role::StructuredTranscription))).expect("json");
    assert_eq!(json["raw"][1]["text"], "La recète est prête ?");
    assert_eq!(json["utterances"][1]["text"], "La recette est prête ?");
    assert_eq!(json["review"]["corrections"][0]["after"], "recette");
    assert_eq!(json["review"]["corrected_passages"][0]["after"], "mardi");
    assert_eq!(json["passages"][1]["excerpt"], "recète");
    assert_eq!(json["passages"][1]["hypothesis"], "requête");
    assert_eq!(json["passages"][1]["relisten"], "la requête est prête");
    assert_eq!(json["passages"][1]["status"], "pending");

    let readable = bench.read(&file(greffier_core::Role::ReadableTranscription));
    assert!(readable.contains("SPEAKER_01 : La recette est prête ?\n> À vérifier : « recète » (hypothèse : « requête »)\n"), "{readable}");
    let sheet = bench.read(&file(greffier_core::Role::Sheet));
    assert!(sheet.contains("relecture-2"), "{sheet}");

    // Relue au lancement suivant, la réunion est la même.
    let before = bench.meeting(id).clone();
    bench.reopen();
    let r = bench.named_meeting(&name);
    assert_eq!((&r.utterances, &r.raw, &r.passages, &r.review, r.corrections), (&before.utterances, &before.raw, &before.passages, &before.review, before.corrections));
    assert_eq!(r.sessions.review.as_deref(), Some("relecture-2"));
}

#[test]
fn lexicon_learns_the_errors_it_does_not_know_yet() {
    let mut bench = Bench::with(adapters(review_with_passages));
    let lexicon = with_lexicon(&mut bench, LEXICON);
    let id = reread(&mut bench);

    let md = bench.read(&lexicon);
    assert!(md.contains("| Entendu | Correct |\n|---|---|\n| Zorglube | Zorglub |\n| recète | recette |\n\n## Vocabulaire"), "{md}");
    assert_eq!(bench.meeting(id).review.as_ref().expect("journal").lexicon_additions, [addition("recète", "recette")], "seul le nouvel ajout est au journal");
}

#[test]
fn redoing_the_review_restarts_from_the_raw_transcription() {
    let runs = Rc::new(RefCell::new(0));
    let p = runs.clone();
    let mut bench = Bench::with(adapters(move |d| {
        *p.borrow_mut() += 1;
        // La seconde relecture voit la transcription brute, pas la première relecture.
        assert_eq!(d.utterances[1].text, "La recète est prête ?");
        let after = if *p.borrow() == 1 { "recette" } else { "requête" };
        Ok(ReviewResult { corrections: vec![correction(1, "recète", after)], ..Default::default() })
    }));
    let id = reread(&mut bench);
    bench.advance_until(id, |s| *s == Status::Done);
    assert_eq!(bench.meeting(id).utterances[1].text, "La recette est prête ?");

    bench.execute(Command::Redo { meeting: id, stage: Stage::Review });
    bench.advance_until(id, |s| *s == Status::Done);
    let r = bench.meeting(id);
    assert_eq!(*runs.borrow(), 2);
    assert_eq!(r.utterances[1].text, "La requête est prête ?");
    assert_eq!(r.raw[1].text, "La recète est prête ?");
    assert_eq!(r.corrections, 1);
}

#[test]
fn if_the_agent_fails_the_stage_fails_with_its_message() {
    let mut bench = Bench::with(adapters(|_| Err("La CLI `claude` est introuvable : installer Claude Code.".into())));
    let id = reread(&mut bench);
    let r = bench.meeting(id);
    assert_eq!(r.status, Status::Failed { stage: Stage::Review, message: "La CLI `claude` est introuvable : installer Claude Code.".into() });
    assert_eq!(r.utterances[1].text, "La recète est prête ?", "la transcription reste celle de Whisper");
    assert!(r.review.is_none());
}
