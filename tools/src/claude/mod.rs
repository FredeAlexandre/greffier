//! Greffier's agent: the `claude` CLI installed on the Mac (ADR 0002), opus model, in
//! non-interactive mode (`claude -p … --output-format json --json-schema …`), with resumable
//! sessions.
//!
//! - [`Claude`]: a call to the CLI, which returns the structured output; [`MacClaude`] actually
//!   runs it, the tests replace it.
//! - [`review`]: the two-pass review, relisten included.
//! - [`summary`]: the summary, which the agent writes itself.
//! - [`AgentClaude`]: the [`Agent`] adapter plugged into the app.

pub mod summary;
pub mod review;

use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use serde_json::Value;

use crate::subprocess::{Interruption, Progression, wait, on_thread};

use greffier_core::adapters::{Agent, SummaryRequest, ReviewRequest, ReviewResult, SummaryResult, BoxedTask};

/// The model requested from the CLI.
pub const MODEL: &str = "opus";

/// A new conversation, with the identifier we give it, or the resumption of a conversation.
#[derive(Clone, Debug, PartialEq)]
pub enum Session {
    New(String),
    Resume(String),
}

/// A call to the `claude` CLI.
#[derive(Clone, Debug, PartialEq)]
pub struct ClaudeCall {
    pub prompt: String,
    /// Given on standard input.
    pub input: String,
    /// The JSON schema of the structured answer.
    pub schema: String,
    pub session: Session,
    /// The agent's working directory.
    pub directory: PathBuf,
    pub allowed_tools: Vec<String>,
    pub forbidden_tools: Vec<String>,
    /// The agent writes files without asking (`--permission-mode acceptEdits`).
    pub can_write: bool,
    /// Folders open to the agent in addition to its working directory (`--add-dir`).
    pub extra_folders: Vec<PathBuf>,
}

impl ClaudeCall {
    /// The CLI arguments (prompt included).
    pub fn arguments(&self) -> Vec<String> {
        let mut a: Vec<String> = vec!["-p".into(), self.prompt.clone(), "--model".into(), MODEL.into()];
        match &self.session {
            Session::New(id) => a.extend(["--session-id".into(), id.clone()]),
            Session::Resume(id) => a.extend(["--resume".into(), id.clone()]),
        }
        a.extend(["--output-format".into(), "json".into(), "--json-schema".into(), self.schema.clone()]);
        if self.can_write {
            a.extend(["--permission-mode".into(), "acceptEdits".into()]);
        }
        if !self.extra_folders.is_empty() {
            a.push("--add-dir".into());
            a.extend(self.extra_folders.iter().map(|d| d.display().to_string()));
        }
        a.push("--allowedTools".into());
        a.extend(self.allowed_tools.iter().cloned());
        a.push("--disallowedTools".into());
        a.extend(self.forbidden_tools.iter().cloned());
        a
    }
}

/// Runs the `claude` CLI and returns its structured answer (`structured_output`), or a readable
/// error message. `stop` interrupts it.
pub trait Claude {
    fn ask(&self, call: &ClaudeCall, stop: &AtomicBool) -> Result<Value, String>;
}

