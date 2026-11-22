//! La relecture par la CLI `claude` (claude::relecture) : la première passe, la réécoute des
//! passages douteux, la deuxième passe dans la même session, et la traduction de leurs réponses
//! en résultat pour le cœur. La CLI et la réécoute sont remplacées par des faux.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;

use greffier_core::DateTime;
use greffier_core::adapters::{SpeakerChange, ConfidentCorrection, ReviewRequest, CorrectedPassage, FlaggedPassage, AgentUtterance};
use greffier_tools::claude::review::{self, Relisten};
use greffier_tools::claude::{self, ClaudeCall, Claude, Session};
use greffier_core::lexicon::LexiconAddition;
use serde_json::{Value, json};

#[derive(Default)]
struct FakeClaude {
    responses: Mutex<Vec<Result<Value, String>>>,
    calls: Mutex<Vec<ClaudeCall>>,
}

impl FakeClaude {
    fn answering(responses: Vec<Result<Value, String>>) -> FakeClaude {
        FakeClaude { responses: Mutex::new(responses), calls: Mutex::default() }
    }
    fn calls(&self) -> Vec<ClaudeCall> {
        self.calls.lock().unwrap().clone()
    }
}

impl Claude for FakeClaude {
    fn ask(&self, call: &ClaudeCall, _: &AtomicBool) -> Result<Value, String> {
        self.calls.lock().unwrap().push(call.clone());
        let mut r = self.responses.lock().unwrap();
        if r.is_empty() { Err("appel de trop".into()) } else { r.remove(0) }
    }
}

/// Une demande de réécoute : l'enregistrement, les extraits, le vocabulaire.
type RelistenRequest = (PathBuf, Vec<(f64, f64)>, String);

#[derive(Default)]
struct FakeRelisten {
    texts: Vec<String>,
    failure: Option<String>,
    requests: Mutex<Vec<RelistenRequest>>,
}

impl Relisten for FakeRelisten {
    fn relisten(&self, recording: &Path, excerpts: &[(f64, f64)], vocabulary: &str, _: &AtomicBool) -> Result<Vec<String>, String> {
        self.requests.lock().unwrap().push((recording.to_path_buf(), excerpts.to_vec(), vocabulary.to_string()));
        match &self.failure {
            Some(e) => Err(e.clone()),
            None => Ok(self.texts.iter().take(excerpts.len()).cloned().collect()),
        }
    }
}

fn utterance(number: usize, speaker: &str, start: f64, end: f64, text: &str) -> AgentUtterance {
    AgentUtterance { number, speaker: speaker.into(), name: None, start, end, text: text.into() }
}

fn request() -> ReviewRequest {
    ReviewRequest {
        recording: "/reunions/2026-11-21 14h02 - Point/2026-11-21 14h02 - Point - enregistrement.mp4".into(),
        meetings_folder: "/reunions".into(),
        folder_name: "2026-11-21 14h02 - Point".into(),
        title: "Point".into(),
        start: DateTime::new(2026, 11, 21, 14, 2),
        speaker_count: Some(2),
        utterances: vec![
            utterance(0, "SPEAKER_00", 0.5, 2.5, "Bonjour, on parle de Zorglube."),
            utterance(1, "SPEAKER_01", 5.0, 7.0, "La recète est prête ?"),
            utterance(2, "SPEAKER_01", 69.2, 111.0, "Pas encore, il faut attendre que l'équipe de recette ait fini, jeudi peut-être, ou alors la semaine prochaine si tout va bien."),
        ],
        lexicon: "---\ntype: lexique\n---\n\n# Lexique\n\n## Erreurs connues\n\n| Entendu | Correct |\n|---|---|\n| Zorglube | Zorglub |\n".into(),
        vocabulary: "Réunion sur Zorglub.".into(),
    }
}

