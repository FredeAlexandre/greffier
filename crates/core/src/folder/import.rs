//! Importing a past meeting (glossary: Import): a recording (video or audio only), with or
//! without its transcription, with or without its summary.
//!
//! [`Import`] says which files to take, with the date and title chosen in the import window;
//! `Greffier::propose_import` prepares it from the dropped files. The core then copies the
//! files into a new meeting folder: the originals are never touched. A media file of the MP4
//! family (`.mov`, `.m4a`, `.m4v`) becomes `… - enregistrement.mp4` there without re-encoding
//! ([`Mp4Muxer`]); if it cannot, or for other media (`.wav`, `.mp3`…), it keeps its
//! extension. A copied video that starts after a gap gets its first frame from 0
//! ([`mp4::fill_leading_video_gap`]), or it would not play.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::transcription::{self, StoredUtterance, StructuredTranscription};
use super::{MeetingsFolder, Role, mp4};
use crate::adapters::Mp4Muxer;
use crate::meeting::{Stage, MeetingId, Speaker, Meeting, Status};
use crate::time::DateTime;

/// What is imported, as confirmed in the import window.
#[derive(Clone, Debug, PartialEq)]
pub struct Import {
    /// The media: video, or audio only.
    pub recording: PathBuf,
    /// Greffier's structured file (`.json`), WebVTT (`.vtt`) or SRT (`.srt`).
    pub transcription: Option<PathBuf>,
    /// Any Markdown, copied as is.
    pub summary: Option<PathBuf>,
    pub start: DateTime,
    pub title: String,
}

/// A file's extension, in lowercase ("mp4").
pub(crate) fn extension(path: &Path) -> String {
    path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase()
}

/// The extensions of MP4-family media that AVFoundation turns into `.mp4` without re-encoding.
pub const MP4_FAMILY_EXTENSIONS: [&str; 4] = ["mov", "qt", "m4a", "m4v"];

/// The extensions of the media AVFoundation reads, video or audio only.
pub const MEDIA_EXTENSIONS: [&str; 17] = ["mp4", "m4v", "mov", "qt", "3gp", "3g2", "m4a", "m4b", "mp3", "wav", "aif", "aiff", "aifc", "caf", "aac", "flac", "amr"];

impl MeetingsFolder {
    /// Copies the import's files into a new meeting folder and returns the meeting, not yet
    /// written: the sheet and transcriptions come next with [`MeetingsFolder::write`].
    /// On error, nothing is left in the meetings folder.
    pub fn import(&self, import: &Import, id: MeetingId, mp4_muxing: &dyn Mp4Muxer) -> Result<Meeting, String> {
        let read = |p: &Path| std::fs::read_to_string(p).map_err(|e| format!("{} : {e}", p.display()));
        if !MEDIA_EXTENSIONS.contains(&extension(&import.recording).as_str()) {
            return Err(format!("{} : ce n'est pas un média que Greffier sait lire.", import.recording.display()));
        }
        if !import.recording.is_file() {
            return Err(format!("{} : fichier introuvable.", import.recording.display()));
        }
        let title = match import.title.trim() {
            "" => "Réunion",
            t => t,
        };
        let name = self.available_name(import.start, title);
        let folder = self.folder(&name);
        std::fs::create_dir_all(&folder).map_err(|e| format!("{} : {e}", folder.display()))?;
        let copy_to = |from: &Path, to: PathBuf| std::fs::copy(from, &to).map(|_| ()).map_err(|e| format!("{} : {e}", from.display()));
        let result = (|| {
            let mut r = Meeting::new(id, name.clone(), import.start, 0.0);
            r.title = title.to_string();
            r.status = Status::ToProcess;
            if let Some(t) = &import.transcription {
                apply_transcription(t, &mut r)?;
                r.completed_stage = Some(Stage::Transcription);
            }
            if let Some(b) = &import.summary {
                let md = read(b)?;
                copy_to(b, self.file(&name, Role::Summary))?;
                r.brief = crate::summary::brief(&md);
                r.set_summary(md);
                // With its transcription, the meeting is complete; without it, processing redoes everything.
                if r.completed_stage.is_some() {
                    r.completed_stage = Some(Stage::Summary);
                    r.status = Status::Done;
                }
            }
            self.copy_recording(&import.recording, &name, mp4_muxing)?;
            if let Some(f) = self.recording(&name) {
                // A video that starts after a gap would not play. Should the repair fail, the
                // meeting is still worth importing: the player tries again (`recording_to_play`).
                let _ = mp4::fill_leading_video_gap(&f);
            }
            r.duration = self.recording(&name).and_then(|f| mp4::duration(&f)).or_else(|| r.utterances.iter().map(|x| x.end).reduce(f64::max)).unwrap_or(0.0);
            Ok(r)
        })();
        if result.is_err() {
            let _ = std::fs::remove_dir_all(&folder);
        }
        result
    }
}

