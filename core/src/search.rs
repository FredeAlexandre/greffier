//! La recherche ⌘K dans les titres, les bilans et les transcriptions de toutes les réunions.
//!
//! Le cœur tient un index : pour chaque dossier de réunion, son titre, les lignes de son bilan
//! (avec l'instant de leur sujet) et ses répliques (avec leur début et leur locuteur). Une entrée
//! est refaite quand les empreintes des fichiers de la réunion changent (écriture de Greffier ou
//! modification hors de l'app) ; elle disparaît avec la réunion (suppression, renommage).
//!
//! La recherche ignore la casse et les accents (« equipe » trouve « Équipe ») et l'apostrophe
//! typographique. Un résultat dit où ouvrir la réunion : la réplique, ou le sujet (la décision)
//! d'une ligne du bilan.
//!
//! **Le cache** (ADR 0001 : le seul état de l'app hors du dossier de réunions, reconstructible) :
//! un fichier JSON par réunion, `<cache>/<nom du dossier>.json`, avec les empreintes des fichiers
//! dont il vient. Au lancement, une entrée du cache sert si ses empreintes sont celles du
//! dossier ; sinon (fichiers changés app fermée, cache abîmé, absent, d'une autre version) elle
//! est refaite depuis la réunion. Ensuite, chaque entrée refaite est réécrite, chaque entrée
//! retirée est effacée.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::summary::first_timestamp;
use crate::folder::watch::Fingerprints;
use crate::meeting::{MeetingId, Meeting};
use crate::Greffier;

/// Où le mot a été trouvé.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Location {
    Title,
    Summary,
    Transcription,
}

/// Un passage trouvé.
#[derive(Clone, Debug, PartialEq)]
pub struct SearchResult {
    pub meeting: MeetingId,
    pub location: Location,
    /// L'instant où ouvrir la réunion, en secondes : le début de la réplique, ou celui du sujet
    /// (de la décision) pour une ligne du bilan. Rien : le début de la réunion.
    pub instant: Option<f64>,
    /// Le locuteur de la réplique, tel qu'il s'affiche.
    pub speaker: Option<String>,
    /// Le passage autour du mot trouvé (coupé par « … »).
    pub excerpt: String,
    /// Le mot trouvé dans `extrait`, en octets.
    pub highlight: (usize, usize),
}

/// Au plus tant de lignes du bilan, de répliques par réunion, et de résultats en tout.
const SUMMARY_LINES: usize = 3;
const UTTERANCES: usize = 4;
const RESULTS: usize = 40;

/// La version du format du cache : une autre version est refaite.
const VERSION: u32 = 1;

/// Une ligne cherchable : le texte, sa version pliée (même nombre de caractères), son instant.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Line {
    text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    instant: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    speaker: Option<String>,
    #[serde(skip)]
    folded: String,
}

impl Line {
    fn new(text: String, instant: Option<f64>, speaker: Option<String>) -> Line {
        let folded = fold_text(&text);
        Line { text, instant, speaker, folded }
    }

    /// Le mot `requete` (déjà plié) dans la ligne : l'extrait et le passage surligné.
    fn find(&self, query: &str) -> Option<(String, (usize, usize))> {
        let i = self.folded.find(query)?;
        let start = self.folded[..i].chars().count();
        let end = start + query.chars().count();
        let byte = |n: usize| self.text.char_indices().nth(n).map_or(self.text.len(), |(o, _)| o);
        Some(excerpt(&self.text, byte(start), byte(end)))
    }
}

/// Ce que l'index retient d'une réunion (et le fichier du cache).
#[derive(Clone, Debug, Serialize, Deserialize)]
struct IndexEntry {
    version: u32,
    /// Les empreintes des fichiers quand l'entrée a été faite.
    fingerprints: Fingerprints,
    title: Option<Line>,
    summary: Vec<Line>,
    transcription: Vec<Line>,
}

impl IndexEntry {
    fn from(r: &Meeting, fingerprints: Fingerprints) -> IndexEntry {
        let title = (!r.title.trim().is_empty() && r.title != "Réunion").then(|| Line::new(r.title.clone(), None, None));
        let transcription = r
            .utterances
            .iter()
            .map(|u| {
                let speaker = r.speakers.get(r.displayed_speaker(u.speaker)).map(|l| l.display_name());
                Line::new(u.text.clone(), Some(u.start), speaker)
            })
            .collect();
        IndexEntry { version: VERSION, fingerprints, title, summary: summary_lines(&r.summary), transcription }
    }