fn first_pass() -> Value {
    json!({
        "corrections": [{ "line": 2, "before": "recète", "after": "recette", "reason": "homophone" }],
        "speakers": [{ "line": 3, "speaker": "SPEAKER_00", "reason": "répond à la question" }],
        "doubts": [
            { "line": 3, "excerpt": "jeudi", "hypothesis": "mardi", "reason": "son couvert" },
            { "line": 1, "excerpt": "Zorglube", "hypothesis": "?", "reason": "nom inconnu" },
            { "line": 3, "excerpt": "jeudi", "hypothesis": "mardi", "reason": "en double" },
            { "line": 9, "excerpt": "hors", "hypothesis": "?", "reason": "ligne inconnue" },
            { "line": 2, "excerpt": " ", "hypothesis": "?", "reason": "extrait vide" }
        ],
        "lexicon": [{ "heard": "recète", "correct": "recette" }]
    })
}

fn second_pass() -> Value {
    json!({
        "decisions": [
            { "line": 3, "excerpt": "jeudi", "status": "corrected", "after": "mardi", "reason": "la réécoute dit mardi" },
            { "line": 1, "excerpt": "Zorglube", "status": "to_check", "after": "Zorglub", "reason": "pas clair" }
        ],
        "lexicon": [{ "heard": "Zorglube", "correct": "Zorglub" }]
    })
}

fn review(claude: &FakeClaude, relisten: &FakeRelisten, request: ReviewRequest) -> Result<greffier_core::adapters::ReviewResult, String> {
    review::review(claude, relisten, &request, &AtomicBool::new(false), &mut |_| {})
}

#[test]
fn first_pass_receives_numbered_utterances_lexicon_and_context() {
    let claude = FakeClaude::answering(vec![Ok(json!({ "corrections": [], "speakers": [], "doubts": [], "lexicon": [] }))]);
    review(&claude, &FakeRelisten::default(), request()).expect("relecture");

    let calls = claude.calls();
    assert_eq!(calls.len(), 1, "sans passage douteux, ni réécoute ni deuxième passe");
    let a = &calls[0];
    assert_eq!(a.input, "#1 [0:00:00 → 0:00:02] SPEAKER_00 : Bonjour, on parle de Zorglube.\n#2 [0:00:05 → 0:00:07] SPEAKER_01 : La recète est prête ?\n#3 [0:01:09 → 0:01:51] SPEAKER_01 : Pas encore, il faut attendre que l'équipe de recette ait fini, jeudi peut-être, ou alors la semaine prochaine si tout va bien.");
    assert!(matches!(&a.session, Session::New(id) if id.len() == 36), "{:?}", a.session);
    assert_eq!(a.directory, PathBuf::from("/reunions"));
    assert!(a.prompt.contains("Réunion du 2026-11-21"), "{}", a.prompt);
    assert!(a.prompt.contains("`2026-11-21 14h02 - Point`"), "{}", a.prompt);
    assert!(a.prompt.contains("2 personnes dans la réunion."), "{}", a.prompt);
    assert!(a.prompt.contains("## Erreurs connues\n\n| Entendu | Correct |\n|---|---|\n| Zorglube | Zorglub |"), "{}", a.prompt);
    assert!(!a.prompt.contains("type: lexique") && !a.prompt.contains("\n# Lexique") && !a.prompt.contains("{{"), "{}", a.prompt);
    assert!(!a.prompt.contains("Deuxième passe"), "la deuxième partie du prompt attend la réécoute");
    for tool in ["Write", "Edit", "Bash", "WebFetch", "WebSearch", "Task"] {
        assert!(a.forbidden_tools.iter().any(|o| o == tool) && !a.allowed_tools.iter().any(|o| o == tool), "{tool}");
    }
    assert_eq!(a.allowed_tools, ["Read", "Glob", "Grep"]);
    let schema: Value = serde_json::from_str(&a.schema).expect("schéma JSON");
    assert_eq!(schema["required"], json!(["corrections", "locuteurs", "doutes", "lexique"]));
}

