//! The summary by Claude: the prompt shipped with the app (`resources/prompt-summary.md`), filled
//! in for the meeting; the agent starts in the meetings folder, the context folders are open to
//! it for reading (`--add-dir`), it can read and write but can neither run commands nor go on
//! the web.
//! It writes the summary file itself and returns the title, brief, 3 actions at most and its
//! proposed names for the speakers.

use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::SystemTime;

use super::{ClaudeCall, Claude, Session};
use greffier_core::adapters::{SummaryRequest, SummarySpeaker, SummaryResult};
use greffier_core::playback_timestamp;

/// The summary prompt, shipped with the app.
pub const SUMMARY_PROMPT: &str = include_str!("../../resources/prompt-summary.md");

/// Read and write; no commands, no web, no subagents.
pub const ALLOWED_TOOLS: [&str; 5] = ["Read", "Glob", "Grep", "Write", "Edit"];
pub const FORBIDDEN_TOOLS: [&str; 4] = ["Bash", "WebFetch", "WebSearch", "Task"];

/// The structured answer asked of the agent.
pub const SCHEMA: &str = r#"{"type":"object","properties":{"title":{"type":"string","description":"Meeting title in a few words, in French"},"brief":{"type":"string","description":"3 to 5 line summary in French, with no title or list"},"actions":{"type":"array","items":{"type":"string"},"maxItems":3,"description":"The 3 most important actions, in French: Who — what — due date"},"participants":{"type":"array","items":{"type":"object","properties":{"speaker":{"type":"string"},"name":{"type":"string"},"justification":{"type":"string"},"same_voice_as":{"type":"string"}},"required":["speaker","name","justification","same_voice_as"],"additionalProperties":false}}},"required":["title","brief","actions","participants"],"additionalProperties":false}"#;

/// The prompt filled in for this meeting (paths relative to the meetings folder).
pub fn prompt(d: &SummaryRequest) -> String {
    let body = SUMMARY_PROMPT.strip_prefix("---\n").and_then(|r| r.split_once("\n---\n")).map_or(SUMMARY_PROMPT, |(_, body)| body).trim();
    let relative = |p: &Path| p.strip_prefix(&d.meetings_folder).unwrap_or(p).display().to_string();
    let folder = d.summary_file.parent().map(relative).unwrap_or_default();
    let or_none = |lines: Vec<String>| if lines.is_empty() { "(none)".to_string() } else { lines.join("\n") };
    let speakers = or_none(
        d.speakers
            .iter()
            .map(|l| match (&l.merged_into, &l.name, &l.proposal) {
                (Some(into), ..) => format!("- {}: same voice as {into} (merged by the user: their utterances are under {into} in the transcription)", l.label),
                (None, Some(name), _) => format!("- {}: named \"{name}\" by the user (certain)", l.label),
                (None, None, Some(p)) => format!("- {}: no name (proposed in the previous summary: \"{p}\")", l.label),
                (None, None, None) => format!("- {}: no name", l.label),
            })
            .collect(),
    );
    let markers = or_none(d.markers.iter().map(|r| format!("- ({}) {}", playback_timestamp(r.instant), r.label)).collect());
    let context = or_none(d.context_folders.iter().map(|c| format!("- `{}`", c.display())).collect());
    let current_summary = if d.current_summary.is_some() {
        "A summary already exists for this meeting: it is being redone, probably after corrections to the transcription or to the names given to the speakers. Replace it entirely, based on the current sheet and transcription."
    } else {
        ""
    };
    let start = d.start;
    let values = [
        ("FOLDER", folder),
        ("TRANSCRIPTION", relative(&d.transcription)),
        ("SHEET", relative(&d.sheet)),
        ("SUMMARY", relative(&d.summary_file)),
        ("DATE", format!("{:04}-{:02}-{:02}", start.year, start.month, start.day)),
        ("START", format!("{:02}:{:02}", start.hour, start.minute)),
        ("DURATION", playback_timestamp(d.duration)),
        ("SPEAKERS", speakers),
        ("MARKERS", markers),
        ("CONTEXT", context),
        ("CURRENT_SUMMARY", current_summary.to_string()),
    ];
    let mut text = body.to_string();
    for (key, value) in values {
        text = text.replace(&format!("{{{{{key}}}}}"), &value);
    }
    // An empty replacement (no current summary) leaves no gap of several lines.
    while text.contains("\n\n\n") {
        text = text.replace("\n\n\n", "\n\n");
    }
    text
}

