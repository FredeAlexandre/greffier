//! ⌘K search: a word from a title, a summary or a transcription, across all meetings, and the
//! moment at which to open the meeting (the utterance, or the topic of a summary line).

mod bench;

use bench::Bench;
use greffier_core::{Command, Role};
use greffier_core::search::{Location, SearchResult};

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
