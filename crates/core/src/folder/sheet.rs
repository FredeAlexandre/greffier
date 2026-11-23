//! The sheet (`… - réunion.md`): a meeting's entry file, written by Greffier alone.
//!
//! ```markdown
//! ---
//! titre: "Revue de release"
//! date: 2026-11-20
//! debut: "10:00"
//! duree: "0:12:34"
//! statut: echec                 # enregistrement_en_cours, enregistrement_interrompu, a_traiter,
//! etape: relecture              #   en_attente, en_cours, echec, terminee; `etape` for the three
//! erreur: "La CLI claude …"     #   before last, `erreur` for a failure
//! etape_faite: transcription    # the last stage completed
//! nombre_de_locuteurs: 3        # absent: detected
//! locuteurs:                    # SPEAKER_xx → name given by the user (merges included)
//!   SPEAKER_00: "Alexandre"
//!   SPEAKER_02: "Alexandre"
//! fusions:                      # SPEAKER_xx merged into SPEAKER_yy
//!   SPEAKER_02: SPEAKER_00
//! noms_avant_fusion:            # a merged speaker's own name, given back at split
//!   SPEAKER_02: "Alex"
//! noms_pretes:                  # SPEAKER_xx bears the name SPEAKER_yy lent at merge, as it had
//!   SPEAKER_04: SPEAKER_03      #   none: it goes back to SPEAKER_yy at split
//! propositions:                 # what the summary agent proposes
//!   SPEAKER_01:
//!     nom: "Khalid"
//!     justification: "Alexandre l'appelle Khalid à 0:00:04."
//!     meme_voix_que: SPEAKER_00
//! reperes:
//!   - instant: "0:05:12"
//!     libelle: "Repère 1"
//! sessions:                     # the agent's session ids
//!   relecture: "…"
//!   bilan: "…"
//! nouvelle: true                # the summary has not been opened yet
//! ---
//!
//! # Revue de release
//!
//! Le résumé.
//!
//! ## Actions principales
//!
//! - Alexandre — publier la release — lundi
//!
//! ## Fichiers
//!
//! - [Bilan](<2026-11-20 10h00 - Revue de release - bilan.md>)
//! ```
//!
//! The front matter is a subset of YAML (tables, lists, double-quoted strings escaped as in
//! JSON): any YAML reader reads it, and this module reads back what it writes.

use crate::folder::Role;
use crate::meeting::{Stage, Marker, Meeting, Sessions, Status};
use crate::time::{DateTime, playback_timestamp};

/// What the summary agent proposes for a speaker.
#[derive(Clone, Debug, PartialEq)]
pub struct Proposal {
    /// « SPEAKER_01 »
    pub speaker: String,
    /// Empty: the agent could not tell who it is (the justification says why).
    pub name: String,
    pub justification: String,
    /// Probably the same voice as another speaker ("SPEAKER_00").
    pub same_voice_as: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Sheet {
    pub title: String,
    pub start: DateTime,
    /// In seconds (written to the nearest second).
    pub duration: f64,
    /// `Running` is written without its progress (read back as 0).
    pub status: Status,
    pub completed_stage: Option<Stage>,
    pub speaker_count: Option<u8>,
    /// The given names: ("SPEAKER_00", "Alexandre"), merges included (a merged speaker carries
    /// the name of the one that replaces it).
    pub names: Vec<(String, String)>,
    /// The merges: ("SPEAKER_02", "SPEAKER_00") — the first is the same voice as the second.
    pub merges: Vec<(String, String)>,
    /// The merged speakers' own names, which `names` hides: ("SPEAKER_02", "Alex"). Absent from
    /// sheets written before they were kept.
    pub names_before_merge: Vec<(String, String)>,
    /// ("SPEAKER_04", "SPEAKER_03"): SPEAKER_04's name is the one SPEAKER_03 lent it at merge.
    pub lent_names: Vec<(String, String)>,
    pub proposals: Vec<Proposal>,
    pub markers: Vec<Marker>,
    pub sessions: Sessions,
    pub is_new: bool,
    pub brief: String,
    pub key_actions: Vec<String>,
}

impl Sheet {
    pub fn from_meeting(r: &Meeting) -> Sheet {
        let label = |i: usize| r.speakers[i].label.clone();
        Sheet {
            title: r.title.clone(),
            start: r.start,
            duration: r.duration,
            status: r.status.clone(),
            completed_stage: r.completed_stage,
            speaker_count: r.speaker_count,
            names: (0..r.speakers.len()).filter_map(|i| Some((label(i), r.speaker_name(i)?.to_string()))).collect(),
            merges: r.speakers.iter().filter_map(|l| Some((l.label.clone(), label(l.merged_into?)))).collect(),
            names_before_merge: r.merged_names().collect(),
            lent_names: r.lent_names().collect(),
            proposals: r
                .speakers
                .iter()
                .filter(|l| l.proposal.is_some() || !l.justification.is_empty())
                .map(|l| Proposal {
                    speaker: l.label.clone(),
                    name: l.proposal.clone().unwrap_or_default(),
                    justification: l.justification.clone(),
                    same_voice_as: l.suggested_merge.map(label),
                })
                .collect(),
            markers: r.markers.clone(),
            sessions: r.sessions.clone(),
            is_new: r.is_new,
            brief: r.brief.clone(),
            key_actions: r.key_actions.clone(),
        }
    }