impl MeetingsFolder {
    /// Copies the media as `… - enregistrement.mp4`: as is for an `.mp4`, muxed to MP4 for the
    /// MP4 family; otherwise (or if muxing to MP4 fails), copied with its extension.
    fn copy_recording(&self, media: &Path, name: &str, mp4_muxing: &dyn Mp4Muxer) -> Result<(), String> {
        let copy_to = |to: PathBuf| std::fs::copy(media, &to).map(|_| ()).map_err(|e| format!("{} : {e}", media.display()));
        let ext = extension(media);
        let mp4 = self.file(name, Role::Recording);
        if ext == "mp4" {
            return copy_to(mp4);
        }
        if MP4_FAMILY_EXTENSIONS.contains(&ext.as_str()) {
            match mp4_muxing.to_mp4(media, &mp4) {
                Ok(()) if mp4.is_file() => return Ok(()),
                _ => {
                    let _ = std::fs::remove_file(&mp4);
                }
            }
        }
        copy_to(self.recording_file(name, &ext))
    }
}

// --- Transcriptions ------------------------------------------------------------------------

/// An utterance read from a subtitle file.
#[derive(Debug)]
struct Line {
    start: f64,
    end: f64,
    speaker: Option<String>,
    text: String,
}

/// Reads the imported transcription into the meeting: Greffier's structured file, WebVTT or SRT.
/// It is treated as not reviewed. Named speakers ("Alexandre") become SPEAKER_00, SPEAKER_01…
/// in the order they speak, with their name; without speakers, everything goes to SPEAKER_00.
fn apply_transcription(path: &Path, r: &mut Meeting) -> Result<(), String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{} : {e}", path.display()))?;
    let (transcription, names) = match extension(path).as_str() {
        "json" => {
            let mut t = StructuredTranscription::read(&text).map_err(|e| format!("{} : {e}", path.display()))?;
            t.review = None;
            t.raw.clear();
            (t, vec![])
        }
        "vtt" | "srt" => from_lines(read_subtitles(&text)),
        _ => return Err(format!("{} : la transcription doit être un fichier de Greffier (.json), WebVTT (.vtt) ou SRT (.srt).", path.display())),
    };
    if transcription.utterances.is_empty() {
        return Err(format!("{} : aucune réplique lue.", path.display()));
    }
    transcription.apply(r);
    for l in &mut r.speakers {
        l.name = names.iter().find(|(e, _)| *e == l.label).map(|(_, n)| n.clone());
    }
    Ok(())
}

/// The utterances as a structured transcription, and the name of each label given to a named speaker.
fn from_lines(lines: Vec<Line>) -> (StructuredTranscription, Vec<(String, String)>) {
    let is_label = |s: &str| s.strip_prefix("SPEAKER_").is_some_and(|n| !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit()));
    let mut taken: Vec<String> = lines.iter().filter_map(|l| l.speaker.clone()).filter(|l| is_label(l)).collect();
    let mut names: Vec<(String, String)> = Vec::new();
    let mut n = 0;
    for name in lines.iter().filter_map(|l| l.speaker.as_deref()).filter(|l| !is_label(l)) {
        if names.iter().any(|(_, x)| x == name) {
            continue;
        }
        let label = loop {
            let e = Speaker::numbered_label(n);
            n += 1;
            if !taken.contains(&e) {
                break e;
            }
        };
        taken.push(label.clone());
        names.push((label, name.to_string()));
    }
    let label = |l: &Option<String>| match l {
        Some(x) if is_label(x) => x.clone(),
        Some(x) => names.iter().find(|(_, n)| n == x).map(|(e, _)| e.clone()).unwrap_or_default(),
        None => "SPEAKER_00".into(),
    };
    let utterances: Vec<StoredUtterance> = lines.iter().map(|l| StoredUtterance { speaker: label(&l.speaker), start: l.start, end: l.end, text: l.text.clone() }).collect();
    let mut speakers: Vec<String> = utterances.iter().map(|x| x.speaker.clone()).collect();
    speakers.sort();
    speakers.dedup();
    let t = StructuredTranscription {
        version: transcription::VERSION,
        speakers,
        names: Default::default(),
        merges: Default::default(),
        names_before_merge: Default::default(),
        lent_names: Default::default(),
        words: vec![],
        turns: vec![],
        utterances,
        raw: vec![],
        review: None,
        passages: vec![],
        manual_corrections: vec![],
    };
    (t, names)
}