#[test]
fn doubtful_passages_are_relistened_then_decided_in_the_same_session() {
    let claude = FakeClaude::answering(vec![Ok(first_pass()), Ok(second_pass())]);
    let relisten = FakeRelisten { texts: vec!["pas encore mardi peut-être".into(), "".into()], ..Default::default() };
    let r = review(&claude, &relisten, request()).expect("relecture");

    // ±1,5 s autour de la réplique, 28 s au plus : la réplique 3 (42 s) est centrée sur l'extrait.
    let requests = relisten.requests.lock().unwrap().clone();
    assert_eq!(requests.len(), 1);
    let (recording, excerpts, vocabulary) = &requests[0];
    assert_eq!(recording, &request().recording);
    assert_eq!(vocabulary, "Réunion sur Zorglub.");
    let center = 69.2 + 63.0 / 126.0 * 41.8;
    let expected = [(center - 12.0, center + 14.0), (0.0, 4.0)];
    assert_eq!(excerpts.len(), 2);
    for ((a, b), (c, d)) in excerpts.iter().zip(expected) {
        assert!((a - c).abs() < 1e-6 && (b - d).abs() < 1e-6, "{excerpts:?} ≠ {expected:?}");
    }

    let calls = claude.calls();
    assert_eq!(calls.len(), 2);
    let Session::New(id) = &calls[0].session else { panic!("{:?}", calls[0].session) };
    assert_eq!(calls[1].session, Session::Resume(id.clone()));
    assert_eq!(calls[1].input, "");
    assert!(calls[1].prompt.starts_with("## Deuxième passe"), "{}", calls[1].prompt);
    assert!(
        calls[1].prompt.ends_with("- ligne #3 [0:01:09] · extrait : « jeudi » · ton hypothèse : « mardi »\n  réécoute : « pas encore mardi peut-être »\n- ligne #1 [0:00:00] · extrait : « Zorglube » · ton hypothèse : « ? »\n  réécoute : « (rien d'exploitable) »"),
        "{}",
        calls[1].prompt
    );
    let schema: Value = serde_json::from_str(&calls[1].schema).expect("schéma JSON");
    assert_eq!(schema["required"], json!(["decisions", "lexique"]));

    assert_eq!(r.session.as_deref(), Some(id.as_str()));
    assert_eq!(r.corrections, [ConfidentCorrection { utterance: 1, before: "recète".into(), after: "recette".into() }]);
    assert_eq!(r.speaker_changes, [SpeakerChange { utterance: 2, speaker: "SPEAKER_00".into() }]);
    assert_eq!(r.corrected_passages, [CorrectedPassage { utterance: 2, excerpt: "jeudi".into(), after: "mardi".into(), relisten: "pas encore mardi peut-être".into() }]);
    assert_eq!(r.passages_to_check, [FlaggedPassage { utterance: 0, excerpt: "Zorglube".into(), hypothesis: Some("Zorglub".into()), relisten: "".into() }]);
    assert_eq!(r.lexicon_additions, [LexiconAddition { heard: "recète".into(), correct: "recette".into() }, LexiconAddition { heard: "Zorglube".into(), correct: "Zorglub".into() }]);
}

#[test]
fn relisten_drops_bracketed_noises_like_the_transcription() {
    let claude = FakeClaude::answering(vec![Ok(first_pass()), Ok(second_pass())]);
    let relisten = FakeRelisten { texts: vec!["[Musique] pas encore (rires) mardi peut-être".into(), " (bruit de fond) ".into()], ..Default::default() };
    let r = review(&claude, &relisten, request()).expect("relecture");
    assert!(
        claude.calls()[1].prompt.ends_with("  réécoute : « pas encore mardi peut-être »\n- ligne #1 [0:00:00] · extrait : « Zorglube » · ton hypothèse : « ? »\n  réécoute : « (rien d'exploitable) »"),
        "{}",
        claude.calls()[1].prompt
    );
    assert_eq!(r.corrected_passages[0].relisten, "pas encore mardi peut-être");
    assert_eq!(r.passages_to_check[0].relisten, "");
}

