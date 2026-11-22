//! Les vrais [`Outils`] des ressources, sur le Mac : `curl` du système pour les téléchargements
//! (reprise avec `--continue-at -`), le sha256 calculé sur un fil, l'uv embarqué dans l'app pour
//! le Python privé, et la CLI `claude` de l'utilisateur.
//!
//! Rien n'est écrit hors du dossier qu'on leur donne : uv et Python reçoivent un environnement
//! vidé où chaque cache, chaque dossier de données ou de configuration pointe dans le dossier du
//! Python privé (voir [`environnement_python`]).

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use sha2::{Digest, Sha256};

use greffier_core::adapters::{TaskProgress, Task, BoxedTask};
use greffier_core::resources::{ClaudeState, Tools, hex};

use crate::subprocess::{Interruption, Progression, wait, on_thread};

/// La version d'uv livrée avec l'app (`scripts/installer_app.py` la télécharge).
pub const UV_VERSION: &str = "0.13.7";

pub struct MacTools {
    uv: Option<PathBuf>,
}

impl MacTools {
    pub fn new() -> MacTools {
        MacTools { uv: bundled_uv() }
    }
    /// Avec cet uv plutôt que celui de l'app (outils de développement).
    pub fn with_uv(uv: PathBuf) -> MacTools {
        MacTools { uv: Some(uv) }
    }
}

impl Default for MacTools {
    fn default() -> MacTools {
        MacTools::new()
    }
}

/// L'uv livré avec l'app : à côté de l'exécutable dans `Greffier.app/Contents/MacOS/`, ou, pour
/// `cargo run`, dans `target/uv-<version>/` où `scripts/installer-app` le dépose.
pub fn bundled_uv() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let folder = exe.parent()?;
    [folder.join("uv"), folder.join(format!("../uv-{UV_VERSION}/uv"))].into_iter().find(|p| p.is_file())
}

/// L'environnement des processus du Python privé (uv et python) : rien n'hérite de celui de
/// l'utilisateur, et tout ce qu'ils pourraient écrire va dans `dossier`.
pub fn python_env(folder: &Path) -> Vec<(&'static str, String)> {
    let d = |s: &str| folder.join(s).to_string_lossy().into_owned();
    let home = std::env::var("HOME").unwrap_or_else(|_| "/".into());
    vec![
        ("HOME", home),
        ("PATH", "/usr/bin:/bin:/usr/sbin:/sbin".into()),
        ("LANG", "fr_FR.UTF-8".into()),
        ("TMPDIR", d("tmp")),
        ("UV_CACHE_DIR", d("cache")),
        ("UV_PYTHON_INSTALL_DIR", d("interpreteurs")),
        ("UV_PYTHON_BIN_DIR", d("bin")),
        ("UV_TOOL_DIR", d("outils")),
        ("UV_TOOL_BIN_DIR", d("bin")),
        ("UV_PROJECT_ENVIRONMENT", d("venv")),
        ("UV_NO_CONFIG", "1".into()),
        ("UV_PYTHON_PREFERENCE", "only-managed".into()),
        ("UV_PYTHON_DOWNLOADS", "automatic".into()),
        ("UV_NO_PROGRESS", "1".into()),
        ("UV_NO_MODIFY_PATH", "1".into()),
        ("XDG_CACHE_HOME", d("xdg/cache")),
        ("XDG_DATA_HOME", d("xdg/data")),
        ("XDG_CONFIG_HOME", d("xdg/config")),
        ("XDG_BIN_HOME", d("bin")),
        ("PYTHONNOUSERSITE", "1".into()),
        ("HF_HOME", d("xdg/huggingface")),
        ("HF_HUB_OFFLINE", "1".into()),
        ("HF_HUB_DISABLE_TELEMETRY", "1".into()),
        ("PYANNOTE_METRICS_ENABLED", "0".into()),
        ("TORCH_HOME", d("xdg/torch")),
        ("MPLCONFIGDIR", d("xdg/matplotlib")),
        ("PYTORCH_ENABLE_MPS_FALLBACK", "1".into()),
    ]
}

/// Le python de l'environnement privé.
pub fn python(folder: &Path) -> PathBuf {
    folder.join("venv/bin/python")
}

/// `curl` qui écrit dans le fichier partiel ; l'avancement est sa taille.
struct Curl {
    child: Child,
    partial: PathBuf,
    size: u64,
}