    /// Applies the sheet to a meeting whose speakers are already known (those of the
    /// transcription); a speaker named in the sheet but unknown is ignored.
    pub fn apply(&self, r: &mut Meeting) {
        r.title = self.title.clone();
        r.start = self.start;
        if self.duration > 0.0 {
            r.duration = self.duration;
        }
        r.status = self.status.clone();
        r.speaker_count = self.speaker_count;
        let index = |r: &Meeting, e: &str| r.speakers.iter().position(|l| l.label == e);
        // The sheet is authoritative for names and merges (the structured transcription keeps a
        // copy).
        for l in &mut r.speakers {
            l.name = None;
            l.merged_into = None;
        }
        for (e, name) in &self.names {
            if let Some(i) = index(r, e) {
                r.speakers[i].name = Some(name.clone());
            }
        }
        for (e, into) in &self.merges {
            if let (Some(i), Some(j)) = (index(r, e), index(r, into)) {
                r.restore_merge(i, j);
            }
        }
        r.restore_merged_names(self.names_before_merge.iter().map(|(a, b)| (a, b)), self.lent_names.iter().map(|(a, b)| (a, b)));
        for p in &self.proposals {
            if let Some(i) = index(r, &p.speaker) {
                let same = p.same_voice_as.as_deref().and_then(|e| index(r, e)).filter(|j| *j != i);
                let l = &mut r.speakers[i];
                l.proposal = Some(p.name.clone()).filter(|n| !n.is_empty());
                l.justification = p.justification.clone();
                l.suggested_merge = same;
            }
        }
        r.markers = self.markers.clone();
        r.sessions = self.sessions.clone();
        r.is_new = self.is_new;
        r.brief = self.brief.clone();
        r.key_actions = self.key_actions.clone();
    }

