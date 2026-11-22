//! The lexicon: a Markdown file in the app's folder, with four sections.
//!
//! - **Personnes** (people): one bullet per person (`- Name : role`);
//! - **Mots justes** (correct words): a paragraph of comma-separated words;
//! - **Erreurs connues** (known errors): a `| Entendu | Correct |` table, extended by the review;
//! - **Vocabulaire** (vocabulary): the sentence given to Whisper before transcription (it only
//!   reads about 220 tokens of it).
//!
//! A line in italics (`_…_`) is a hint from the template, not an entry.
//!
//! The app ships with an empty template lexicon; it writes it into its folder on first launch
//! and never touches it afterwards: it belongs to the user.

use std::path::{Path, PathBuf};

/// The lexicon shipped with the app.
pub const INITIAL_LEXICON: &str = include_str!("../resources/lexicon.md");

/// Where the `## title` section is: the start of its body (after the heading) and its end (the
/// next heading, or the end of the text).
fn bounds(md: &str, title: &str) -> Option<(usize, usize)> {
    let header = format!("## {title}");
    let mut start = None;
    let mut position = 0;
    for line in md.split_inclusive('\n') {
        let end = position + line.len();
        if let Some(d) = start {
            if line.starts_with("## ") || line.starts_with("# ") {
                return Some((d, position));
            }
        } else if line.trim_end() == header {
            start = Some(end);
        }
        position = end;
    }
    start.map(|d| (d, md.len()))
}

/// The body of a `## title` section, up to the next section.
fn section<'a>(md: &'a str, title: &str) -> &'a str {
    bounds(md, title).map_or("", |(d, f)| &md[d..f])
}

/// The Vocabulaire section, on one line: Whisper's prompt (empty: no prompt).
pub fn vocabulary(md: &str) -> String {
    section(md, "Vocabulaire").split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The names in the Personnes section (`- Name : role`, `- **Name** — role`), in order: those
/// offered to name a speaker.
pub fn people(md: &str) -> Vec<String> {
    section(md, "Personnes")
        .lines()
        .filter_map(|l| l.trim_start().strip_prefix("- "))
        .map(|p| p.split([':', '—', '(']).next().unwrap_or_default().replace("**", "").trim().to_string())
        .filter(|p| !p.is_empty())
        .collect()
}

/// What the lexicon counts, for the settings.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Overview {
    pub people: usize,
    pub correct_words: usize,
    pub known_errors: usize,
}

pub fn preview(md: &str) -> Overview {
    let people = section(md, "Personnes").lines().filter(|l| l.trim_start().starts_with("- ")).count();
    let correct_words = section(md, "Mots justes")
        .lines()
        .filter(|l| !l.trim_start().starts_with('_'))
        .flat_map(|l| l.split(','))
        .filter(|m| !m.trim().trim_end_matches('.').is_empty())
        .count();
    let known_errors = section(md, "Erreurs connues")
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with('|') && !l.trim_matches(|c| c == '|' || c == '-' || c == ' ' || c == ':').is_empty())
        .count()
        .saturating_sub(1);
    Overview { people, correct_words, known_errors }
}

/// Writes the shipped lexicon into the app's folder if there is none yet; returns its path.
pub fn install(app_folder: &Path) -> std::io::Result<PathBuf> {
    let path = app_folder.join("lexique.md");
    if !path.exists() {
        std::fs::create_dir_all(app_folder)?;
        std::fs::write(&path, INITIAL_LEXICON)?;
    }
    Ok(path)
}

/// A learned transcription error: what Whisper heard, and what should have been written.
#[derive(Clone, Debug, PartialEq)]
pub struct LexiconAddition {
    pub heard: String,
    pub correct: String,
}

const TABLE: &str = "| Entendu | Correct |\n|---|---|\n";

