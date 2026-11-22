//! La séparation des locuteurs : le nombre de locuteurs indiqué avant le traitement, la
//! transcription et la séparation en parallèle, puis l'attribution de chaque mot à un locuteur
//! selon les règles du vault (décalage de +0,2 s, interruptions rendues, recalage sur les fins de
//! phrase) et les coupures de réplique. Mots et tours de parole écrits à la main ; noms inventés.

mod bench;

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use bench::{Bench, word, turn};
use greffier_core::folder::Import;
use greffier_core::fake::{FakeDiarizer, FakeTranscriber};
use greffier_core::{Command, DateTime, Stage, Event, MeetingId, Word, Meeting, Role, Status, SpeakerTurn};

fn video() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/meetings-folder/2026-11-19 15h30 - Réunion/2026-11-19 15h30 - Réunion - enregistrement.mp4")
}

fn import_media(bench: &mut Bench) -> MeetingId {
    let import = Import { recording: video(), transcription: None, summary: None, start: DateTime::new(2026, 10, 2, 9, 30), title: "Point Ormeau".into() };
    let ev = bench.execute(Command::Import(import));
    ev.iter().find_map(|e| if let Event::MeetingImported(id) = e { Some(*id) } else { None }).expect("réunion importée")
}

fn process_transcription(bench: &mut Bench, id: MeetingId) {
    bench.execute(Command::Process(id));
    bench.advance_until(id, |s| matches!(s, Status::Running { stage: Stage::Review, .. } | Status::Failed { .. } | Status::Done));
}

/// Transcrit ces mots avec ces tours de parole et rend la réunion.
fn transcribe(words: Vec<Word>, turns: Vec<SpeakerTurn>) -> (Bench, MeetingId) {
    let mut a = bench::adapters();
    a.transcriber = Box::new(FakeTranscriber::fixed(1.0, words));
    a.diarizer = Box::new(FakeDiarizer::fixed(1.0, turns));
    let mut bench = Bench::with(a);
    let id = import_media(&mut bench);
    process_transcription(&mut bench, id);
    (bench, id)
}

/// L'étiquette du locuteur de chaque mot.
fn word_speakers(r: &Meeting) -> Vec<String> {
    r.words.iter().map(|m| m.speaker.map_or("?".into(), |i| r.speakers[i].label.clone())).collect()
}

/// « SPEAKER_00 : texte » pour chaque réplique.
fn utterances(r: &Meeting) -> Vec<String> {
    r.utterances.iter().map(|x| format!("{} : {}", r.speakers[x.speaker].label, x.text)).collect()
}

/// Des mots qui se suivent, 0,4 s chacun, à partir de `debut`.
fn sentence(start: f64, text: &str) -> Vec<Word> {
    text.split(' ').enumerate().map(|(i, t)| word(start + 0.4 * i as f64, start + 0.4 * (i + 1) as f64, t)).collect()
}

fn file(bench: &Bench, id: MeetingId, role: Role) -> String {
    let r = bench.meeting(id);
    bench.read(&bench.greffier.folder().file(&r.folder_name, role))
}

// --- Nombre de locuteurs -------------------------------------------------------------------

#[test]
fn speaker_count_given_before_processing_is_kept_in_the_sheet_and_passed_to_the_diarizer() {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let v = seen.clone();
    let mut a = bench::adapters();
    a.diarizer = Box::new(FakeDiarizer::new(1.0, move |d| {
        v.borrow_mut().push(d.speaker_count);
        Ok(bench::turns())
    }));
    let mut bench = Bench::with(a);
    let id = import_media(&mut bench);

    bench.execute(Command::SpeakerCount { meeting: id, number: Some(3) });
    assert!(file(&bench, id, Role::Sheet).contains("nombre_de_locuteurs: 3\n"), "{}", file(&bench, id, Role::Sheet));
    process_transcription(&mut bench, id);
    assert_eq!(*seen.borrow(), [Some(3)]);
}

#[test]
fn speaker_count_is_read_back_on_next_launch() {
    let mut bench = Bench::new();
    let id = import_media(&mut bench);
    bench.execute(Command::SpeakerCount { meeting: id, number: Some(4) });
    let name = bench.meeting(id).folder_name.clone();
    bench.reopen();
    assert_eq!(bench.named_meeting(&name).speaker_count, Some(4));
}

