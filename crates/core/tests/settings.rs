//! The settings kept from one launch to the next (`settings.json` in the app folder) and the
//! lexicon, read and written by the core: the UI does not touch these files.

mod bench;

use std::path::PathBuf;

use bench::Bench;
use greffier_core::adapters::Sources;
use greffier_core::settings::{self, DEFAULT_SHORTCUT, PersistedSettings};
use greffier_core::{Command, Settings, lexicon};

/// A temporary app folder, deleted at the end.
struct AppFolder(PathBuf);

impl AppFolder {
    fn new(name: &str) -> AppFolder {
        let d = std::env::temp_dir().join(format!("greffier-app-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        AppFolder(d)
    }
    fn settings_json(&self) -> serde_json::Value {
        serde_json::from_str(&std::fs::read_to_string(self.0.join("settings.json")).expect("settings.json")).expect("JSON")
    }
}

impl Drop for AppFolder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn without_file_the_settings_are_the_defaults() {
    let app = AppFolder::new("defaults");
    let r = PersistedSettings::read(&app.0);
    assert_eq!(r, PersistedSettings::default());
    assert_eq!(r.meetings_folder, None);
    assert_eq!(r.shortcut, DEFAULT_SHORTCUT);
    assert!(r.process_on_stop);
    assert_eq!(r.sources, Sources::default());
    assert!(!app.0.exists(), "reading writes nothing");
}

#[test]
fn saved_settings_persist_across_launches() {
    let app = AppFolder::new("round-trip");
    let r = PersistedSettings {
        meetings_folder: Some(PathBuf::from("/Users/odile/Réunions")),
        shortcut: "cmd+alt+e".into(),
        process_on_stop: false,
        sources: Sources { screen: false, mac_audio: true, micro: true },
        context_folders: vec![PathBuf::from("/Users/odile/Notes")],
    };
    r.remember(&app.0).expect("written");
    assert_eq!(PersistedSettings::read(&app.0), r);
    let json = app.settings_json();
    assert_eq!(json["meetings_folder"], "/Users/odile/Réunions");
    assert_eq!(json["sources"]["screen"], false);
}

#[test]
fn unknown_file_keys_are_kept() {
    let app = AppFolder::new("unknown");
    std::fs::create_dir_all(&app.0).unwrap();
    std::fs::write(app.0.join("settings.json"), r#"{"future": {"a": 1}, "process_on_stop": false}"#).unwrap();
    let mut r = PersistedSettings::read(&app.0);
    assert!(!r.process_on_stop);
    r.shortcut = String::new();
    r.remember(&app.0).unwrap();
    let json = app.settings_json();
    assert_eq!(json["future"]["a"], 1);
    assert_eq!(json["shortcut"], "");
    assert_eq!(PersistedSettings::read(&app.0).shortcut, "", "empty: no shortcut");
}

#[test]
fn a_shortcut_set_back_to_the_default_is_remembered() {
    let app = AppFolder::new("shortcut-back");
    let mut r = PersistedSettings { shortcut: "ctrl+alt+super+r".into(), ..PersistedSettings::default() };
    r.remember(&app.0).unwrap();
    // The UI writes the shortcut as Rune displays it.
    r.shortcut = "shift+super+r".into();
    r.remember(&app.0).unwrap();
    assert_eq!(app.settings_json()["shortcut"], "shift+super+r");
    assert_eq!(PersistedSettings::read(&app.0).shortcut, "shift+super+r");
    r.shortcut = DEFAULT_SHORTCUT.into();
    r.remember(&app.0).unwrap();
    assert_eq!(PersistedSettings::read(&app.0).shortcut, DEFAULT_SHORTCUT, "the default is written, not dropped");
}

#[test]
fn an_unreadable_file_gives_the_default_settings() {
    let app = AppFolder::new("unreadable");
    std::fs::create_dir_all(&app.0).unwrap();
    std::fs::write(app.0.join("settings.json"), "pas du JSON").unwrap();
    assert_eq!(PersistedSettings::read(&app.0), PersistedSettings::default());
}

#[test]
fn choosing_the_meetings_folder_creates_and_remembers_it() {
    let app = AppFolder::new("choice");
    let folder = app.0.with_extension("meetings");
    let _ = std::fs::remove_dir_all(&folder);
    settings::choose_meetings_folder(&app.0, &folder).expect("chosen");
    assert!(folder.is_dir());
    assert_eq!(PersistedSettings::read(&app.0).meetings_folder, Some(folder.clone()));
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn saved_settings_apply_to_core_settings_without_touching_lexicon_or_cache() {
    let r = PersistedSettings { process_on_stop: false, context_folders: vec![PathBuf::from("/Users/odile/Notes")], ..PersistedSettings::default() };
    let before = Settings { lexicon: Some(PathBuf::from("/app/lexique.md")), search_cache: Some(PathBuf::from("/app/recherche")), ..Settings::default() };
    let after = r.apply(before.clone());
    assert_eq!(after, Settings { process_on_stop: false, context_folders: vec![PathBuf::from("/Users/odile/Notes")], ..before });
}

#[test]
fn core_reads_the_lexicon_and_its_people() {
    let mut bench = Bench::new();
    // Without a lexicon: the shipped lexicon.
    assert_eq!(bench.greffier.lexicon(), lexicon::preview(lexicon::INITIAL_LEXICON));
    assert_eq!(bench.greffier.people(), lexicon::people(lexicon::INITIAL_LEXICON));

    let path = bench.folder.with_extension("lexique.md");
    std::fs::write(&path, "# Lexique\n\n## Personnes\n\n- Odile : cheffe de projet\n- Bastien\n\n## Mots justes\n\nZorglub, Ormeau.\n\n## Erreurs connues\n\n| Entendu | Correct |\n|---|---|\n| Zorglube | Zorglub |\n").unwrap();
    bench.execute(Command::ChangeSettings(Settings { lexicon: Some(path.clone()), ..bench.greffier.settings().clone() }));
    let a = bench.greffier.lexicon();
    assert_eq!((a.people, a.correct_words, a.known_errors), (2, 2, 1));
    assert_eq!(bench.greffier.people(), ["Odile", "Bastien"]);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn app_folder_is_in_the_users_library_unless_replaced() {
    let home = PathBuf::from("/Users/odile");
    assert_eq!(settings::app_folder_for(None, &home), home.join("Library/Application Support/Greffier"));
    assert_eq!(settings::app_folder_for(Some(PathBuf::from("/tmp/essai")), &home), PathBuf::from("/tmp/essai"), "GREFFIER_APP_FOLDER replaces it");
    assert_eq!(settings::suggested_folder(), settings::home().join("Documents/Réunions"));
}

#[test]
fn the_meetings_folder_is_the_replaced_one_otherwise_the_remembered_one() {
    let app = AppFolder::new("meetings-folder");
    assert_eq!(settings::meetings_folder_for(None, &app.0), None, "none chosen yet");
    assert_eq!(settings::meetings_folder_for(Some(PathBuf::from("/tmp/copie")), &app.0), Some(PathBuf::from("/tmp/copie")));
    assert!(!app.0.exists(), "reading writes nothing");

    PersistedSettings { meetings_folder: Some(PathBuf::from("/Users/odile/Réunions")), ..PersistedSettings::default() }.remember(&app.0).expect("written");
    assert_eq!(settings::meetings_folder_for(None, &app.0), Some(PathBuf::from("/Users/odile/Réunions")));
    assert_eq!(settings::meetings_folder_for(Some(PathBuf::from("/tmp/copie")), &app.0), Some(PathBuf::from("/tmp/copie")), "GREFFIER_FOLDER wins over the remembered folder");
}