impl Task<()> for Curl {
    fn poll(&mut self, _elapsed: f64) -> TaskProgress<()> {
        match self.child.try_wait() {
            Ok(None) => TaskProgress::Running(std::fs::metadata(&self.partial).map_or(0, |m| m.len()) as f64 / self.size.max(1) as f64),
            Ok(Some(status)) if status.success() => TaskProgress::Done(()),
            Ok(Some(status)) => {
                let mut error = String::new();
                if let Some(mut e) = self.child.stderr.take() {
                    let _ = e.read_to_string(&mut error);
                }
                let error = error.lines().last().unwrap_or("").trim().to_string();
                TaskProgress::Failed(if error.is_empty() { format!("curl a échoué ({status})") } else { error })
            }
            Err(e) => TaskProgress::Failed(format!("curl : {e}")),
        }
    }
    fn stop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Curl {
    fn drop(&mut self) {
        if let Ok(None) = self.child.try_wait() {
            self.stop();
        }
    }
}

/// Lance `commande` jusqu'au bout (ou jusqu'à l'arrêt), sa sortie d'erreur dans `journal` ;
/// l'erreur rendue reprend les dernières lignes du journal.
fn launch(mut command: Command, log: &Path, what: &str, stop: &AtomicBool) -> Result<(), String> {
    let output = std::fs::File::create(log).map_err(|e| format!("{} : {e}", log.display()))?;
    let mut child = command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(output).spawn().map_err(|e| format!("{what} : {e}"))?;
    match wait(&mut child, stop, None, Duration::from_millis(100), || {}) {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => {
            let text = std::fs::read_to_string(log).unwrap_or_default();
            let end: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
            let end = end[end.len().saturating_sub(4)..].join("\n");
            Err(format!("{what} a échoué ({s}) : {end}"))
        }
        Err(Interruption::Error(e)) => Err(format!("{what} : {e}")),
        Err(Interruption::Stopped | Interruption::Timeout) => Err(format!("{what} : arrêté")),
    }
}

/// Lance `programme` avec `arguments` et rend sa sortie standard, en `delai` au plus.
fn output_of(program: &Path, arguments: &[&str], timeout: Duration) -> Result<(bool, String), String> {
    let mut child = Command::new(program)
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("{} : {e}", program.display()))?;
    let mut stdout = child.stdout.take().ok_or("pas de sortie")?;
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stdout.read_to_string(&mut text);
        text
    });
    let status = match wait(&mut child, &AtomicBool::new(false), Some(timeout), Duration::from_millis(50), || {}) {
        Ok(s) => s,
        Err(Interruption::Error(e)) => return Err(e.to_string()),
        Err(Interruption::Stopped | Interruption::Timeout) => return Err(format!("`{} {}` ne répond pas", program.display(), arguments.join(" "))),
    };
    Ok((status.success(), reader.join().unwrap_or_default()))
}

/// La CLI `claude` : une app lancée du Finder n'a pas le PATH du terminal, on regarde donc aux
/// endroits où ses installateurs la mettent avant le PATH.
pub(crate) fn find_claude() -> Option<PathBuf> {
    let home = greffier_core::settings::home();
    let known = [home.join(".local/bin/claude"), home.join(".claude/local/claude"), "/opt/homebrew/bin/claude".into(), "/usr/local/bin/claude".into()];
    let path = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).map(|d| d.join("claude")).collect::<Vec<_>>()).unwrap_or_default();
    known.into_iter().chain(path).find(|p| p.is_file())
}

/// La valeur d'un champ texte ou booléen d'un JSON plat, sans dépendance de plus.
fn field(json: &str, name: &str) -> Option<String> {
    let compact: String = json.split_whitespace().collect();
    let rest = &compact[compact.find(&format!("\"{name}\":"))? + name.len() + 3..];
    Some(match rest.strip_prefix('"') {
        Some(text) => text[..text.find('"')?].to_string(),
        None => rest[..rest.find([',', '}']).unwrap_or(rest.len())].to_string(),
    })
}