#[test]
fn left_on_auto_detect_the_diarizer_finds_the_speaker_count_itself() {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let v = seen.clone();
    let mut a = bench::adapters();
    a.diarizer = Box::new(FakeDiarizer::new(1.0, move |d| {
        v.borrow_mut().push(d.speaker_count);
        Ok(bench::turns())
    }));
    let mut bench = Bench::with(a);
    let id = import_media(&mut bench);
    bench.execute(Command::SpeakerCount { meeting: id, number: Some(3) });
    bench.execute(Command::SpeakerCount { meeting: id, number: None });
    assert!(!file(&bench, id, Role::Sheet).contains("nombre_de_locuteurs"), "{}", file(&bench, id, Role::Sheet));
    process_transcription(&mut bench, id);
    assert_eq!(*seen.borrow(), [None]);
}

// --- En parallèle --------------------------------------------------------------------------

#[test]
fn transcription_and_diarization_run_in_parallel() {
    let mut a = bench::adapters();
    a.transcriber = Box::new(FakeTranscriber::fixed(3.0, bench::words()));
    a.diarizer = Box::new(FakeDiarizer::fixed(3.0, bench::turns()));
    let mut bench = Bench::with(a);
    let id = import_media(&mut bench);
    bench.execute(Command::Process(id));
    // Les deux durent 3 s : en parallèle, l'étape dure 3 s, pas 6.
    bench.advance(1.5);
    let p = bench.meeting(id).status.progress();
    assert!((p - 0.5).abs() < 0.05, "{p}");
    bench.advance(1.7);
    assert!(matches!(bench.meeting(id).status, Status::Running { stage: Stage::Review, .. }), "{:?}", bench.meeting(id).status);
    assert_eq!(bench.meeting(id).utterances.len(), 2);
}

// --- Attribution ---------------------------------------------------------------------------

#[test]
fn words_are_shifted_by_0_2_s_before_looking_up_the_speaker() {
    // « Non. » (1,0 → 1,4 s) chevauche surtout SPEAKER_A sans décalage (0,25 s contre 0,15 s),
    // surtout SPEAKER_B avec (1,2 → 1,6 s : 0,05 s contre 0,35 s). Des phrases d'un mot : ni
    // interruption, ni recalage.
    let words = vec![word(0.0, 0.5, "Oui."), word(1.0, 1.4, "Non."), word(1.4, 1.8, "Si."), word(1.8, 2.2, "Voilà.")];
    let turns = vec![turn(0.0, 1.25, "SPEAKER_A"), turn(1.25, 3.0, "SPEAKER_B")];
    let (bench, id) = transcribe(words, turns);
    assert_eq!(word_speakers(bench.meeting(id)), ["SPEAKER_00", "SPEAKER_01", "SPEAKER_01", "SPEAKER_01"]);
}

#[test]
fn a_word_stretched_over_silence_counts_only_for_its_first_moments() {
    // « Bon. » s'étire de 1,0 à 5,0 s : il ne compte que jusqu'à 1,0 + 0,3 + 0,08 × 4 = 1,62 s
    // (+0,2 s de décalage), où SPEAKER_A parle le plus.
    let words = vec![word(0.0, 0.5, "Alors."), word(1.0, 5.0, "Bon."), word(5.0, 5.4, "Ensuite."), word(5.4, 5.8, "Fini.")];
    let turns = vec![turn(0.0, 1.6, "SPEAKER_A"), turn(1.6, 6.0, "SPEAKER_B")];
    let (bench, id) = transcribe(words, turns);
    assert_eq!(word_speakers(bench.meeting(id)), ["SPEAKER_00", "SPEAKER_00", "SPEAKER_01", "SPEAKER_01"]);
}