    /// L'entrée du cache, si elle est lisible et de cette version.
    fn read(path: &Path) -> Option<IndexEntry> {
        let mut e: IndexEntry = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
        if e.version != VERSION {
            return None;
        }
        for l in e.title.iter_mut().chain(&mut e.summary).chain(&mut e.transcription) {
            l.folded = fold_text(&l.text);
        }
        Some(e)
    }
}

/// Les lignes du bilan, sans les titres de section. Une ligne prend son propre horodatage (une
/// décision), sinon celui du sujet `### Titre (h:mm:ss)` qui la contient.
fn summary_lines(md: &str) -> Vec<Line> {
    let mut topic = None;
    let mut lines = Vec::new();
    for line in md.lines() {
        let t = line.trim();
        if t.is_empty() || t == "---" {
            continue;
        }
        if t.starts_with("# ") || t.starts_with("## ") {
            topic = None;
            continue;
        }
        if let Some(title) = t.strip_prefix("### ") {
            topic = first_timestamp(title);
        }
        let mut text = t.trim_start_matches(['#', '-', '*', '>', '|', ' ']);
        for case in ["[ ] ", "[x] ", "[X] "] {
            text = text.strip_prefix(case).unwrap_or(text);
        }
        let text = text.replace("**", "");
        if !text.trim().is_empty() {
            lines.push(Line::new(text.trim().to_string(), first_timestamp(t).or(topic), None));
        }
    }
    lines
}

/// Minuscules, sans accents, apostrophe droite : un caractère pour un caractère, pour retrouver
/// le passage dans le texte d'origine.
fn fold_text(s: &str) -> String {
    s.chars().map(fold_char).collect()
}

fn fold_char(c: char) -> char {
    let mut l = c.to_lowercase();
    let c = match (l.next(), l.next()) {
        (Some(x), None) => x,
        _ => c,
    };
    match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => 'a',
        'ç' => 'c',
        'è' | 'é' | 'ê' | 'ë' => 'e',
        'ì' | 'í' | 'î' | 'ï' => 'i',
        'ñ' => 'n',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' => 'o',
        'ù' | 'ú' | 'û' | 'ü' => 'u',
        'ý' | 'ÿ' => 'y',
        '’' | '‘' => '\'',
        _ => c,
    }
}

/// Un extrait de `s` autour de `[a, b)` (en octets), coupé entre deux mots, et la place du
/// passage dans l'extrait.
fn excerpt(s: &str, a: usize, b: usize) -> (String, (usize, usize)) {
    let mut start = a.saturating_sub(50);
    while !s.is_char_boundary(start) {
        start -= 1;
    }
    let mut end = (b + 90).min(s.len());
    while !s.is_char_boundary(end) {
        end += 1;
    }
    // Couper entre deux mots plutôt qu'au milieu d'un mot.
    if start > 0 && let Some(i) = s[start..a].find(' ') {
        start += i + 1;
    }
    if end < s.len() && let Some(i) = s[b..end].rfind(' ') {
        end = b + i;
    }
    let before = if start > 0 { "…" } else { "" };
    let after = if end < s.len() { "…" } else { "" };
    let offset = before.len() + a - start;
    (format!("{before}{}{after}", &s[start..end]), (offset, offset + (b - a)))
}

/// L'index de recherche, par nom de dossier de réunion.
#[derive(Default)]
pub(crate) struct Index {
    entries: HashMap<String, IndexEntry>,
    /// Le dossier du cache, s'il y en a un.
    cache: Option<PathBuf>,
}

impl Index {
    fn file(cache: &Path, name: &str) -> PathBuf {
        cache.join(format!("{name}.json"))
    }

    /// Écrit l'entrée dans le cache (sans erreur : le cache se refait).
    fn write(&self, name: &str, e: &IndexEntry) {
        let Some(cache) = &self.cache else { return };
        let Ok(json) = serde_json::to_vec(e) else { return };
        let partial = cache.join(format!(".{name}.json.partial"));
        if std::fs::create_dir_all(cache).is_ok() && std::fs::write(&partial, json).is_ok() {
            let _ = std::fs::rename(&partial, Index::file(cache, name));
        }
    }