/// The CLI call for this summary: a new session in the meetings folder, the context folders
/// opened as well, writing allowed except in the context folders (they are read only: the CLI's
/// `Write(//path/**)` and `Edit(//path/**)` rules, `//` = absolute path). A context folder that
/// contains the meetings folder stays open for writing, since the summary is written there.
pub fn call(d: &SummaryRequest, session: &str) -> ClaudeCall {
    let mut forbidden_tools: Vec<String> = FORBIDDEN_TOOLS.map(String::from).to_vec();
    for folder in d.context_folders.iter().filter(|c| !d.meetings_folder.starts_with(c)) {
        let pattern = format!("/{}/**", folder.display().to_string().trim_end_matches('/'));
        forbidden_tools.extend([format!("Write({pattern})"), format!("Edit({pattern})")]);
    }
    ClaudeCall {
        prompt: prompt(d),
        input: String::new(),
        schema: SCHEMA.into(),
        session: Session::New(session.into()),
        directory: d.meetings_folder.clone(),
        allowed_tools: ALLOWED_TOOLS.map(String::from).to_vec(),
        forbidden_tools,
        can_write: true,
        extra_folders: d.context_folders.clone(),
    }
}

/// Reads the agent's structured answer.
pub fn result(response: &serde_json::Value, session: &str) -> Result<SummaryResult, String> {
    let text = |v: &serde_json::Value, key: &str| v.get(key).and_then(serde_json::Value::as_str).map(str::trim).unwrap_or_default().to_string();
    let title = text(response, "title");
    if title.is_empty() {
        return Err("Claude n'a pas renvoyé le titre de la réunion.".into());
    }
    let list = |key: &str| response.get(key).and_then(serde_json::Value::as_array).cloned().unwrap_or_default();
    let actions = list("actions").iter().filter_map(|a| a.as_str()).map(|a| a.trim().trim_start_matches("- ").trim_start_matches("[ ]").trim().to_string()).filter(|a| !a.is_empty()).take(3).collect();
    let speakers = list("participants")
        .iter()
        .map(|p| SummarySpeaker { speaker: text(p, "speaker"), proposed_name: text(p, "name"), justification: text(p, "justification"), same_voice_as: Some(text(p, "same_voice_as")).filter(|m| !m.is_empty()) })
        .filter(|p| !p.speaker.is_empty() && (!p.proposed_name.is_empty() || !p.justification.is_empty()))
        .collect();
    Ok(SummaryResult { session: Some(session.to_string()), title, brief: text(response, "brief"), actions, speakers })
}

fn modified(file: &Path) -> Option<SystemTime> {
    std::fs::metadata(file).and_then(|m| m.modified()).ok()
}

/// Makes the summary: the agent writes the file, then returns its answer. Fails if the summary
/// file was not (re)written.
pub fn run(claude: &dyn Claude, d: &SummaryRequest, stop: &AtomicBool) -> Result<SummaryResult, String> {
    let session = super::new_session();
    let before = modified(&d.summary_file);
    let response = claude.ask(&call(d, &session), stop)?;
    let after = modified(&d.summary_file);
    if after.is_none() || after == before {
        return Err("Claude a terminé sans écrire le bilan.".into());
    }
    result(&response, &session)
}

#[cfg(test)]
mod tests {
    use super::*;
    use greffier_core::adapters::AgentSpeaker;
    use greffier_core::Marker;
    use greffier_core::DateTime;
    use std::path::PathBuf;