#[test]
fn without_an_overlapping_turn_a_word_goes_to_the_nearest_turn_within_a_second_else_to_the_previous_word() {
    let words = vec![
        word(0.0, 0.4, "Oui."),  // SPEAKER_A
        word(2.0, 2.4, "Bien."), // 2,2 → 2,6 s : SPEAKER_B commence à 3,0 s (0,4 s), SPEAKER_A finit à 1,0 s
        word(6.0, 6.4, "Hum."),  // 6,2 → 6,6 s : rien à moins d'une seconde, comme le mot précédent
    ];
    let turns = vec![turn(0.0, 1.0, "SPEAKER_A"), turn(3.0, 4.0, "SPEAKER_B"), turn(9.0, 10.0, "SPEAKER_A")];
    let (bench, id) = transcribe(words, turns);
    assert_eq!(word_speakers(bench.meeting(id)), ["SPEAKER_00", "SPEAKER_01", "SPEAKER_01"]);
}

#[test]
fn an_interruption_of_at_most_two_words_without_sentence_end_is_returned_to_the_current_speaker() {
    // SPEAKER_B ne prend que « que oui » au milieu de la phrase de SPEAKER_A.
    let words = sentence(0.0, "Je pense que oui sans aucun doute.");
    let turns = vec![turn(0.0, 1.0, "SPEAKER_A"), turn(1.0, 1.8, "SPEAKER_B"), turn(1.8, 5.0, "SPEAKER_A")];
    let (bench, id) = transcribe(words, turns);
    let r = bench.meeting(id);
    assert_eq!(utterances(r), ["SPEAKER_00 : Je pense que oui sans aucun doute."]);
    assert_eq!(r.speakers.len(), 2, "le locuteur reste détecté");
}

#[test]
fn an_interruption_ending_a_sentence_or_of_three_words_is_not_returned() {
    // « D'accord. » finit une phrase : c'est une vraie réponse.
    let mut words = sentence(0.0, "Tu viens.");
    words.push(word(0.8, 1.2, "D'accord."));
    words.extend(sentence(1.2, "Alors on y va."));
    let turns = vec![turn(0.0, 1.0, "SPEAKER_A"), turn(1.0, 1.4, "SPEAKER_B"), turn(1.4, 5.0, "SPEAKER_A")];
    let (bench, id) = transcribe(words, turns);
    assert_eq!(utterances(bench.meeting(id)), ["SPEAKER_00 : Tu viens.", "SPEAKER_01 : D'accord.", "SPEAKER_00 : Alors on y va."]);

    // Trois mots : pas une interruption (ni rendue, ni recalée : aucune fin de phrase proche).
    let words = sentence(0.0, "Alors nous allons voir tout cela ensemble cette semaine");
    let turns = vec![turn(0.0, 1.0, "SPEAKER_A"), turn(1.0, 2.2, "SPEAKER_B"), turn(2.2, 6.0, "SPEAKER_A")];
    let (bench, id) = transcribe(words, turns);
    let expected = ["SPEAKER_00", "SPEAKER_00", "SPEAKER_01", "SPEAKER_01", "SPEAKER_01", "SPEAKER_00", "SPEAKER_00", "SPEAKER_00", "SPEAKER_00"];
    assert_eq!(word_speakers(bench.meeting(id)), expected);
}

#[test]
fn punctuation_marks_do_not_count_toward_the_two_words_of_an_interruption() {
    let words = vec![word(0.0, 0.4, "On"), word(0.4, 0.8, "voit"), word(0.8, 1.2, "bien"), word(1.2, 1.3, ","), word(1.3, 1.7, "là"), word(1.7, 2.1, "le"), word(2.1, 2.5, "souci.")];
    let turns = vec![turn(0.0, 1.0, "SPEAKER_A"), turn(1.0, 1.9, "SPEAKER_B"), turn(1.9, 5.0, "SPEAKER_A")];
    let (bench, id) = transcribe(words, turns);
    assert_eq!(utterances(bench.meeting(id)), ["SPEAKER_00 : On voit bien , là le souci."]);
}

#[test]
fn a_speaker_change_just_after_a_sentence_end_is_moved_back_to_it() {
    // SPEAKER_B commence trop tard d'après la séparation : « Alors on » suit une fin de phrase,
    // ils passent à SPEAKER_B.
    let mut words = sentence(0.0, "Bon, voilà le plan.");
    words.extend(sentence(1.6, "Alors on regarde demain."));
    let turns = vec![turn(0.0, 2.6, "SPEAKER_A"), turn(2.6, 5.0, "SPEAKER_B")];
    let (bench, id) = transcribe(words, turns);
    assert_eq!(utterances(bench.meeting(id)), ["SPEAKER_00 : Bon, voilà le plan.", "SPEAKER_01 : Alors on regarde demain."]);
}

