//! ⌘K search: a word from a title, a summary or a transcription, across all meetings, and the
//! moment at which to open the meeting (the utterance, or the topic of a summary line).

mod bench;

use bench::Bench;
use greffier_core::{Command, MeetingId, Role};
use greffier_core::search::{Found, Location, SearchQuery, SearchReport, SearchResult};

const RELEASE: &str = "2026-11-20 10h00 - Revue de release";

/// What we look at in a result: where, at what moment, and the highlighted passage.
fn outline(r: &SearchResult) -> (Location, Option<f64>, &str) {
    (r.location, r.instant, &r.excerpt[r.highlight.0..r.highlight.1])
}

#[test]
fn a_summary_line_opens_the_meeting_at_its_topic() {
    let bench = Bench::on_fixtures();
    let id = bench.named_meeting(RELEASE).id;

    let results = bench.greffier.search("prévient");

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].meeting, id);
    assert_eq!(outline(&results[0]), (Location::Summary, Some(5.0), "prévient"));
    assert_eq!(results[0].excerpt, "Alexandre prévient le support.");
}

#[test]
fn a_decision_opens_the_meeting_at_its_own_moment() {
    let bench = Bench::on_fixtures();

    let results = bench.greffier.search("vendredi");

    let found: Vec<_> = results.iter().map(outline).collect();
    assert_eq!(
        found,
        [
            // En bref and Actions have no moment: the meeting opens at the start.
            (Location::Summary, None, "vendredi"),
            (Location::Summary, Some(2.0), "vendredi"),
            (Location::Summary, None, "vendredi"),
            (Location::Transcription, Some(2.0), "vendredi"),
        ]
    );
    assert_eq!(results[1].excerpt, "(0:00:02) Release lundi si la recette passe vendredi.");
    assert_eq!(results[2].excerpt, "Khalid — lancer la recette — vendredi");
}

#[test]
fn an_utterance_opens_the_meeting_at_its_start_with_its_speaker() {
    let bench = Bench::on_fixtures();
    let id = bench.named_meeting("2026-11-18 09h15 - Réunion").id;

    let results = bench.greffier.search("mesures");

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].meeting, id);
    assert_eq!(outline(&results[0]), (Location::Transcription, Some(6.5), "mesures"));
    assert_eq!(results[0].excerpt, "Il reste deux mesures à refaire.");
    assert_eq!(results[0].speaker.as_deref(), Some("SPEAKER_00"));
}

#[test]
fn title_is_found_without_a_timestamp() {
    let bench = Bench::on_fixtures();

    let results = bench.greffier.search("revue");

    assert_eq!(results.iter().map(outline).collect::<Vec<_>>(), [(Location::Title, None, "Revue")]);
    assert_eq!(results[0].excerpt, "Revue de release");
}

#[test]
fn case_and_accents_do_not_matter() {
    let bench = Bench::on_fixtures();

    let results = bench.greffier.search("  PREVIENS le Support ");

    assert_eq!(results.iter().map(outline).collect::<Vec<_>>(), [(Location::Transcription, Some(5.0), "préviens le support")]);
    assert!(bench.greffier.search("").is_empty());
    assert!(bench.greffier.search("kangourou").is_empty());
}

#[test]
fn search_follows_changes_made_in_the_app() {
    let mut bench = Bench::on_fixtures();
    let id = bench.named_meeting("2026-11-18 09h15 - Réunion").id;

    bench.execute(Command::NameSpeaker { meeting: id, speaker: 0, name: "Odile".into() });

    assert_eq!(bench.greffier.search("mesures")[0].speaker.as_deref(), Some("Odile"));
}

#[test]
fn search_follows_a_summary_edited_outside_the_app() {
    let mut bench = Bench::on_fixtures();
    let path = bench.greffier.folder().file(RELEASE, Role::Summary);
    let md = bench.read(&path).replace("- Alexandre prévient le support.", "- Alexandre prévient le support.\n- Le kangourou du support est en congé.");
    std::fs::write(&path, md).unwrap();

    bench.greffier.rescan_folder();

    assert_eq!(bench.greffier.search("kangourou").iter().map(outline).collect::<Vec<_>>(), [(Location::Summary, Some(5.0), "kangourou")]);
}

