//! Le lexique : un fichier Markdown du dossier de l'app, à quatre sections.
//!
//! - **Personnes** : une puce par personne (`- Nom : rôle`) ;
//! - **Mots justes** : un paragraphe de mots séparés par des virgules ;
//! - **Erreurs connues** : un tableau `| Entendu | Correct |`, enrichi par la relecture ;
//! - **Vocabulaire** : la phrase donnée à Whisper avant la transcription (il n'en lit
//!   qu'environ 220 tokens).
//!
//! Une ligne en italique (`_…_`) est une indication du modèle, pas une entrée.
//!
//! L'app est livrée avec un lexique modèle, vide ; elle l'écrit dans son dossier au premier
//! lancement et n'y touche plus ensuite : il appartient à l'utilisateur.

use std::path::{Path, PathBuf};

/// Le lexique livré avec l'app.
pub const LEXIQUE_INITIAL: &str = include_str!("../ressources/lexique.md");

/// Le corps d'une section `## titre`, jusqu'à la section suivante.
fn section<'a>(md: &'a str, titre: &str) -> &'a str {
    let entete = format!("## {titre}");
    let mut debut = None;
    let mut position = 0;
    for ligne in md.split_inclusive('\n') {
        let fin = position + ligne.len();
        if let Some(d) = debut {
            if ligne.starts_with("## ") || ligne.starts_with("# ") {
                return &md[d..position];
            }
        } else if ligne.trim_end() == entete {
            debut = Some(fin);
        }
        position = fin;
    }
    debut.map_or("", |d| &md[d..])
}

/// La section Vocabulaire, sur une ligne : le prompt de Whisper (vide : pas de prompt).
pub fn vocabulaire(md: &str) -> String {
    section(md, "Vocabulaire").split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Ce que compte le lexique, pour les réglages.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Resume {
    pub personnes: usize,
    pub mots_justes: usize,
    pub erreurs_connues: usize,
}

pub fn resume(md: &str) -> Resume {
    let personnes = section(md, "Personnes").lines().filter(|l| l.trim_start().starts_with("- ")).count();
    let mots_justes = section(md, "Mots justes")
        .lines()
        .filter(|l| !l.trim_start().starts_with('_'))
        .flat_map(|l| l.split(','))
        .filter(|m| !m.trim().trim_end_matches('.').is_empty())
        .count();
    let erreurs_connues = section(md, "Erreurs connues")
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with('|') && !l.trim_matches(|c| c == '|' || c == '-' || c == ' ' || c == ':').is_empty())
        .count()
        .saturating_sub(1);
    Resume { personnes, mots_justes, erreurs_connues }
}

/// Écrit le lexique livré dans le dossier de l'app s'il n'y en a pas encore ; rend son chemin.
pub fn installer(dossier_de_l_app: &Path) -> std::io::Result<PathBuf> {
    let chemin = dossier_de_l_app.join("lexique.md");
    if !chemin.exists() {
        std::fs::create_dir_all(dossier_de_l_app)?;
        std::fs::write(&chemin, LEXIQUE_INITIAL)?;
    }
    Ok(chemin)
}
