//! Le lexique livré avec l'app : ses quatre sections, la phrase donnée à Whisper, et son
//! installation dans le dossier de l'app sans écraser celui de l'utilisateur.

use std::sync::atomic::{AtomicU32, Ordering};

use greffier_coeur::lexique::{self, LEXIQUE_INITIAL};

fn dossier() -> std::path::PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let d = std::env::temp_dir().join(format!("greffier-lexique-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    let _ = std::fs::remove_dir_all(&d);
    d
}

#[test]
fn le_lexique_livre_a_quatre_sections() {
    let titres: Vec<&str> = LEXIQUE_INITIAL.lines().filter(|l| l.starts_with("## ")).collect();
    assert_eq!(titres, ["## Personnes", "## Mots justes", "## Erreurs connues", "## Vocabulaire"]);
}

#[test]
fn le_vocabulaire_est_la_phrase_donnee_a_whisper_sur_une_ligne() {
    let md = "# Lexique\n\n## Mots justes\n\nAcme, Zorglub.\n\n## Vocabulaire\n\nRéunion avec Alice\net Bob,   chez Acme.\n\n## Autre\n\nrien\n";
    assert_eq!(lexique::vocabulaire(md), "Réunion avec Alice et Bob, chez Acme.");
    assert_eq!(lexique::vocabulaire("## Personnes\n\n- Alice\n"), "");
    assert!(lexique::vocabulaire(LEXIQUE_INITIAL).starts_with("Réunion d'équipe en visio."));
}

#[test]
fn le_resume_compte_personnes_mots_justes_et_erreurs_connues() {
    let md = "## Personnes\n\n- Alice : dev\n- Bob\n\n## Mots justes\n\n_Une indication, avec une virgule._\n\nAcme, Zorglub, Quux.\n\n## Erreurs connues\n\n| Entendu | Correct |\n|---|---|\n| Alisse | Alice |\n| Akmé | Acme |\n\n## Vocabulaire\n\nRéunion.\n";
    let r = lexique::resume(md);
    assert_eq!((r.personnes, r.mots_justes, r.erreurs_connues), (2, 3, 2));
}

#[test]
fn le_lexique_livre_est_un_modele_vide() {
    assert_eq!(lexique::resume(LEXIQUE_INITIAL), lexique::Resume::default());
}

#[test]
fn l_installation_ecrit_le_lexique_livre_sans_ecraser_celui_de_l_utilisateur() {
    let d = dossier();
    let chemin = lexique::installer(&d).expect("lexique installé");
    assert_eq!(chemin, d.join("lexique.md"));
    assert_eq!(std::fs::read_to_string(&chemin).unwrap(), LEXIQUE_INITIAL);

    std::fs::write(&chemin, "## Vocabulaire\n\nÀ moi.\n").unwrap();
    lexique::installer(&d).unwrap();
    assert_eq!(std::fs::read_to_string(&chemin).unwrap(), "## Vocabulaire\n\nÀ moi.\n");
    let _ = std::fs::remove_dir_all(&d);
}