    fn request() -> SummaryRequest {
        let root = PathBuf::from("/Users/odile/Réunions");
        let name = "2026-11-21 14h02 - Réunion";
        let f = |role: &str| root.join(name).join(format!("{name} - {role}"));
        SummaryRequest {
            meetings_folder: root.clone(),
            context_folders: vec![PathBuf::from("/Users/odile/Notes")],
            summary_file: f("bilan.md"),
            sheet: f("réunion.md"),
            transcription: f("transcription.md"),
            title: "Réunion".into(),
            start: DateTime::new(2026, 11, 21, 14, 2),
            duration: 3723.0,
            utterances: vec![],
            speakers: vec![
                AgentSpeaker { label: "SPEAKER_00".into(), name: Some("Odile".into()), proposal: None, justification: String::new(), proposed_same_voice: None, merged_into: None },
                AgentSpeaker { label: "SPEAKER_01".into(), name: None, proposal: Some("Bastien".into()), justification: String::new(), proposed_same_voice: None, merged_into: None },
                AgentSpeaker { label: "SPEAKER_02".into(), name: None, proposal: None, justification: String::new(), proposed_same_voice: None, merged_into: Some("SPEAKER_00".into()) },
            ],
            markers: vec![Marker { instant: 312.0, label: "Repère 1".into() }],
            current_summary: None,
        }
    }

    #[test]
    fn prompt_is_filled_in_for_the_meeting() {
        let p = prompt(&request());
        assert!(!p.contains("{{"), "{p}");
        assert!(!p.starts_with("---"), "the file's front matter is not sent");
        assert!(p.contains("Meeting folder: `2026-11-21 14h02 - Réunion`"), "{p}");
        assert!(p.contains("- Transcription: `2026-11-21 14h02 - Réunion/2026-11-21 14h02 - Réunion - transcription.md`"), "{p}");
        assert!(p.contains("Create `2026-11-21 14h02 - Réunion/2026-11-21 14h02 - Réunion - bilan.md`"), "{p}");
        assert!(p.contains("- Date: 2026-11-21, start 14:02, duration 1:02:03"), "{p}");
        assert!(
            p.contains("- SPEAKER_00: named \"Odile\" by the user (certain)\n- SPEAKER_01: no name (proposed in the previous summary: \"Bastien\")\n- SPEAKER_02: same voice as SPEAKER_00 (merged by the user: their utterances are under SPEAKER_00 in the transcription)"),
            "{p}"
        );
        assert!(p.contains("`participants`: for each speaker in the list that is not merged"), "one proposal per speaker: {p}");
        assert!(p.contains("- (0:05:12) Repère 1"), "{p}");
        assert!(p.contains("- `/Users/odile/Notes`"), "{p}");
        assert!(!p.contains("A summary already exists"));
        assert!(!p.contains("\n\n\n"));

        let redone = prompt(&SummaryRequest { current_summary: Some("## En bref".into()), context_folders: vec![], markers: vec![], ..request() });
        assert!(redone.contains("A summary already exists for this meeting"));
        assert!(redone.contains("if a name, a project or an acronym is unclear:\n(none)"), "{redone}");
    }

    #[test]
    fn agent_reads_and_writes_without_commands_or_web_with_context_folders() {
        let a = call(&request(), "s-1").arguments();
        let after = |flag: &str| a.iter().position(|x| x == flag).map(|i| a[i + 1].as_str());
        assert_eq!(a[0], "-p");
        assert_eq!(after("--model"), Some("opus"));
        assert_eq!(after("--session-id"), Some("s-1"));
        assert_eq!(after("--output-format"), Some("json"));
        assert_eq!(after("--permission-mode"), Some("acceptEdits"));
        assert_eq!(after("--add-dir"), Some("/Users/odile/Notes"));
        let i = a.iter().position(|x| x == "--allowedTools").unwrap();
        assert_eq!(a[i + 1..i + 6], ["Read", "Glob", "Grep", "Write", "Edit"]);
        // The context folders are read, not written.
        let i = a.iter().position(|x| x == "--disallowedTools").unwrap();
        assert_eq!(a[i + 1..], ["Bash", "WebFetch", "WebSearch", "Task", "Write(//Users/odile/Notes/**)", "Edit(//Users/odile/Notes/**)"]);
        assert!(serde_json::from_str::<serde_json::Value>(SCHEMA).is_ok());
        assert_eq!(call(&request(), "s").directory, PathBuf::from("/Users/odile/Réunions"));

        let without_context = call(&SummaryRequest { context_folders: vec![], ..request() }, "s").arguments();
        assert!(!without_context.contains(&"--add-dir".to_string()));

        // A context folder that contains the meetings folder is not closed to writing: the summary
        // is written there.
        let parent = call(&SummaryRequest { context_folders: vec![PathBuf::from("/Users/odile")], ..request() }, "s");
        assert_eq!(parent.forbidden_tools, ["Bash", "WebFetch", "WebSearch", "Task"]);
    }