#[test]
fn a_passage_without_decision_or_corrected_to_a_question_mark_stays_to_check() {
    let second = json!({ "decisions": [{ "line": 1, "excerpt": "autre chose", "status": "corrected", "after": "? (un nom de client, peut-être)", "reason": "?" }], "lexicon": [] });
    let claude = FakeClaude::answering(vec![Ok(first_pass()), Ok(second)]);
    let relisten = FakeRelisten { texts: vec!["a".into(), "b".into()], ..Default::default() };
    let r = review(&claude, &relisten, request()).expect("relecture");
    assert!(r.corrected_passages.is_empty());
    let passages: Vec<(usize, &str, Option<&str>, &str)> = r.passages_to_check.iter().map(|p| (p.utterance, p.excerpt.as_str(), p.hypothesis.as_deref(), p.relisten.as_str())).collect();
    assert_eq!(passages, [(2, "jeudi", Some("mardi"), "a"), (0, "Zorglube", None, "b")]);
}

#[test]
fn at_most_thirty_doubtful_passages() {
    let doubts: Vec<Value> = (0..40).map(|i| json!({ "line": 3, "excerpt": format!("mot {i}"), "hypothesis": "?", "reason": "" })).collect();
    let claude = FakeClaude::answering(vec![Ok(json!({ "corrections": [], "speakers": [], "doubts": doubts, "lexicon": [] })), Ok(json!({ "decisions": [], "lexicon": [] }))]);
    let r = review(&claude, &FakeRelisten::default(), request()).expect("relecture");
    assert_eq!(r.passages_to_check.len(), 30);
}

#[test]
fn a_failed_relisten_does_not_prevent_the_second_pass() {
    let claude = FakeClaude::answering(vec![Ok(first_pass()), Ok(second_pass())]);
    let relisten = FakeRelisten { failure: Some("modèle illisible".into()), ..Default::default() };
    let r = review(&claude, &relisten, request()).expect("relecture");
    assert!(claude.calls()[1].prompt.contains("réécoute : « (réécoute impossible : modèle illisible) »"), "{}", claude.calls()[1].prompt);
    assert_eq!(r.corrected_passages.len(), 1);
}

#[test]
fn if_claude_fails_the_review_fails_with_its_message() {
    let claude = FakeClaude::answering(vec![Err("Claude : Credit balance is too low".into())]);
    assert_eq!(review(&claude, &FakeRelisten::default(), request()), Err("Claude : Credit balance is too low".into()));

    let claude = FakeClaude::answering(vec![Ok(first_pass()), Err("Claude : délai dépassé".into())]);
    assert_eq!(review(&claude, &FakeRelisten::default(), request()), Err("Claude : délai dépassé".into()));

    let claude = FakeClaude::answering(vec![Ok(json!({ "corrections": "pas une liste" }))]);
    let e = review(&claude, &FakeRelisten::default(), request()).expect_err("réponse illisible");
    assert!(e.starts_with("Claude a rendu une réponse illisible"), "{e}");
}

#[test]
fn cli_response_gives_structured_output_or_a_clear_message() {
    let ok = r#"{"type":"result","is_error":false,"result":"","session_id":"abc","structured_output":{"x":1}}"#;
    assert_eq!(claude::read_response(ok, "", true), Ok(json!({ "x": 1 })));

    let error = r#"{"type":"result","is_error":true,"result":"Credit balance is too low"}"#;
    assert_eq!(claude::read_response(error, "", false), Err("Claude : Credit balance is too low".into()));

    let without = r#"{"type":"result","is_error":false,"result":"Je n'ai pas pu."}"#;
    assert_eq!(claude::read_response(without, "", true), Err("Claude n'a pas rendu de réponse structurée : Je n'ai pas pu.".into()));

    assert_eq!(claude::read_response("", "ligne 1\nligne 2\n\nError: not logged in\n", false), Err("Claude : ligne 2 / Error: not logged in".into()));
    assert_eq!(claude::read_response("", "", false), Err("Claude s'est arrêté sans rien dire.".into()));
}