/// A fresh session identifier (UUID v4), as `claude --session-id` expects.
pub fn new_session() -> String {
    let mut bytes = [0u8; 16];
    let contents = std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut bytes));
    if contents.is_err() {
        // Without /dev/urandom: the time and the standard library's hash map randomness.
        use std::hash::{BuildHasher, Hasher};
        for (i, piece) in bytes.chunks_mut(8).enumerate() {
            let mut h = std::collections::hash_map::RandomState::new().build_hasher();
            h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos()) + i as u128);
            piece.copy_from_slice(&h.finish().to_le_bytes());
        }
    }
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let h: String = bytes.iter().map(|o| format!("{o:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

fn truncate(s: &str, n: usize) -> String {
    let s = s.trim();
    match s.char_indices().nth(n) {
        Some((i, _)) => format!("{}…", &s[..i]),
        None => s.to_string(),
    }
}

/// Reads what the CLI returned (`--output-format json`): the structured answer, otherwise a
/// message saying why (Claude error, end of the error output…).
pub fn read_response(output: &str, errors: &str, succeeded: bool) -> Result<Value, String> {
    let errors_tail = || {
        let lines: Vec<&str> = errors.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
        lines[lines.len().saturating_sub(2)..].join(" / ")
    };
    let json: Option<Value> = serde_json::from_str(output.trim()).ok().or_else(|| output.lines().rev().find_map(|l| serde_json::from_str(l.trim()).ok()));
    let Some(json) = json.filter(Value::is_object) else {
        let end = errors_tail();
        return Err(if end.is_empty() { "Claude s'est arrêté sans rien dire.".into() } else { format!("Claude : {}", truncate(&end, 300)) });
    };
    let text = |key: &str| json.get(key).and_then(|v| v.as_str().map(str::to_string).or_else(|| (!v.is_null()).then(|| v.to_string()))).filter(|s| !s.trim().is_empty());
    if !succeeded || json.get("is_error").and_then(Value::as_bool) == Some(true) {
        let cause = text("result").or_else(|| text("api_error_status")).unwrap_or_else(errors_tail);
        return Err(if cause.is_empty() { "Claude a échoué sans dire pourquoi.".into() } else { format!("Claude : {}", truncate(&cause, 300)) });
    }
    match json.get("structured_output") {
        Some(v) if !v.is_null() => Ok(v.clone()),
        _ => Err(format!("Claude n'a pas rendu de réponse structurée : {}", truncate(&text("result").unwrap_or_default(), 300))),
    }
}

/// The real `claude` CLI, found where its installers put it.
pub struct MacClaude {
    /// Beyond this, the call is abandoned.
    pub timeout: Duration,
}

impl Default for MacClaude {
    fn default() -> MacClaude {
        MacClaude { timeout: Duration::from_secs(40 * 60) }
    }
}

impl Claude for MacClaude {
    fn ask(&self, call: &ClaudeCall, stop: &AtomicBool) -> Result<Value, String> {
        let Some(claude) = crate::mac::find_claude() else {
            return Err("La CLI `claude` est introuvable : installer Claude Code (https://claude.com/claude-code), s'y connecter (`claude` dans un terminal), puis réessayer cette étape.".into());
        };
        // Launched from the Finder, the app does not have the terminal's PATH: add at least the CLI's folder.
        let mut path: Vec<PathBuf> = claude.parent().map(|d| vec![d.to_path_buf()]).unwrap_or_default();
        path.extend(std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect::<Vec<_>>()).unwrap_or_default());
        path.extend(["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin", "/usr/sbin", "/sbin"].map(PathBuf::from));
        let path = std::env::join_paths(path).unwrap_or_default();
        let mut child = Command::new(&claude)
            .args(call.arguments())
            .current_dir(&call.directory)
            .env("PATH", path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Impossible de lancer `{}` : {e}", claude.display()))?;
        let read = |stream: Option<Box<dyn Read + Send>>| {
            std::thread::spawn(move || {
                let mut text = String::new();
                if let Some(mut f) = stream {
                    let _ = f.read_to_string(&mut text);
                }
                text
            })
        };
        let output = read(child.stdout.take().map(|s| Box::new(s) as Box<dyn Read + Send>));
        let errors = read(child.stderr.take().map(|s| Box::new(s) as Box<dyn Read + Send>));
        if let Some(mut input) = child.stdin.take() {
            let text = call.input.clone();
            std::thread::spawn(move || {
                let _ = input.write_all(text.as_bytes());
            });
        }
        let status = match wait(&mut child, stop, Some(self.timeout), Duration::from_millis(200), || {}) {
            Ok(s) => s,
            Err(Interruption::Stopped) => return Err("arrêté".into()),
            Err(Interruption::Timeout) => return Err(format!("Claude n'a pas répondu en {} min.", self.timeout.as_secs() / 60)),
            Err(Interruption::Error(e)) => return Err(format!("claude : {e}")),
        };
        read_response(&output.join().unwrap_or_default(), &errors.join().unwrap_or_default(), status.success())
    }
}

/// The agent plugged into the app: the review (with the given relisten) and the summary by the
/// `claude` CLI.
pub struct AgentClaude {
    claude: Arc<dyn Claude + Send + Sync>,
    relisten: Arc<dyn review::Relisten + Send + Sync>,
}

impl AgentClaude {
    pub fn new(claude: Arc<dyn Claude + Send + Sync>, relisten: Arc<dyn review::Relisten + Send + Sync>) -> AgentClaude {
        AgentClaude { claude, relisten }
    }
}

impl Agent for AgentClaude {
    fn review(&mut self, request: ReviewRequest) -> BoxedTask<ReviewResult> {
        let (claude, relisten) = (self.claude.clone(), self.relisten.clone());
        on_thread("claude", Progression::Milestones(&review::MILESTONES), move |stop, progress| review::review(claude.as_ref(), relisten.as_ref(), &request, stop, progress))
    }

    fn summarize(&mut self, request: SummaryRequest) -> BoxedTask<SummaryResult> {
        let claude = self.claude.clone();
        on_thread("claude", Progression::Milestones(&[]), move |stop, _| summary::run(claude.as_ref(), &request, stop))
    }
}