// --- The cache in the app folder -------------------------------------------------------------

fn cache_file(cache: &std::path::Path, name: &str) -> std::path::PathBuf {
    cache.join(format!("{name}.json"))
}

#[test]
fn cache_rebuilds_itself_if_missing() {
    let mut bench = Bench::on_fixtures();
    let cache = bench.enable_search_cache();
    assert!(cache_file(&cache, RELEASE).is_file());
    assert!(cache_file(&cache, "2026-11-18 09h15 - Réunion").is_file());

    std::fs::remove_dir_all(&cache).unwrap();
    bench.reopen();
    bench.enable_search_cache();

    assert!(bench.read(&cache_file(&cache, RELEASE)).contains("Alexandre prévient le support."));
    assert_eq!(bench.greffier.search("prévient").len(), 1);
}

#[test]
fn cache_follows_changes_to_the_meetings_folder() {
    let mut bench = Bench::on_fixtures();
    let cache = bench.enable_search_cache();
    let path = bench.greffier.folder().file(RELEASE, Role::Summary);
    let md = bench.read(&path).replace("- Alexandre prévient le support.", "- Le kangourou du support est en congé.");
    std::fs::write(&path, md).unwrap();

    bench.greffier.rescan_folder();

    let json = bench.read(&cache_file(&cache, RELEASE));
    assert!(json.contains("kangourou") && !json.contains("prévient"));

    // A folder deleted in the Finder: its entry leaves the cache.
    std::fs::remove_dir_all(bench.folder.join(RELEASE)).unwrap();
    bench.greffier.rescan_folder();
    assert!(!cache_file(&cache, RELEASE).exists());
    assert!(bench.greffier.search("kangourou").is_empty());
}

#[test]
fn a_stale_or_unreadable_cache_is_rebuilt_at_launch() {
    let mut bench = Bench::on_fixtures();
    let cache = bench.enable_search_cache();
    // App closed: the summary changes, and another meeting's cache gets corrupted.
    let path = bench.greffier.folder().file(RELEASE, Role::Summary);
    let md = bench.read(&path).replace("- Alexandre prévient le support.", "- Le kangourou du support est en congé.");
    std::fs::write(&path, md).unwrap();
    std::fs::write(cache_file(&cache, "2026-11-18 09h15 - Réunion"), "{pas du json").unwrap();

    bench.reopen();
    bench.enable_search_cache();

    assert_eq!(bench.greffier.search("kangourou").len(), 1);
    assert!(bench.greffier.search("prévient").is_empty());
    assert_eq!(bench.greffier.search("mesures").len(), 1);
    assert!(bench.read(&cache_file(&cache, "2026-11-18 09h15 - Réunion")).contains("mesures"));
}

#[test]
fn a_changed_title_follows_in_search_and_cache() {
    let mut bench = Bench::on_fixtures();
    let cache = bench.enable_search_cache();
    let id = bench.named_meeting(RELEASE).id;

    bench.execute(Command::ChangeTitle { meeting: id, title: "Revue de sortie".into() });

    assert_eq!(bench.greffier.search("sortie").iter().map(outline).collect::<Vec<_>>(), [(Location::Title, None, "sortie")]);
    assert_eq!(bench.greffier.search("prévient")[0].meeting, id);
    assert!(!cache_file(&cache, RELEASE).exists());
    assert!(cache_file(&cache, "2026-11-20 10h00 - Revue de sortie").is_file());
}