    /// The sheet's file, for the folder `folder_name`. `recording` is the recording's role and
    /// extension as on disk (`enregistrement.mp4`, or `enregistrement.wav` for an imported audio
    /// file that stayed as is), so that its link leads to a file that exists.
    pub fn markdown(&self, folder_name: &str, recording: &str) -> String {
        let mut y = String::from("---\n");
        let mut line = |key: &str, value: String| y.push_str(&format!("{key}: {value}\n"));
        line("titre", c_string(&self.title));
        line("date", format!("{:04}-{:02}-{:02}", self.start.year, self.start.month, self.start.day));
        line("debut", c_string(&format!("{:02}:{:02}", self.start.hour, self.start.minute)));
        line("duree", c_string(&instant(self.duration)));
        let (status, stage, error) = match &self.status {
            Status::Recording => ("enregistrement_en_cours", None, None),
            Status::RecordingInterrupted => ("enregistrement_interrompu", None, None),
            Status::ToProcess => ("a_traiter", None, None),
            Status::Pending(e) => ("en_attente", Some(*e), None),
            Status::Running { stage, .. } => ("en_cours", Some(*stage), None),
            Status::Failed { stage, message } => ("echec", Some(*stage), Some(message)),
            Status::Done => ("terminee", None, None),
        };
        line("statut", status.into());
        if let Some(e) = stage {
            line("etape", stage_name(e).into());
        }
        if let Some(m) = error {
            line("erreur", c_string(m));
        }
        if let Some(e) = self.completed_stage {
            line("etape_faite", stage_name(e).into());
        }
        if let Some(n) = self.speaker_count {
            line("nombre_de_locuteurs", n.to_string());
        }
        if !self.names.is_empty() {
            y.push_str("locuteurs:\n");
            for (e, name) in &self.names {
                y.push_str(&format!("  {e}: {}\n", c_string(name)));
            }
        }
        if !self.merges.is_empty() {
            y.push_str("fusions:\n");
            for (e, into) in &self.merges {
                y.push_str(&format!("  {e}: {into}\n"));
            }
        }
        if !self.names_before_merge.is_empty() {
            y.push_str("noms_avant_fusion:\n");
            for (e, name) in &self.names_before_merge {
                y.push_str(&format!("  {e}: {}\n", c_string(name)));
            }
        }
        if !self.lent_names.is_empty() {
            y.push_str("noms_pretes:\n");
            for (e, lender) in &self.lent_names {
                y.push_str(&format!("  {e}: {lender}\n"));
            }
        }
        if !self.proposals.is_empty() {
            y.push_str("propositions:\n");
            for p in &self.proposals {
                y.push_str(&format!("  {}:\n    nom: {}\n    justification: {}\n", p.speaker, c_string(&p.name), c_string(&p.justification)));
                if let Some(m) = &p.same_voice_as {
                    y.push_str(&format!("    meme_voix_que: {m}\n"));
                }
            }
        }
        if !self.markers.is_empty() {
            y.push_str("reperes:\n");
            for r in &self.markers {
                y.push_str(&format!("  - instant: {}\n    libelle: {}\n", c_string(&instant(r.instant)), c_string(&r.label)));
            }
        }
        if self.sessions != Sessions::default() {
            y.push_str("sessions:\n");
            for (key, s) in [("relecture", &self.sessions.review), ("bilan", &self.sessions.summary)] {
                if let Some(s) = s {
                    y.push_str(&format!("  {key}: {}\n", c_string(s)));
                }
            }
        }
        if self.is_new {
            y.push_str("nouvelle: true\n");
        }
        y.push_str("---\n\n");
        y.push_str(&format!("# {}\n\n", self.title));
        if !self.brief.is_empty() {
            y.push_str(&format!("{}\n\n", self.brief.trim()));
        }
        if !self.key_actions.is_empty() {
            y.push_str("## Actions principales\n\n");
            for a in &self.key_actions {
                y.push_str(&format!("- {a}\n"));
            }
            y.push('\n');
        }
        y.push_str("## Fichiers\n\n");
        for (label, role) in [
            ("Bilan", Role::Summary),
            ("Transcription", Role::ReadableTranscription),
            ("Transcription structurée", Role::StructuredTranscription),
            ("Enregistrement", Role::Recording),
        ] {
            let suffix = if role == Role::Recording { recording } else { role.suffix() };
            y.push_str(&format!("- [{label}](<{folder_name} - {suffix}>)\n"));
        }
        y
    }