/// The cells of a table row (`\|` is a pipe inside a cell).
fn cells(line: &str) -> Vec<String> {
    let line = line.trim().trim_start_matches('|');
    let mut cells = vec![String::new()];
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&'|') => cells.last_mut().unwrap().push(chars.next().unwrap()),
            '|' => cells.push(String::new()),
            c => cells.last_mut().unwrap().push(c),
        }
    }
    let cells: Vec<String> = cells.into_iter().map(|c| c.trim().to_string()).collect();
    match cells.split_last() {
        Some((last, rest)) if last.is_empty() => rest.to_vec(),
        _ => cells,
    }
}

fn is_separator(line: &str) -> bool {
    line.trim().starts_with('|') && line.chars().all(|c| matches!(c, '|' | '-' | ':' | ' ' | '\t' | '\r' | '\n'))
}

/// What is already heard in the known errors, in lowercase (each variant of the first column,
/// comma-separated).
fn already_heard(md: &str) -> Vec<String> {
    let lines: Vec<&str> = section(md, "Erreurs connues").lines().filter(|l| l.trim_start().starts_with('|')).collect();
    let body = match lines.iter().position(|l| is_separator(l)) {
        Some(i) => &lines[i + 1..],
        None => &lines[..],
    };
    body.iter().filter_map(|l| cells(l).into_iter().next()).flat_map(|c| c.split(',').map(|v| v.trim().to_lowercase()).collect::<Vec<_>>()).filter(|v| !v.is_empty()).collect()
}

/// Adds to the known errors table those not in it yet. Skipped: empty entries, those that only
/// change case, and those whose heard form is already known (one of the first column's
/// variants). The table, or the section, is created if missing. Returns the lexicon and the
/// errors actually added.
pub fn add_errors(md: &str, additions: &[LexiconAddition]) -> (String, Vec<LexiconAddition>) {
    let mut known = already_heard(md);
    let mut added = Vec::new();
    for a in additions {
        let (heard, correct) = (a.heard.trim(), a.correct.trim());
        let key = heard.to_lowercase();
        if heard.is_empty() || correct.is_empty() || key == correct.to_lowercase() || known.contains(&key) {
            continue;
        }
        known.push(key);
        added.push(LexiconAddition { heard: heard.into(), correct: correct.into() });
    }
    if added.is_empty() {
        return (md.to_string(), added);
    }
    let escape = |s: &str| s.replace('|', "\\|");
    let lines: String = added.iter().map(|a| format!("| {} | {} |\n", escape(&a.heard), escape(&a.correct))).collect();
    let md = match bounds(md, "Erreurs connues") {
        None => {
            let mut s = md.to_string();
            if !s.is_empty() && !s.ends_with('\n') {
                s.push('\n');
            }
            format!("{s}\n## Erreurs connues\n\n{TABLE}{lines}")
        }
        Some((start, end)) => {
            let body = &md[start..end];
            // After the table's last row.
            let mut position = None;
            let mut p = start;
            for line in body.split_inclusive('\n') {
                p += line.len();
                if line.trim_start().starts_with('|') {
                    position = Some(p);
                }
            }
            match position {
                Some(p) => {
                    let newline = if md[..p].ends_with('\n') { "" } else { "\n" };
                    format!("{}{newline}{lines}{}", &md[..p], &md[p..])
                }
                None => {
                    let indication = body.trim();
                    let indication = if indication.is_empty() { String::new() } else { format!("{indication}\n\n") };
                    let sequence = if end < md.len() { "\n" } else { "" };
                    format!("{}\n{indication}{TABLE}{lines}{sequence}{}", &md[..start], &md[end..])
                }
            }
        }
    };
    (md, added)
}

/// Rewrites the lexicon in one go (temporary file alongside, then rename): an editor reading it
/// at the same moment never sees it half-written.
pub fn write(path: &Path, md: &str) -> std::io::Result<()> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let temp = path.with_file_name(format!(".{name}.greffier"));
    std::fs::write(&temp, md)?;
    std::fs::rename(&temp, path)
}