#[test]
fn a_long_passage_is_cut_between_two_words_around_the_found_word() {
    let mut bench = Bench::on_fixtures();
    let path = bench.greffier.folder().file(RELEASE, Role::Summary);
    let is_long = "Alexandre rappelle que la recette de vendredi conditionne tout le calendrier de la publication, puis il présente le kangourou mascotte de la nouvelle version, que toute l'équipe trouve réussi même si le support préfère attendre les retours des clients avant de valider.";
    let md = bench.read(&path).replace("- Alexandre prévient le support.", &format!("- {is_long}"));
    std::fs::write(&path, md).unwrap();
    bench.greffier.rescan_folder();

    let r = &bench.greffier.search("kangourou")[0];

    assert_eq!(r.excerpt, "…de la publication, puis il présente le kangourou mascotte de la nouvelle version, que toute l'équipe trouve réussi même si le support…");
    assert_eq!(&r.excerpt[r.highlight.0..r.highlight.1], "kangourou");
}

// --- The agents' search (`greffier search`) -----------------------------------------------------

const WORKSHOP: &str = "2026-11-21 14h00 - Atelier marque-pages";

const WORKSHOP_SUMMARY: &str = "## En bref\n\nLes bookmarks restent synchronisés.\n\n## Sujets abordés\n\n### Synchronisation (0:00:04)\n\n- Le serveur synchronise les bookmarks.\n- Les bookmarks partagés suivent.\n";

/// Adds a meeting folder with these utterances (speaker, start, text) and this summary, as if
/// written outside the app, and rereads the folder.
fn add_meeting(bench: &mut Bench, name: &str, utterances: &[(&str, f64, &str)], summary: Option<&str>) -> MeetingId {
    let folder = bench.folder.join(name);
    std::fs::create_dir_all(&folder).unwrap();
    let utterances: Vec<_> = utterances.iter().map(|(speaker, start, text)| serde_json::json!({ "speaker": speaker, "start": start, "end": start + 1.0, "text": text })).collect();
    std::fs::write(folder.join(format!("{name} - transcription.json")), serde_json::json!({ "version": 1, "utterances": utterances }).to_string()).unwrap();
    if let Some(md) = summary {
        std::fs::write(folder.join(format!("{name} - bilan.md")), md).unwrap();
    }
    bench.greffier.rescan_folder();
    bench.named_meeting(name).id
}

/// The fixtures, and a workshop on bookmarks the day after the release review.
fn with_workshop() -> (Bench, MeetingId) {
    let mut bench = Bench::on_fixtures();
    let id = add_meeting(
        &mut bench,
        WORKSHOP,
        &[
            ("SPEAKER_00", 0.0, "On regarde les marque-pages aujourd’hui."),
            ("SPEAKER_01", 4.0, "Les bookmarks du navigateur sont synchronisés avec le serveur."),
            ("SPEAKER_00", 9.0, "Le serveur des bookmarks tombe le lundi."),
            ("SPEAKER_01", 15.0, "Et l’équipe support le sait."),
            ("SPEAKER_00", 20.0, "On synchronise les bookmarks chaque nuit, et les bookmarks partagés aussi."),
        ],
        Some(WORKSHOP_SUMMARY),
    );
    (bench, id)
}

fn query(text: &str) -> SearchQuery {
    SearchQuery { text: text.into(), ..SearchQuery::default() }
}

/// What we look at in a result found by an agent: where, at what moment, which utterance.
fn spot(r: &Found) -> (Location, Option<f64>, Option<usize>) {
    (r.location, r.instant, r.utterance)
}

fn spots(report: &SearchReport) -> Vec<(Location, Option<f64>, Option<usize>)> {
    report.results.iter().map(spot).collect()
}

fn highlighted(r: &Found) -> Vec<&str> {
    r.highlights.iter().map(|&(a, b)| &r.excerpt[a..b]).collect()
}

