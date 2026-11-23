//! A meeting's identifier (glossary: Identifier): its short, stable name for the command line
//! and agents, taken from its folder's date and time, and what a user or an agent may type to
//! name a meeting.

use greffier_core::DateTime;
use greffier_core::identifier::{self, Identifiers};

/// Meeting folders as `MeetingsFolder::names` gives them (most recent first), with two
/// collisions at 9:39 and a folder that does not follow the naming rule.
fn folders() -> Identifiers {
    Identifiers::new([
        "2026-11-21 14h02 - Point produit",
        "2026-11-20 10h00 - Revue de release",
        "2026-11-20 09h39 - Rétro",
        "2026-11-20 09h39 - Point d'équipe (2)",
        "2026-11-20 09h39 - Point d'équipe",
        "Divers",
    ])
}

#[test]
fn the_identifier_is_the_folders_date_and_time() {
    assert_eq!(identifier::base(DateTime::new(2026, 11, 20, 9, 39)), "2026-11-20-0939");
    assert_eq!(identifier::base(DateTime::new(2026, 1, 2, 14, 5)), "2026-01-02-1405");
    let ids = folders();
    assert_eq!(ids.of("2026-11-21 14h02 - Point produit"), Some("2026-11-21-1402"));
    assert_eq!(ids.of("2026-11-20 10h00 - Revue de release"), Some("2026-11-20-1000"));
    assert_eq!(ids.of("Divers"), None, "a folder that does not follow the naming rule has none");
    assert_eq!(ids.of("2026-11-22 08h00 - Inconnue"), None);
}

#[test]
fn meetings_in_the_same_minute_are_numbered_in_folder_name_order() {
    let ids = folders();
    assert_eq!(ids.of("2026-11-20 09h39 - Point d'équipe"), Some("2026-11-20-0939"));
    assert_eq!(ids.of("2026-11-20 09h39 - Point d'équipe (2)"), Some("2026-11-20-0939-2"));
    assert_eq!(ids.of("2026-11-20 09h39 - Rétro"), Some("2026-11-20-0939-3"));
    // In the order given, with the folder names.
    let all: Vec<(&str, &str)> = ids.iter().collect();
    assert_eq!(all[0], ("2026-11-21-1402", "2026-11-21 14h02 - Point produit"));
    assert_eq!(all.len(), 5);
}

#[test]
fn identifiers_do_not_depend_on_the_other_minutes() {
    let more = Identifiers::new(["2026-11-25 08h00 - Nouvelle", "2026-11-20 09h39 - Point d'équipe", "2026-11-19 09h39 - Hier"]);
    assert_eq!(more.of("2026-11-20 09h39 - Point d'équipe"), folders().of("2026-11-20 09h39 - Point d'équipe"));
}

#[test]
fn an_exact_identifier_or_folder_name_names_its_meeting() {
    let ids = folders();
    assert_eq!(ids.resolve("2026-11-20-1000"), Ok("2026-11-20 10h00 - Revue de release"));
    assert_eq!(ids.resolve("2026-11-20-0939"), Ok("2026-11-20 09h39 - Point d'équipe"), "exact, although also the start of -2 and -3");
    assert_eq!(ids.resolve("2026-11-20-0939-3"), Ok("2026-11-20 09h39 - Rétro"));
    assert_eq!(ids.resolve("2026-11-20 09h39 - Rétro"), Ok("2026-11-20 09h39 - Rétro"));
    assert_eq!(ids.resolve(" 2026-11-20-1000\n"), Ok("2026-11-20 10h00 - Revue de release"), "surrounding spaces are ignored");
}

#[test]
fn a_path_names_no_meeting_by_itself() {
    // Its folder may be another meetings folder's: the caller, who can read the disk, checks
    // where it is and passes its folder name.
    let ids = folders();
    for query in ["/Users/odile/Documents/Réunions/2026-11-20 09h39 - Rétro", "Réunions/2026-11-20 09h39 - Rétro/", "/tmp/2026-11-20-1000"] {
        assert!(ids.resolve(query).unwrap_err().candidates.is_empty(), "{query}");
    }
}

#[test]
fn a_date_or_the_start_of_an_identifier_names_the_only_meeting_it_matches() {
    let ids = folders();
    assert_eq!(ids.resolve("2026-11-21"), Ok("2026-11-21 14h02 - Point produit"));
    assert_eq!(ids.resolve("2026-11-20-10"), Ok("2026-11-20 10h00 - Revue de release"));
    assert_eq!(ids.resolve("2026-11-20-0939-2"), Ok("2026-11-20 09h39 - Point d'équipe (2)"));
}

#[test]
fn an_ambiguous_name_lists_the_candidates() {
    let ids = folders();
    let e = ids.resolve("2026-11-20").unwrap_err();
    let candidates: Vec<&str> = e.candidates.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(candidates, ["2026-11-20-1000", "2026-11-20-0939-3", "2026-11-20-0939-2", "2026-11-20-0939"]);
    assert_eq!(
        e.to_string(),
        "several meetings match \"2026-11-20\":\n2026-11-20-1000 · Revue de release\n2026-11-20-0939-3 · Rétro\n2026-11-20-0939-2 · Point d'équipe\n2026-11-20-0939 · Point d'équipe"
    );
    assert_eq!(ids.resolve("2026-11").unwrap_err().candidates.len(), 5);
}

#[test]
fn an_unknown_name_is_not_found() {
    let ids = folders();
    for query in ["2026-12-01", "Divers", "Revue de release", "", "  "] {
        let e = ids.resolve(query).unwrap_err();
        assert!(e.candidates.is_empty(), "{query}");
    }
    assert_eq!(ids.resolve("2026-12-01").unwrap_err().to_string(), "meeting not found: \"2026-12-01\"");
}