/// The utterances of a WebVTT or SRT file. The speaker is read at the start of the utterance:
/// `<v Name>`, `[Name]:` or `Name :`. The last form is only kept if at least half the utterances
/// carry it (otherwise "Bref : on y va" would be taken for a speaker). An utterance without a
/// speaker in a file that has speakers keeps the previous one's.
fn read_subtitles(text: &str) -> Vec<Line> {
    let text = text.trim_start_matches('\u{feff}').replace("\r\n", "\n").replace('\r', "\n");
    let mut lines = Vec::new();
    // For each utterance without a sure speaker: (name, rest) if it starts with "Name :".
    let mut candidates: Vec<Option<(String, String)>> = Vec::new();
    let mut it = text.lines().peekable();
    while let Some(l) = it.next() {
        let Some((a, b)) = l.split_once("-->") else { continue };
        let (Some(start), Some(end)) = (timestamp(a.trim()), b.split_whitespace().next().and_then(timestamp)) else { continue };
        let mut body = Vec::new();
        while let Some(l) = it.next_if(|l| !l.trim().is_empty()) {
            body.push(l.trim());
        }
        let body = body.join(" ");
        let voice = body.strip_prefix("<v").filter(|r| r.starts_with([' ', '.'])).and_then(|r| r.split_once('>')).and_then(|(tag, _)| tag.split_once(' ')).map(|(_, name)| name.trim().to_string()).filter(|n| !n.is_empty());
        let text = without_tags(&body);
        let (speaker, text) = match voice {
            Some(v) => (Some(v), text),
            None => match bracketed(&text) {
                Some((name, rest)) => (Some(name), rest),
                None => {
                    candidates.push(name_and_colon(&text));
                    (None, text)
                }
            },
        };
        lines.push(Line { start, end, speaker, text });
    }
    let without = lines.iter().filter(|l| l.speaker.is_none()).count();
    if without > 0 && candidates.iter().filter(|c| c.is_some()).count() * 2 >= without {
        let mut c = candidates.into_iter();
        for l in lines.iter_mut().filter(|l| l.speaker.is_none()) {
            if let Some(Some((name, rest))) = c.next() {
                l.speaker = Some(name);
                l.text = rest;
            }
        }
    }
    let mut previous = None;
    for l in &mut lines {
        match &l.speaker {
            Some(x) => previous = Some(x.clone()),
            None => l.speaker = previous.clone(),
        }
    }
    lines.retain(|l| !l.text.is_empty());
    lines
}

/// `hh:mm:ss.mmm`, `mm:ss.mmm` (WebVTT) or `hh:mm:ss,mmm` (SRT), in seconds.
fn timestamp(s: &str) -> Option<f64> {
    let s = s.replace(',', ".");
    let parts: Vec<&str> = s.split(':').collect();
    let number = |x: &str| -> Option<f64> { x.bytes().all(|c| c.is_ascii_digit() || c == b'.').then(|| x.parse().ok()).flatten() };
    match parts[..] {
        [m, s] => Some(number(m)? * 60.0 + number(s)?),
        [h, m, s] => Some(number(h)? * 3600.0 + number(m)? * 60.0 + number(s)?),
        _ => None,
    }
}