#[test]
fn every_word_of_the_query_must_be_in_the_line_in_any_order() {
    let (bench, id) = with_workshop();

    let report = bench.greffier.find(&query("serveur bookmarks"));

    assert!(report.results.iter().all(|r| r.meeting == id));
    assert_eq!(
        spots(&report),
        [(Location::Summary, Some(4.0), None), (Location::Transcription, Some(4.0), Some(1)), (Location::Transcription, Some(9.0), Some(2))]
    );
    assert_eq!(report.results[1].excerpt, "Les bookmarks du navigateur sont synchronisés avec le serveur.");
    assert_eq!(report.results[1].speaker.as_deref(), Some("SPEAKER_01"));
}

#[test]
fn a_quoted_phrase_must_be_in_the_line_as_it_is() {
    let (bench, _) = with_workshop();

    let phrase = bench.greffier.find(&query("\"serveur des bookmarks\""));
    let words = bench.greffier.find(&query("serveur des bookmarks"));
    let mixed = bench.greffier.find(&query("lundi \"le serveur\""));

    assert_eq!(spots(&phrase), [(Location::Transcription, Some(9.0), Some(2))]);
    assert_eq!(spots(&words), [(Location::Transcription, Some(9.0), Some(2))]);
    assert_eq!(highlighted(&words.results[0]), ["serveur", "des", "bookmarks"]);
    assert_eq!(highlighted(&phrase.results[0]), ["serveur des bookmarks"]);
    assert_eq!(spots(&mixed), [(Location::Transcription, Some(9.0), Some(2))]);
}

#[test]
fn the_agents_search_ignores_case_accents_and_apostrophes() {
    let (bench, _) = with_workshop();

    let report = bench.greffier.find(&query("L'EQUIPE Support"));

    assert_eq!(spots(&report), [(Location::Transcription, Some(15.0), Some(3))]);
    assert_eq!(highlighted(&report.results[0]), ["l’équipe", "support"]);
}

#[test]
fn a_single_word_is_found_as_by_the_quick_search() {
    let (bench, _) = with_workshop();

    for word in ["vendredi", "prévient", "mesures", "revue"] {
        let found = bench.greffier.find(&query(word));
        let quick = bench.greffier.search(word);
        let found: Vec<_> = found.results.iter().map(|r| (r.meeting, r.location, r.instant, r.speaker.clone(), r.excerpt.clone(), r.highlights.clone())).collect();
        let quick: Vec<_> = quick.into_iter().map(|r| (r.meeting, r.location, r.instant, r.speaker, r.excerpt, vec![r.highlight])).collect();
        assert_eq!(found, quick, "{word}");
    }
    let empty = bench.greffier.find(&query("  \"\" "));
    assert!(empty.results.is_empty());
    assert_eq!((empty.total, empty.meetings), (0, 0));
}

#[test]
fn recent_meetings_come_first_and_in_each_the_summary_before_the_transcription() {
    let (bench, workshop) = with_workshop();
    let release = bench.named_meeting(RELEASE).id;
    let bench_test = bench.named_meeting("2026-11-18 09h15 - Réunion").id;
    let unlimited = SearchQuery { text: "le".into(), summary_lines: usize::MAX, utterances: usize::MAX, results: usize::MAX, ..SearchQuery::default() };

    let report = bench.greffier.find(&unlimited);

    let mut meetings: Vec<_> = report.results.iter().map(|r| r.meeting).collect();
    meetings.dedup();
    assert_eq!(meetings, [workshop, release, bench_test]);
    let rank = |l: Location| match l {
        Location::Title => 0,
        Location::Summary => 1,
        Location::Transcription => 2,
    };
    for pair in report.results.windows(2).filter(|p| p[0].meeting == p[1].meeting) {
        assert!((rank(pair[0].location), pair[0].utterance) <= (rank(pair[1].location), pair[1].utterance), "{pair:?}");
    }
    let release_spots: Vec<_> = report.results.iter().filter(|r| r.meeting == release).map(spot).collect();
    assert_eq!(release_spots[0], (Location::Title, None, None));
    assert!(release_spots.contains(&(Location::Summary, Some(0.0), None)));
    assert_eq!(release_spots.last(), Some(&(Location::Transcription, Some(5.0), Some(2))));
}