    /// A fake CLI: writes the summary file (or not), and returns the given answer.
    struct FakeCli {
        write: bool,
        response: serde_json::Value,
    }

    impl Claude for FakeCli {
        fn ask(&self, call: &ClaudeCall, _: &AtomicBool) -> Result<serde_json::Value, String> {
            if self.write {
                let file = call.prompt.split("Create `").nth(1).and_then(|r| r.split('`').next()).expect("the summary file in the prompt");
                std::fs::write(call.directory.join(file), "## En bref\n").map_err(|e| e.to_string())?;
            }
            Ok(self.response.clone())
        }
    }

    #[test]
    fn summary_fails_if_the_agent_does_not_write_the_file() {
        let root = std::env::temp_dir().join(format!("greffier-summary-claude-{}", std::process::id()));
        let name = "2026-11-21 14h02 - Réunion";
        std::fs::create_dir_all(root.join(name)).unwrap();
        let d = SummaryRequest { meetings_folder: root.clone(), summary_file: root.join(name).join(format!("{name} - bilan.md")), ..request() };
        let response = serde_json::json!({"title": "Budget", "brief": "R.", "actions": [], "participants": []});
        let stop = AtomicBool::new(false);

        let silent = FakeCli { write: false, response: response.clone() };
        assert_eq!(run(&silent, &d, &stop), Err("Claude a terminé sans écrire le bilan.".into()));

        let r = run(&FakeCli { write: true, response }, &d, &stop).unwrap();
        assert_eq!(r.title, "Budget");
        assert_eq!(r.session.as_deref().map(str::len), Some(36), "the session given to the CLI");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn response_gives_title_brief_actions_and_speakers() {
        let response = serde_json::json!({
            "title": " Budget du trimestre ",
            "brief": "On garde le budget.",
            "actions": ["- [ ] Odile — envoyer le devis — lundi", "Bastien — relire", "C", "D"],
            "participants": [
                {"speaker": "SPEAKER_01", "name": "Bastien", "justification": "Odile l'appelle Bastien (0:00:04).", "same_voice_as": ""},
                {"speaker": "SPEAKER_02", "name": "Bastien", "justification": "Même voix.", "same_voice_as": "SPEAKER_01"},
                {"speaker": "SPEAKER_03", "name": " ", "justification": "Personne ne le nomme.", "same_voice_as": ""},
                {"speaker": "", "name": "Odile", "justification": "Sans locuteur.", "same_voice_as": ""}
            ]
        });
        let r = result(&response, "s-1").unwrap();
        assert_eq!(r.session.as_deref(), Some("s-1"));
        assert_eq!(r.title, "Budget du trimestre");
        assert_eq!(r.brief, "On garde le budget.");
        assert_eq!(r.actions, ["Odile — envoyer le devis — lundi", "Bastien — relire", "C"]);
        assert_eq!(r.speakers[0].same_voice_as, None);
        assert_eq!(r.speakers[1].same_voice_as.as_deref(), Some("SPEAKER_01"));
        assert_eq!(r.speakers.len(), 3, "a speaker without a proposed name keeps its justification");
        assert_eq!((r.speakers[2].proposed_name.as_str(), r.speakers[2].justification.as_str()), ("", "Personne ne le nomme."));
        assert!(result(&serde_json::json!({"title": " ", "brief": "", "actions": [], "participants": []}), "s").is_err());
    }
}