    fn erase(&self, name: &str) {
        if let Some(cache) = &self.cache {
            let _ = std::fs::remove_file(Index::file(cache, name));
        }
    }

    /// Prend `cache` comme dossier du cache : l'index repart des entrées qui y sont à jour
    /// (`a_jour(nom, empreintes)`), les autres fichiers sont effacés.
    fn open(&mut self, cache: Option<PathBuf>, up_to_date: impl Fn(&str, &Fingerprints) -> bool) {
        self.entries.clear();
        self.cache = cache;
        let Some(cache) = &self.cache else { return };
        for f in std::fs::read_dir(cache).into_iter().flatten().flatten() {
            let path = f.path();
            let name = f.file_name().to_string_lossy().strip_suffix(".json").map(str::to_owned);
            match name.and_then(|name| Some((IndexEntry::read(&path).filter(|e| up_to_date(&name, &e.fingerprints))?, name))) {
                Some((e, name)) => _ = self.entries.insert(name, e),
                // Périmé, illisible, ou une écriture interrompue (`.….json.partiel`).
                None => _ = std::fs::remove_file(&path),
            }
        }
    }
}

impl Greffier {
    /// Les passages qui contiennent `requete` (sans tenir compte de la casse ni des accents),
    /// des réunions les plus récentes aux plus anciennes : pour chacune, le titre, puis quelques
    /// lignes du bilan, puis quelques répliques.
    pub fn search(&self, query: &str) -> Vec<SearchResult> {
        let q = fold_text(query.trim());
        let mut results = Vec::new();
        if q.is_empty() {
            return results;
        }
        for r in &self.meetings {
            let Some(e) = self.index.entries.get(&r.folder_name) else { continue };
            let title = e.title.as_ref().and_then(|l| Some((Location::Title, (l, l.find(&q)?))));
            let summary = e.summary.iter().filter_map(|l| Some((l, l.find(&q)?))).take(SUMMARY_LINES).map(|x| (Location::Summary, x));
            let transcription = e.transcription.iter().filter_map(|l| Some((l, l.find(&q)?))).take(UTTERANCES).map(|x| (Location::Transcription, x));
            for (location, (line, (excerpt, highlight))) in title.into_iter().chain(summary).chain(transcription) {
                results.push(SearchResult { meeting: r.id, location, instant: line.instant, speaker: line.speaker.clone(), excerpt, highlight });
            }
            if results.len() >= RESULTS {
                break;
            }
        }
        results
    }

    /// Met l'index à jour : une entrée refaite pour chaque réunion dont les fichiers ont changé
    /// depuis, une entrée retirée pour chaque réunion qui n'existe plus.
    pub(crate) fn indexer(&mut self) {
        for r in &self.meetings {
            let fingerprints = self.fingerprints.get(&r.folder_name).cloned().unwrap_or_default();
            if self.index.entries.get(&r.folder_name).is_some_and(|e| e.fingerprints == fingerprints) {
                continue;
            }
            let e = IndexEntry::from(r, fingerprints);
            self.index.write(&r.folder_name, &e);
            self.index.entries.insert(r.folder_name.clone(), e);
        }
        let names: HashSet<&str> = self.meetings.iter().map(|r| r.folder_name.as_str()).collect();
        let gone: Vec<String> = self.index.entries.keys().filter(|name| !names.contains(name.as_str())).cloned().collect();
        for name in gone {
            self.index.erase(&name);
            self.index.entries.remove(&name);
        }
    }

    /// Garde l'index dans ce dossier (ou en mémoire seulement) : voir le cache plus haut.
    pub(crate) fn use_search_cache(&mut self, cache: Option<PathBuf>) {
        let expected: HashMap<&str, Fingerprints> = self.meetings.iter().map(|r| (r.folder_name.as_str(), self.fingerprints.get(&r.folder_name).cloned().unwrap_or_default())).collect();
        self.index.open(cache, |name, e| expected.get(name) == Some(e));
        self.indexer();
    }
}