#[test]
fn a_speaker_change_just_before_a_sentence_end_is_pushed_to_it() {
    // Pas de fin de phrase dans les trois mots d'avant, mais « réunion. » finit la phrase de
    // SPEAKER_A juste après le changement : il lui revient.
    let mut words = sentence(0.0, "Bonjour tout le monde on commence la");
    words.push(word(2.8, 3.2, "réunion."));
    words.extend(sentence(3.2, "Merci Bérénice."));
    let turns = vec![turn(0.0, 2.9, "SPEAKER_A"), turn(2.9, 5.0, "SPEAKER_B")];
    let (bench, id) = transcribe(words, turns);
    assert_eq!(utterances(bench.meeting(id)), ["SPEAKER_00 : Bonjour tout le monde on commence la réunion.", "SPEAKER_01 : Merci Bérénice."]);
}

#[test]
fn when_a_sentence_end_is_in_reach_on_both_sides_snapping_goes_backward() {
    // « la plus proche, à 3 mots près » = la règle du vault, reprise à l'identique : en arrière
    // d'abord (le retard habituel de la séparation), même si « voit. », juste après le
    // changement, est plus près que « Bon. », deux mots avant.
    let mut words = sentence(0.0, "Bon. Alors on");
    words.extend(sentence(1.2, "voit. Rien de plus."));
    let turns = vec![turn(0.0, 1.3, "SPEAKER_A"), turn(1.3, 5.0, "SPEAKER_B")];
    let (bench, id) = transcribe(words, turns);
    assert_eq!(utterances(bench.meeting(id)), ["SPEAKER_00 : Bon.", "SPEAKER_01 : Alors on voit. Rien de plus."]);
}

#[test]
fn an_interruption_counts_even_if_the_previous_sentence_just_ended() {
    // Comme le vault : la règle regarde seulement les mots de l'interruption (≤ 2, sans fin de
    // phrase) et le locuteur de part et d'autre, pas où en est la phrase de l'autre.
    let mut words = sentence(0.0, "C'est fini.");
    words.extend(sentence(0.8, "Euh bon"));
    words.extend(sentence(1.6, "on continue demain."));
    let turns = vec![turn(0.0, 0.9, "SPEAKER_A"), turn(0.9, 1.7, "SPEAKER_B"), turn(1.7, 5.0, "SPEAKER_A")];
    let (bench, id) = transcribe(words, turns);
    assert_eq!(utterances(bench.meeting(id)), ["SPEAKER_00 : C'est fini. Euh bon on continue demain."]);
}

#[test]
fn snapping_does_not_go_beyond_three_words() {
    // Quatre mots entre la fin de phrase et le changement : il reste où il est.
    let mut words = sentence(0.0, "Voilà.");
    words.extend(sentence(0.4, "Alors nous allons voir"));
    words.extend(sentence(2.0, "ça plus tard."));
    let turns = vec![turn(0.0, 2.2, "SPEAKER_A"), turn(2.2, 5.0, "SPEAKER_B")];
    let (bench, id) = transcribe(words, turns);
    // Recalage en avant à 3 mots près : « ça plus tard. » finit la phrase, il revient à SPEAKER_A.
    assert_eq!(utterances(bench.meeting(id)), ["SPEAKER_00 : Voilà. Alors nous allons voir ça plus tard."]);

    let mut words = sentence(0.0, "Voilà.");
    words.extend(sentence(0.4, "Alors nous allons voir"));
    words.extend(sentence(2.0, "ça bien plus tard."));
    let turns = vec![turn(0.0, 2.2, "SPEAKER_A"), turn(2.2, 5.0, "SPEAKER_B")];
    let (bench, id) = transcribe(words, turns);
    assert_eq!(utterances(bench.meeting(id)), ["SPEAKER_00 : Voilà. Alors nous allons voir", "SPEAKER_01 : ça bien plus tard."]);
}

