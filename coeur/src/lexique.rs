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

/// Où est la section `## titre` : le début de son corps (après le titre) et sa fin (le titre
/// suivant, ou la fin du texte).
fn bornes(md: &str, titre: &str) -> Option<(usize, usize)> {
    let entete = format!("## {titre}");
    let mut debut = None;
    let mut position = 0;
    for ligne in md.split_inclusive('\n') {
        let fin = position + ligne.len();
        if let Some(d) = debut {
            if ligne.starts_with("## ") || ligne.starts_with("# ") {
                return Some((d, position));
            }
        } else if ligne.trim_end() == entete {
            debut = Some(fin);
        }
        position = fin;
    }
    debut.map(|d| (d, md.len()))
}

/// Le corps d'une section `## titre`, jusqu'à la section suivante.
fn section<'a>(md: &'a str, titre: &str) -> &'a str {
    bornes(md, titre).map_or("", |(d, f)| &md[d..f])
}

/// La section Vocabulaire, sur une ligne : le prompt de Whisper (vide : pas de prompt).
pub fn vocabulaire(md: &str) -> String {
    section(md, "Vocabulaire").split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Les noms de la section Personnes (`- Nom : rôle`, `- **Nom** — rôle`), dans l'ordre : ceux
/// qu'on propose pour nommer un locuteur.
pub fn personnes(md: &str) -> Vec<String> {
    section(md, "Personnes")
        .lines()
        .filter_map(|l| l.trim_start().strip_prefix("- "))
        .map(|p| p.split([':', '—', '(']).next().unwrap_or_default().replace("**", "").trim().to_string())
        .filter(|p| !p.is_empty())
        .collect()
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

/// Une erreur de transcription apprise : ce que Whisper a entendu, et ce qu'il fallait écrire.
#[derive(Clone, Debug, PartialEq)]
pub struct AjoutAuLexique {
    pub entendu: String,
    pub correct: String,
}

const TABLEAU: &str = "| Entendu | Correct |\n|---|---|\n";

/// Les cellules d'une ligne de tableau (`\|` est une barre dans une cellule).
fn cellules(ligne: &str) -> Vec<String> {
    let ligne = ligne.trim().trim_start_matches('|');
    let mut cellules = vec![String::new()];
    let mut car = ligne.chars().peekable();
    while let Some(c) = car.next() {
        match c {
            '\\' if car.peek() == Some(&'|') => cellules.last_mut().unwrap().push(car.next().unwrap()),
            '|' => cellules.push(String::new()),
            c => cellules.last_mut().unwrap().push(c),
        }
    }
    let cellules: Vec<String> = cellules.into_iter().map(|c| c.trim().to_string()).collect();
    match cellules.split_last() {
        Some((derniere, reste)) if derniere.is_empty() => reste.to_vec(),
        _ => cellules,
    }
}

fn est_un_separateur(ligne: &str) -> bool {
    ligne.trim().starts_with('|') && ligne.chars().all(|c| matches!(c, '|' | '-' | ':' | ' ' | '\t' | '\r' | '\n'))
}

/// Ce qui est déjà entendu dans les erreurs connues, en minuscules (chaque variante de la première
/// colonne, séparées par des virgules).
fn deja_entendu(md: &str) -> Vec<String> {
    let lignes: Vec<&str> = section(md, "Erreurs connues").lines().filter(|l| l.trim_start().starts_with('|')).collect();
    let corps = match lignes.iter().position(|l| est_un_separateur(l)) {
        Some(i) => &lignes[i + 1..],
        None => &lignes[..],
    };
    corps.iter().filter_map(|l| cellules(l).into_iter().next()).flat_map(|c| c.split(',').map(|v| v.trim().to_lowercase()).collect::<Vec<_>>()).filter(|v| !v.is_empty()).collect()
}

/// Ajoute au tableau des erreurs connues celles qui n'y sont pas encore. Sont écartées : les
/// entrées vides, celles qui ne changent que la casse, et celles dont la forme entendue est déjà
/// connue (une des variantes de la première colonne). Le tableau, ou la section, est créé s'il
/// manque. Rend le lexique et les erreurs vraiment ajoutées.
pub fn ajouter_erreurs(md: &str, ajouts: &[AjoutAuLexique]) -> (String, Vec<AjoutAuLexique>) {
    let mut connus = deja_entendu(md);
    let mut ajoutes = Vec::new();
    for a in ajouts {
        let (entendu, correct) = (a.entendu.trim(), a.correct.trim());
        let cle = entendu.to_lowercase();
        if entendu.is_empty() || correct.is_empty() || cle == correct.to_lowercase() || connus.contains(&cle) {
            continue;
        }
        connus.push(cle);
        ajoutes.push(AjoutAuLexique { entendu: entendu.into(), correct: correct.into() });
    }
    if ajoutes.is_empty() {
        return (md.to_string(), ajoutes);
    }
    let echapper = |s: &str| s.replace('|', "\\|");
    let lignes: String = ajoutes.iter().map(|a| format!("| {} | {} |\n", echapper(&a.entendu), echapper(&a.correct))).collect();
    let md = match bornes(md, "Erreurs connues") {
        None => {
            let mut s = md.to_string();
            if !s.is_empty() && !s.ends_with('\n') {
                s.push('\n');
            }
            format!("{s}\n## Erreurs connues\n\n{TABLEAU}{lignes}")
        }
        Some((debut, fin)) => {
            let corps = &md[debut..fin];
            // Après la dernière ligne du tableau.
            let mut position = None;
            let mut p = debut;
            for ligne in corps.split_inclusive('\n') {
                p += ligne.len();
                if ligne.trim_start().starts_with('|') {
                    position = Some(p);
                }
            }
            match position {
                Some(p) => {
                    let saut = if md[..p].ends_with('\n') { "" } else { "\n" };
                    format!("{}{saut}{lignes}{}", &md[..p], &md[p..])
                }
                None => {
                    let indication = corps.trim();
                    let indication = if indication.is_empty() { String::new() } else { format!("{indication}\n\n") };
                    let suite = if fin < md.len() { "\n" } else { "" };
                    format!("{}\n{indication}{TABLEAU}{lignes}{suite}{}", &md[..debut], &md[fin..])
                }
            }
        }
    };
    (md, ajoutes)
}

/// Réécrit le lexique d'un coup (fichier temporaire à côté, puis renommage) : un éditeur qui le
/// lit au même moment ne le voit jamais à moitié écrit.
pub fn ecrire(chemin: &Path, md: &str) -> std::io::Result<()> {
    let nom = chemin.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let temporaire = chemin.with_file_name(format!(".{nom}.greffier"));
    std::fs::write(&temporaire, md)?;
    std::fs::rename(&temporaire, chemin)
}