/// The text without tags (`<i>`, `<v Name>`, `<00:00:01.000>`…) or entities, whitespace collapsed.
fn without_tags(s: &str) -> String {
    let mut out = String::new();
    let mut into = false;
    for c in s.chars() {
        match c {
            '<' => into = true,
            '>' if into => into = false,
            c if !into => out.push(c),
            _ => {}
        }
    }
    let out = out.replace("&lt;", "<").replace("&gt;", ">").replace("&nbsp;", " ").replace("&lrm;", "").replace("&rlm;", "").replace("&amp;", "&");
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `[Name]: text` or `[Name] : text`.
fn bracketed(s: &str) -> Option<(String, String)> {
    let (name, rest) = s.strip_prefix('[')?.split_once(']')?;
    let rest = rest.trim_start().strip_prefix(':')?;
    let name = name.trim();
    (!name.is_empty()).then(|| (name.to_string(), rest.trim().to_string()))
}

/// `Name : text` or `Name: text`: a short name (four words at most) starting with a capital
/// letter, followed by a colon then a space ("10:30" is not one).
fn name_and_colon(s: &str) -> Option<(String, String)> {
    let (name, rest) = s.split_once(':')?;
    if !rest.is_empty() && !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let name = name.trim();
    let allowed = |c: char| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.' | '\'' | '’');
    let short = name.chars().count() <= 40 && name.split_whitespace().count() <= 4;
    (name.starts_with(char::is_uppercase) && short && name.chars().all(allowed)).then(|| (name.to_string(), rest.trim().to_string()))
}

// --- The import window's proposal ----------------------------------------------------------

/// What the import window proposes for these dropped files: the media, the transcription and
/// the summary (at most one of each, sorted by extension), the date (the creation date stored
/// in the media, otherwise the file's modification date) and the title (the summary's,
/// otherwise the media's name).
pub(crate) fn propose(paths: &[PathBuf], to_local_time: impl Fn(SystemTime) -> DateTime) -> Result<Import, String> {
    let (mut media, mut transcriptions, mut summaries) = (vec![], vec![], vec![]);
    for c in paths {
        let e = extension(c);
        match e.as_str() {
            e if MEDIA_EXTENSIONS.contains(&e) => media.push(c.clone()),
            "json" | "vtt" | "srt" => transcriptions.push(c.clone()),
            "md" | "markdown" => summaries.push(c.clone()),
            _ => return Err(format!("« {} » : Greffier importe un média (vidéo ou audio), une transcription (.vtt, .srt, .json) et un bilan (.md).", file_name(c))),
        }
    }
    let single = |v: Vec<PathBuf>, what: &str| match v.len() {
        0 | 1 => Ok(v.into_iter().next()),
        _ => Err(format!("Un seul fichier de {what} par réunion.")),
    };
    let recording = single(media, "média")?.ok_or("Il faut un média (vidéo ou audio) pour importer une réunion.")?;
    let transcription = single(transcriptions, "transcription")?;
    let summary = single(summaries, "bilan")?;
    let instant = mp4::creation_date(&recording).or_else(|| std::fs::metadata(&recording).and_then(|m| m.modified()).ok());
    let start = instant.map(to_local_time).ok_or_else(|| format!("{} : fichier introuvable.", recording.display()))?;
    let title = summary.as_ref().and_then(|b| std::fs::read_to_string(b).ok()).and_then(|md| summary_title(&md));
    let title = title.unwrap_or_else(|| recording.file_stem().map(|s| s.to_string_lossy().trim().to_string()).unwrap_or_default());
    Ok(Import { recording, transcription, summary, start, title })
}

fn file_name(c: &Path) -> String {
    c.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// A summary's title: `titre:` or `title:` from its header, otherwise its first `# Title`,
/// without a leading "Bilan —".
fn summary_title(md: &str) -> Option<String> {
    let md = md.trim_start_matches('\u{feff}');
    let mut body = md;
    if let Some(rest) = md.strip_prefix("---\n").or_else(|| md.strip_prefix("---\r\n")) {
        let (header, after) = rest.split_once("\n---").unwrap_or((rest, ""));
        body = after;
        for l in header.lines() {
            if let Some(v) = l.strip_prefix("titre:").or_else(|| l.strip_prefix("title:")) {
                let v = v.trim().trim_matches(['"', '\'']).trim();
                if !v.is_empty() {
                    return Some(v.to_string());
                }
            }
        }
    }
    let h1 = body.lines().find_map(|l| l.strip_prefix("# "))?.trim();
    let title = match h1.strip_prefix("Bilan") {
        Some(r) if r.starts_with([' ', ':']) => r.trim_start_matches([' ', '—', '–', '-', ':']).trim(),
        _ => h1,
    };
    (!title.is_empty()).then(|| title.to_string())
}