// --- Locuteurs, répliques, fichiers --------------------------------------------------------

#[test]
fn speakers_are_numbered_by_first_word_order_and_turns_follow() {
    // pyannote numérote à sa façon : celui qui parle en premier devient SPEAKER_00.
    let words = vec![word(0.0, 0.4, "Salut."), word(2.0, 2.4, "Bonjour."), word(4.0, 4.4, "Coucou.")];
    let turns = vec![turn(0.0, 1.0, "SPEAKER_02"), turn(2.0, 3.0, "SPEAKER_00"), turn(4.0, 5.0, "SPEAKER_01"), turn(8.0, 9.0, "SPEAKER_03")];
    let (bench, id) = transcribe(words, turns);
    let r = bench.meeting(id);
    let labels: Vec<&str> = r.speakers.iter().map(|l| l.label.as_str()).collect();
    // Un locuteur sans mot reste détecté, après les autres.
    assert_eq!(labels, ["SPEAKER_00", "SPEAKER_01", "SPEAKER_02", "SPEAKER_03"]);
    let turns: Vec<(f64, &str)> = r.turns.iter().map(|t| (t.start, t.speaker.as_str())).collect();
    assert_eq!(turns, [(0.0, "SPEAKER_00"), (2.0, "SPEAKER_01"), (4.0, "SPEAKER_02"), (8.0, "SPEAKER_03")]);
    assert_eq!(utterances(r), ["SPEAKER_00 : Salut.", "SPEAKER_01 : Bonjour.", "SPEAKER_02 : Coucou."]);
}

#[test]
fn a_new_utterance_starts_at_each_speaker_change() {
    let mut words = sentence(0.0, "On démarre ?");
    words.extend(sentence(1.2, "Oui, allons-y."));
    words.extend(sentence(2.4, "Parfait."));
    let turns = vec![turn(0.0, 1.3, "SPEAKER_A"), turn(1.3, 2.5, "SPEAKER_B"), turn(2.5, 3.2, "SPEAKER_A")];
    let (bench, id) = transcribe(words, turns);
    assert_eq!(utterances(bench.meeting(id)), ["SPEAKER_00 : On démarre ?", "SPEAKER_01 : Oui, allons-y.", "SPEAKER_00 : Parfait."]);
}

#[test]
fn readable_and_structured_transcriptions_contain_speakers_turns_and_each_word_speaker() {
    let mut words = sentence(0.0, "On démarre ?");
    words.extend(sentence(1.2, "Oui, allons-y."));
    let turns = vec![turn(0.0, 1.3, "SPEAKER_A"), turn(1.3, 2.4, "SPEAKER_B")];
    let (mut bench, id) = transcribe(words, turns);

    let json: serde_json::Value = serde_json::from_str(&file(&bench, id, Role::StructuredTranscription)).expect("JSON");
    assert_eq!(json["speakers"], serde_json::json!(["SPEAKER_00", "SPEAKER_01"]));
    let stored_speakers: Vec<&str> = json["words"].as_array().expect("mots").iter().map(|m| m["speaker"].as_str().unwrap_or("?")).collect();
    assert_eq!(stored_speakers, ["SPEAKER_00", "SPEAKER_00", "SPEAKER_00", "SPEAKER_01", "SPEAKER_01"]);
    assert_eq!(json["turns"], serde_json::json!([{ "start": 0.0, "end": 1.3, "speaker": "SPEAKER_00" }, { "start": 1.3, "end": 2.4, "speaker": "SPEAKER_01" }]));

    let md = file(&bench, id, Role::ReadableTranscription);
    assert!(md.contains("[0:00:00] SPEAKER_00 : On démarre ?"), "{md}");
    assert!(md.contains("[0:00:01] SPEAKER_01 : Oui, allons-y."), "{md}");

    // Relue telle quelle au lancement suivant.
    let name = bench.meeting(id).folder_name.clone();
    bench.reopen();
    let r = bench.named_meeting(&name);
    assert_eq!(word_speakers(r), ["SPEAKER_00", "SPEAKER_00", "SPEAKER_00", "SPEAKER_01", "SPEAKER_01"]);
    assert_eq!(r.turns.len(), 2);
}
