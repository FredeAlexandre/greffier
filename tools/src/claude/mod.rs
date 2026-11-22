//! L'agent de Greffier : la CLI `claude` installée sur le Mac (ADR 0002), modèle opus, en mode
//! non interactif (`claude -p … --output-format json --json-schema …`), avec des sessions
//! reprenables.
//!
//! - [`Claude`] : un appel à la CLI, qui rend la sortie structurée ; [`ClaudeDuMac`] la lance
//!   vraiment, les tests la remplacent.
//! - [`relecture`] : la relecture en deux passes, réécoute comprise.
//! - [`bilan`] : le bilan, que l'agent écrit lui-même.
//! - [`AgentClaude`] : l'adaptateur [`Agent`] branché dans l'app.

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

/// Le modèle demandé à la CLI.
pub const MODEL: &str = "opus";

/// Une nouvelle conversation, avec l'identifiant qu'on lui donne, ou la reprise d'une conversation.
#[derive(Clone, Debug, PartialEq)]
pub enum Session {
    New(String),
    Resume(String),
}

/// Un appel à la CLI `claude`.
#[derive(Clone, Debug, PartialEq)]
pub struct ClaudeCall {
    pub prompt: String,
    /// Donné sur l'entrée standard.
    pub input: String,
    /// Le schéma JSON de la réponse structurée.
    pub schema: String,
    pub session: Session,
    /// Le répertoire de travail de l'agent.
    pub directory: PathBuf,
    pub allowed_tools: Vec<String>,
    pub forbidden_tools: Vec<String>,
    /// L'agent écrit des fichiers sans demander (`--permission-mode acceptEdits`).
    pub can_write: bool,
    /// Des dossiers ouverts à l'agent en plus de son répertoire de travail (`--add-dir`).
    pub extra_folders: Vec<PathBuf>,
}

impl ClaudeCall {
    /// Les arguments de la CLI (le prompt compris).
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

/// Lance la CLI `claude` et rend sa réponse structurée (`structured_output`), ou un message
/// d'erreur lisible. `arret` l'interrompt.
pub trait Claude {
    fn ask(&self, call: &ClaudeCall, stop: &AtomicBool) -> Result<Value, String>;
}

/// Un identifiant de session neuf (UUID v4), comme `claude --session-id` l'attend.
pub fn new_session() -> String {
    let mut bytes = [0u8; 16];
    let contents = std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut bytes));
    if contents.is_err() {
        // Sans /dev/urandom : l'heure et l'aléa de la table de hachage de la bibliothèque standard.
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

/// Lit ce que la CLI a rendu (`--output-format json`) : la réponse structurée, sinon un message
/// qui dit pourquoi (erreur de Claude, fin de la sortie d'erreur…).
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

/// La vraie CLI `claude`, trouvée là où ses installateurs la mettent.
pub struct MacClaude {
    /// Au-delà, l'appel est abandonné.
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
        // Lancée du Finder, l'app n'a pas le PATH du terminal : on y met au moins le dossier de la CLI.
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

/// L'agent branché dans l'app : la relecture (avec la réécoute donnée) et le bilan par la CLI
/// `claude`.
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