    /// Reads a sheet back. Unknown keys are ignored; a front matter is required.
    pub fn read(md: &str) -> Result<Sheet, String> {
        let md = md.strip_prefix('\u{feff}').unwrap_or(md);
        let rest = md.strip_prefix("---\n").or_else(|| md.strip_prefix("---\r\n")).ok_or("la fiche n'a pas de frontmatter")?;
        let end = rest.find("\n---").ok_or("le frontmatter de la fiche n'est pas fermé")?;
        let (yaml, body) = (&rest[..end + 1], &rest[end + 4..]);
        let root = yaml::read(yaml)?;
        let text = |key: &str| root.get(key).and_then(yaml::Value::text);

        let date = text("date").ok_or("la fiche n'a pas de date")?;
        let (year, month, day) = read_date(date).ok_or_else(|| format!("date illisible : {date}"))?;
        let (hour, minute) = text("debut").and_then(read_time).unwrap_or((0, 0));
        let stage = |key: &str| text(key).and_then(read_stage);
        let status = match text("statut").unwrap_or("a_traiter") {
            "enregistrement_en_cours" => Status::Recording,
            "enregistrement_interrompu" => Status::RecordingInterrupted,
            "en_attente" => Status::Pending(stage("etape").unwrap_or(Stage::Transcription)),
            "en_cours" => Status::Running { stage: stage("etape").unwrap_or(Stage::Transcription), progress: 0.0 },
            "echec" => Status::Failed { stage: stage("etape").unwrap_or(Stage::Transcription), message: text("erreur").unwrap_or_default().to_string() },
            "terminee" => Status::Done,
            _ => Status::ToProcess,
        };
        let pairs = |key: &str| -> Vec<(String, String)> {
            root.get(key).map(|t| t.entries().filter_map(|(k, v)| Some((k.to_string(), v.text()?.to_string()))).collect()).unwrap_or_default()
        };
        let proposals = root
            .get("propositions")
            .map(|t| {
                t.entries()
                    .filter_map(|(k, v)| {
                        Some(Proposal {
                            speaker: k.to_string(),
                            name: v.get("nom")?.text()?.to_string(),
                            justification: v.get("justification").and_then(yaml::Value::text).unwrap_or_default().to_string(),
                            same_voice_as: v.get("meme_voix_que").and_then(yaml::Value::text).map(str::to_string),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        let markers = root
            .get("reperes")
            .map(|l| {
                l.elements()
                    .filter_map(|v| {
                        Some(Marker {
                            instant: read_timestamp(v.get("instant")?.text()?)?,
                            label: v.get("libelle").and_then(yaml::Value::text).unwrap_or_default().to_string(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        let session = |key: &str| root.get("sessions").and_then(|s| s.get(key)).and_then(yaml::Value::text).map(str::to_string);
        let (brief, key_actions) = read_body(body);
        Ok(Sheet {
            title: text("titre").unwrap_or("Réunion").to_string(),
            start: DateTime::new(year, month, day, hour, minute),
            duration: text("duree").and_then(read_timestamp).unwrap_or(0.0),
            status,
            completed_stage: stage("etape_faite"),
            speaker_count: text("nombre_de_locuteurs").and_then(|n| n.parse().ok()),
            names: pairs("locuteurs"),
            merges: pairs("fusions"),
            names_before_merge: pairs("noms_avant_fusion"),
            lent_names: pairs("noms_pretes"),
            proposals,
            markers,
            sessions: Sessions { review: session("relecture"), summary: session("bilan") },
            is_new: text("nouvelle") == Some("true"),
            brief,
            key_actions,
        })
    }
}

/// The brief (between the title and the first section) and the key actions.
fn read_body(body: &str) -> (String, Vec<String>) {
    let mut brief = Vec::new();
    let mut actions = Vec::new();
    let mut section: Option<&str> = None;
    for l in body.lines() {
        if let Some(h) = l.strip_prefix("## ") {
            section = Some(h.trim());
        } else if l.starts_with("# ") {
            continue;
        } else if section.is_none() {
            brief.push(l);
        } else if section == Some("Actions principales")
            && let Some(a) = l.trim_start().strip_prefix("- ")
        {
            actions.push(a.trim().to_string());
        }
    }
    (brief.join("\n").trim().to_string(), actions)
}

pub(crate) fn stage_name(e: Stage) -> &'static str {
    match e {
        Stage::Transcription => "transcription",
        Stage::Review => "relecture",
        Stage::Summary => "bilan",
    }
}

fn read_stage(s: &str) -> Option<Stage> {
    Stage::ALL.into_iter().find(|e| stage_name(*e) == s)
}

/// A double-quoted YAML string (JSON's escapes are a subset of YAML's).
fn c_string(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}

fn read_date(s: &str) -> Option<(i32, u8, u8)> {
    let mut it = s.trim().splitn(3, '-');
    Some((it.next()?.parse().ok()?, it.next()?.parse().ok()?, it.next()?.parse().ok()?))
}

fn read_time(s: &str) -> Option<(u8, u8)> {
    let (h, m) = s.trim().split_once([':', 'h'])?;
    Some((h.parse().ok()?, m.parse().ok()?))
}

/// "0:12:34", with hundredths if any ("0:12:34.5").
fn instant(t: f64) -> String {
    let cs = (t.max(0.0) * 100.0).round() as u64;
    let base = playback_timestamp((cs / 100) as f64);
    match cs % 100 {
        0 => base,
        c if c % 10 == 0 => format!("{base}.{}", c / 10),
        c => format!("{base}.{c:02}"),
    }
}

/// "0:12:34" or "12:34" in seconds.
fn read_timestamp(s: &str) -> Option<f64> {
    s.trim().split(':').try_fold(0.0, |acc, x| Some(acc * 60.0 + x.parse::<f64>().ok()?))
}

/// The sheet's subset of YAML.
mod yaml {
    #[derive(Clone, Debug, PartialEq)]
    pub enum Value {
        Text(String),
        List(Vec<Value>),
        Table(Vec<(String, Value)>),
    }

    impl Value {
        pub fn text(&self) -> Option<&str> {
            match self {
                Value::Text(s) => Some(s),
                _ => None,
            }
        }
        pub fn get(&self, key: &str) -> Option<&Value> {
            self.entries().find(|(k, _)| *k == key).map(|(_, v)| v)
        }
        pub fn entries(&self) -> impl Iterator<Item = (&str, &Value)> {
            let t: &[(String, Value)] = match self {
                Value::Table(t) => t,
                _ => &[],
            };
            t.iter().map(|(k, v)| (k.as_str(), v))
        }
        pub fn elements(&self) -> impl Iterator<Item = &Value> {
            let l: &[Value] = match self {
                Value::List(l) => l,
                _ => &[],
            };
            l.iter()
        }
    }

    pub fn read(text: &str) -> Result<Value, String> {
        let mut lines: Vec<(usize, String)> = text
            .lines()
            .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
            .map(|l| (l.len() - l.trim_start().len(), l.trim().to_string()))
            .collect();
        let mut i = 0;
        let v = block(&mut lines, &mut i, 0);
        match lines.get(i) {
            None => Ok(v),
            Some((_, l)) => Err(format!("frontmatter illisible à « {l} »")),
        }
    }

    fn is_dash(l: &str) -> bool {
        l == "-" || l.starts_with("- ")
    }

    fn block(l: &mut Vec<(usize, String)>, i: &mut usize, indent: usize) -> Value {
        if l.get(*i).is_some_and(|(_, x)| is_dash(x)) { list(l, i, indent) } else { table(l, i, indent) }
    }

    /// `key: value` or `key:`; returns the key and the rest.
    fn key(l: &str) -> Option<(&str, &str)> {
        if l.starts_with(['"', '\'']) {
            return None;
        }
        if let Some(k) = l.strip_suffix(':') {
            return Some((k.trim(), ""));
        }
        let (k, v) = l.split_once(": ")?;
        Some((k.trim(), v.trim()))
    }

    fn table(l: &mut Vec<(usize, String)>, i: &mut usize, indent: usize) -> Value {
        let mut t = Vec::new();
        while let Some((r, line)) = l.get(*i).cloned() {
            if r != indent || is_dash(&line) {
                break;
            }
            let Some((k, v)) = key(&line) else { break };
            *i += 1;
            let value = if !v.is_empty() {
                Value::Text(scalar(v))
            } else {
                match l.get(*i) {
                    Some((r2, x)) if *r2 > indent || (*r2 == indent && is_dash(x)) => {
                        let r2 = *r2;
                        block(l, i, r2)
                    }
                    _ => Value::Text(String::new()),
                }
            };
            t.push((k.to_string(), value));
        }
        Value::Table(t)
    }

    fn list(l: &mut Vec<(usize, String)>, i: &mut usize, indent: usize) -> Value {
        let mut v = Vec::new();
        while let Some((r, line)) = l.get(*i).cloned() {
            if r != indent || !is_dash(&line) {
                break;
            }
            let element = line[1..].trim().to_string();
            if element.is_empty() {
                *i += 1;
                match l.get(*i) {
                    Some((r2, _)) if *r2 > indent => {
                        let r2 = *r2;
                        v.push(block(l, i, r2));
                    }
                    _ => v.push(Value::Text(String::new())),
                }
            } else if key(&element).is_some() {
                // `- key: value` opens a table aligned on the key.
                l[*i] = (indent + 2, element);
                v.push(table(l, i, indent + 2));
            } else {
                *i += 1;
                v.push(Value::Text(scalar(&element)));
            }
        }
        Value::List(v)
    }

    fn scalar(s: &str) -> String {
        let s = s.trim();
        if s.starts_with('"') {
            if let Ok(v) = serde_json::from_str::<String>(s) {
                return v;
            }
            // A comment after the string.
            if let Some(end) = s[1..].find("\" #").or_else(|| s[1..].rfind('"')) {
                return serde_json::from_str::<String>(&s[..end + 2]).unwrap_or_else(|_| s[1..end + 1].to_string());
            }
        }
        if let Some(r) = s.strip_prefix('\'')
            && let Some(end) = r.rfind('\'')
        {
            return r[..end].replace("''", "'");
        }
        match s.find(" #") {
            Some(i) => s[..i].trim().to_string(),
            None => s.to_string(),
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn reads_tables_lists_and_strings() {
            let v = read("a: \"x: \\\"y\\\"\" # note\nb:\n  c: 1\n  d:\n    e: 'l''f'\nl:\n  - k: 1\n    m: 2\n  - 3\nn:\n- 4\n").unwrap();
            assert_eq!(v.get("a").and_then(Value::text), Some("x: \"y\""));
            assert_eq!(v.get("b").and_then(|b| b.get("d")).and_then(|d| d.get("e")).and_then(Value::text), Some("l'f"));
            let l: Vec<&Value> = v.get("l").unwrap().elements().collect();
            assert_eq!(l[0].get("m").and_then(Value::text), Some("2"));
            assert_eq!(l[1].text(), Some("3"));
            assert_eq!(v.get("n").unwrap().elements().count(), 1);
        }
    }
}