fn check_claude() -> Result<ClaudeState, String> {
    let Some(claude) = find_claude() else { return Ok(ClaudeState::Missing) };
    let timeout = Duration::from_secs(30);
    let (_, version) = output_of(&claude, &["--version"], timeout)?;
    let version = version.split_whitespace().next().unwrap_or("version inconnue").to_string();
    let (_, status) = output_of(&claude, &["auth", "status"], timeout)?;
    match field(&status, "loggedIn").as_deref() {
        Some("true") => Ok(ClaudeState::Connected { version, account: field(&status, "authMethod").unwrap_or_else(|| "compte inconnu".into()) }),
        Some(_) => Ok(ClaudeState::NotConnected { version }),
        None => Err(format!("`claude auth status` n'a pas dit si la CLI est connectée (version {version}) : mettre Claude Code à jour.")),
    }
}

impl Tools for MacTools {
    fn download(&mut self, url: &str, partial: &Path, size: u64) -> BoxedTask<()> {
        let child = Command::new("/usr/bin/curl")
            .args(["--fail", "--location", "--silent", "--show-error", "--retry", "3", "--retry-delay", "2", "--connect-timeout", "30"])
            .args(["--speed-limit", "1024", "--speed-time", "60", "--continue-at", "-", "--output"])
            .arg(partial)
            .arg(url)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn();
        match child {
            Ok(child) => Box::new(Curl { child, partial: partial.to_path_buf(), size }),
            Err(e) => greffier_core::fake::FakeTask::boxed(0.0, Err(format!("curl : {e}"))),
        }
    }

    fn fingerprint(&mut self, file: &Path) -> BoxedTask<String> {
        let file = file.to_path_buf();
        on_thread("sha256", Progression::Exact, move |stop, progress| {
            let mut f = std::fs::File::open(&file).map_err(|e| format!("{} : {e}", file.display()))?;
            let total = f.metadata().map_or(1, |m| m.len().max(1));
            let mut sha = Sha256::new();
            let mut buffer = vec![0u8; 8 << 20];
            let mut contents = 0u64;
            loop {
                if stop.load(Ordering::Relaxed) {
                    return Err("arrêté".into());
                }
                let n = f.read(&mut buffer).map_err(|e| format!("{} : {e}", file.display()))?;
                if n == 0 {
                    return Ok(hex(&sha.finalize()));
                }
                sha.update(&buffer[..n]);
                contents += n as u64;
                progress(contents as f64 / total as f64);
            }
        })
    }

    fn install_python(&mut self, project: &Path, folder: &Path) -> BoxedTask<()> {
        let (uv, project, folder) = (self.uv.clone(), project.to_path_buf(), folder.to_path_buf());
        on_thread("uv", Progression::Exact, move |stop, _| {
            let uv = uv.ok_or("uv embarqué introuvable : installer l'app avec scripts/installer-app.")?;
            let env = python_env(&folder);
            std::fs::create_dir_all(folder.join("tmp")).map_err(|e| e.to_string())?;
            let mut sync = Command::new(&uv);
            sync.args(["sync", "--frozen", "--project"]).arg(&project).current_dir(&project).env_clear().envs(env.iter().cloned());
            launch(sync, &folder.join("uv.log"), "uv sync", stop)?;
            let mut import = Command::new(python(&folder));
            import.args(["-c", "import pyannote.audio"]).current_dir(&folder).env_clear().envs(env.iter().cloned());
            launch(import, &folder.join("import.log"), "import pyannote.audio", stop)?;
            // Le cache d'uv ne sert qu'à l'installation : l'environnement a ses propres copies.
            let _ = std::fs::remove_dir_all(folder.join("cache"));
            let _ = std::fs::remove_dir_all(folder.join("tmp"));
            Ok(())
        })
    }

    fn check_claude(&mut self) -> BoxedTask<ClaudeState> {
        on_thread("claude", Progression::Exact, |_, _| check_claude())
    }
}

#[cfg(test)]
mod tests {
    use super::field;

    #[test]
    fn reads_claude_auth_status_fields() {
        let json = "{\n  \"loggedIn\": true,\n  \"authMethod\": \"claude.ai\",\n  \"apiProvider\": \"firstParty\"\n}";
        assert_eq!(field(json, "loggedIn").as_deref(), Some("true"));
        assert_eq!(field(json, "authMethod").as_deref(), Some("claude.ai"));
        assert_eq!(field("{\"loggedIn\": false}", "loggedIn").as_deref(), Some("false"));
        assert_eq!(field("Unknown command auth", "loggedIn"), None);
    }
}
