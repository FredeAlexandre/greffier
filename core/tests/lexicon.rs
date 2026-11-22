//! Le lexique livré avec l'app : ses quatre sections, la phrase donnée à Whisper, et son
//! installation dans le dossier de l'app sans écraser celui de l'utilisateur.

use std::sync::atomic::{AtomicU32, Ordering};

use greffier_core::lexicon::{self, INITIAL_LEXICON};

fn folder() -> std::path::PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let d = std::env::temp_dir().join(format!("greffier-lexique-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    let _ = std::fs::remove_dir_all(&d);
    d
}

#[test]
fn shipped_lexicon_has_four_sections() {
    let titles: Vec<&str> = INITIAL_LEXICON.lines().filter(|l| l.starts_with("## ")).collect();
    assert_eq!(titles, ["## Personnes", "## Mots justes", "## Erreurs connues", "## Vocabulaire"]);
}

#[test]
fn vocabulary_is_the_sentence_given_to_whisper_on_one_line() {
    let md = "# Lexique\n\n## Mots justes\n\nAcme, Zorglub.\n\n## Vocabulaire\n\nRéunion avec Alice\net Bob,   chez Acme.\n\n## Autre\n\nrien\n";
    assert_eq!(lexicon::vocabulary(md), "Réunion avec Alice et Bob, chez Acme.");
    assert_eq!(lexicon::vocabulary("## Personnes\n\n- Alice\n"), "");
    assert!(lexicon::vocabulary(INITIAL_LEXICON).starts_with("Réunion d'équipe en visio."));
}

#[test]
fn overview_counts_people_correct_words_and_known_errors() {
    let md = "## Personnes\n\n- Alice : dev\n- Bob\n\n## Mots justes\n\n_Une indication, avec une virgule._\n\nAcme, Zorglub, Quux.\n\n## Erreurs connues\n\n| Entendu | Correct |\n|---|---|\n| Alisse | Alice |\n| Akmé | Acme |\n\n## Vocabulaire\n\nRéunion.\n";
    let r = lexicon::preview(md);
    assert_eq!((r.people, r.correct_words, r.known_errors), (2, 3, 2));
}

#[test]
fn shipped_lexicon_is_an_empty_template() {
    assert_eq!(lexicon::preview(INITIAL_LEXICON), lexicon::Overview::default());
}

#[test]
fn install_writes_shipped_lexicon_without_overwriting_the_users() {
    let d = folder();
    let path = lexicon::install(&d).expect("lexique installé");
    assert_eq!(path, d.join("lexique.md"));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), INITIAL_LEXICON);

    std::fs::write(&path, "## Vocabulaire\n\nÀ moi.\n").unwrap();
    lexicon::install(&d).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "## Vocabulaire\n\nÀ moi.\n");
    let _ = std::fs::remove_dir_all(&d);
}

fn addition(heard: &str, correct: &str) -> lexicon::LexiconAddition {
    lexicon::LexiconAddition { heard: heard.into(), correct: correct.into() }
}

#[test]
fn review_adds_its_errors_to_the_known_errors_table() {
    let md = "## Erreurs connues\n\n_Une indication._\n\n| Entendu | Correct |\n|---|---|\n| Alisse, Halice | Alice |\n\n## Vocabulaire\n\nRéunion.\n";
    let (md, added) = lexicon::add_errors(md, &[addition("Zorglube", "Zorglub"), addition("halice", "Alice"), addition("Acme", "acme"), addition(" ", "Bob"), addition("Zorglube", "Zorglub"), addition("a|b", "c")]);
    assert_eq!(added, [addition("Zorglube", "Zorglub"), addition("a|b", "c")], "déjà connue, simple casse, vide et doublon sont écartés");
    assert_eq!(md, "## Erreurs connues\n\n_Une indication._\n\n| Entendu | Correct |\n|---|---|\n| Alisse, Halice | Alice |\n| Zorglube | Zorglub |\n| a\\|b | c |\n\n## Vocabulaire\n\nRéunion.\n");
    assert_eq!(lexicon::preview(&md).known_errors, 3);
}

#[test]
fn shipped_lexicon_receives_errors_under_its_empty_table() {
    let (md, added) = lexicon::add_errors(INITIAL_LEXICON, &[addition("Zorglube", "Zorglub")]);
    assert_eq!(added.len(), 1);
    assert!(md.contains("| Entendu | Correct |\n|---|---|\n| Zorglube | Zorglub |\n\n## Vocabulaire"), "{md}");
    assert_eq!(lexicon::vocabulary(&md), lexicon::vocabulary(INITIAL_LEXICON));
}

#[test]
fn without_table_or_section_known_errors_are_created() {
    let (md, _) = lexicon::add_errors("# Lexique\n\n## Personnes\n\n- Alice\n", &[addition("Zorglube", "Zorglub")]);
    assert_eq!(md, "# Lexique\n\n## Personnes\n\n- Alice\n\n## Erreurs connues\n\n| Entendu | Correct |\n|---|---|\n| Zorglube | Zorglub |\n");
    let (md, _) = lexicon::add_errors("## Erreurs connues\n\n## Vocabulaire\n\nX.\n", &[addition("Zorglube", "Zorglub")]);
    assert_eq!(md, "## Erreurs connues\n\n| Entendu | Correct |\n|---|---|\n| Zorglube | Zorglub |\n\n## Vocabulaire\n\nX.\n");
}

#[test]
fn lexicon_people_are_the_names_proposed_for_speakers() {
    let md = "## Personnes\n\n_Une puce par personne._\n\n- Alice : dev\n- **Bob** — produit\n-   Chloé Martin\n- \n\n## Mots justes\n\n- Pas une personne\n";
    assert_eq!(lexicon::people(md), ["Alice", "Bob", "Chloé Martin"]);
    assert!(lexicon::people(INITIAL_LEXICON).is_empty());
}
