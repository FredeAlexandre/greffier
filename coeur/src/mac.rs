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
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

use crate::adaptateurs::{Avancement, Tache, TacheDe};
use crate::ressources::{EtatDeClaude, Outils, hex};

/// La version d'uv livrée avec l'app (`scripts/installer_app.py` la télécharge).
pub const VERSION_D_UV: &str = "0.13.7";

pub struct OutilsDuMac {
    uv: Option<PathBuf>,
}

impl OutilsDuMac {
    pub fn new() -> OutilsDuMac {
        OutilsDuMac { uv: uv_embarque() }
    }
    /// Avec cet uv plutôt que celui de l'app (outils de développement).
    pub fn avec_uv(uv: PathBuf) -> OutilsDuMac {
        OutilsDuMac { uv: Some(uv) }
    }
}

impl Default for OutilsDuMac {
    fn default() -> OutilsDuMac {
        OutilsDuMac::new()
    }
}

/// L'uv livré avec l'app : à côté de l'exécutable dans `Greffier.app/Contents/MacOS/`, ou, pour
/// `cargo run`, dans `target/uv-<version>/` où `scripts/installer-app` le dépose.
pub fn uv_embarque() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dossier = exe.parent()?;
    [dossier.join("uv"), dossier.join(format!("../uv-{VERSION_D_UV}/uv"))].into_iter().find(|p| p.is_file())
}

/// L'environnement des processus du Python privé (uv et python) : rien n'hérite de celui de
/// l'utilisateur, et tout ce qu'ils pourraient écrire va dans `dossier`.
pub fn environnement_python(dossier: &Path) -> Vec<(&'static str, String)> {
    let d = |s: &str| dossier.join(s).to_string_lossy().into_owned();
    let maison = std::env::var("HOME").unwrap_or_else(|_| "/".into());
    vec![
        ("HOME", maison),
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
pub fn python(dossier: &Path) -> PathBuf {
    dossier.join("venv/bin/python")
}

// --- Tâches ----------------------------------------------------------------------------------

/// Un travail sur un fil ; `sonder` lit son avancement (0 à 1, en bits d'un f64).
struct SurUnFil<T> {
    resultat: mpsc::Receiver<Result<T, String>>,
    progres: Arc<AtomicU64>,
    arret: Arc<AtomicBool>,
}

pub(crate) fn sur_un_fil<T: Send + 'static>(travail: impl FnOnce(&AtomicU64, &AtomicBool) -> Result<T, String> + Send + 'static) -> TacheDe<T> {
    let (envoi, resultat) = mpsc::channel();
    let progres = Arc::new(AtomicU64::new(0f64.to_bits()));
    let arret = Arc::new(AtomicBool::new(false));
    let (p, a) = (progres.clone(), arret.clone());
    std::thread::spawn(move || {
        let _ = envoi.send(travail(&p, &a));
    });
    Box::new(SurUnFil { resultat, progres, arret })
}

impl<T> Tache<T> for SurUnFil<T> {
    fn sonder(&mut self, _ecoule: f64) -> Avancement<T> {
        match self.resultat.try_recv() {
            Ok(Ok(v)) => Avancement::Fini(v),
            Ok(Err(e)) => Avancement::Echec(e),
            Err(mpsc::TryRecvError::Empty) => Avancement::EnCours(f64::from_bits(self.progres.load(Ordering::Relaxed))),
            Err(mpsc::TryRecvError::Disconnected) => Avancement::Echec("le travail s'est arrêté sans résultat".into()),
        }
    }
    fn arreter(&mut self) {
        self.arret.store(true, Ordering::Relaxed);
    }
}

impl<T> Drop for SurUnFil<T> {
    fn drop(&mut self) {
        self.arret.store(true, Ordering::Relaxed);
    }
}

/// `curl` qui écrit dans le fichier partiel ; l'avancement est sa taille.
struct Curl {
    enfant: Child,
    partiel: PathBuf,
    taille: u64,
}

impl Tache<()> for Curl {
    fn sonder(&mut self, _ecoule: f64) -> Avancement<()> {
        match self.enfant.try_wait() {
            Ok(None) => Avancement::EnCours(std::fs::metadata(&self.partiel).map_or(0, |m| m.len()) as f64 / self.taille.max(1) as f64),
            Ok(Some(statut)) if statut.success() => Avancement::Fini(()),
            Ok(Some(statut)) => {
                let mut erreur = String::new();
                if let Some(mut e) = self.enfant.stderr.take() {
                    let _ = e.read_to_string(&mut erreur);
                }
                let erreur = erreur.lines().last().unwrap_or("").trim().to_string();
                Avancement::Echec(if erreur.is_empty() { format!("curl a échoué ({statut})") } else { erreur })
            }
            Err(e) => Avancement::Echec(format!("curl : {e}")),
        }
    }
    fn arreter(&mut self) {
        let _ = self.enfant.kill();
        let _ = self.enfant.wait();
    }
}

impl Drop for Curl {
    fn drop(&mut self) {
        if let Ok(None) = self.enfant.try_wait() {
            self.arreter();
        }
    }
}

/// Lance `commande` jusqu'au bout (ou jusqu'à l'arrêt), sa sortie d'erreur dans `journal` ;
/// l'erreur rendue reprend les dernières lignes du journal.
fn lancer(mut commande: Command, journal: &Path, quoi: &str, arret: &AtomicBool) -> Result<(), String> {
    let sortie = std::fs::File::create(journal).map_err(|e| format!("{} : {e}", journal.display()))?;
    let mut enfant = commande.stdin(Stdio::null()).stdout(Stdio::null()).stderr(sortie).spawn().map_err(|e| format!("{quoi} : {e}"))?;
    loop {
        if arret.load(Ordering::Relaxed) {
            let _ = enfant.kill();
            let _ = enfant.wait();
            return Err(format!("{quoi} : arrêté"));
        }
        match enfant.try_wait() {
            Ok(Some(s)) if s.success() => return Ok(()),
            Ok(Some(s)) => {
                let texte = std::fs::read_to_string(journal).unwrap_or_default();
                let fin: Vec<&str> = texte.lines().filter(|l| !l.trim().is_empty()).collect();
                let fin = fin[fin.len().saturating_sub(4)..].join("\n");
                return Err(format!("{quoi} a échoué ({s}) : {fin}"));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            Err(e) => return Err(format!("{quoi} : {e}")),
        }
    }
}

/// Lance `programme` avec `arguments` et rend sa sortie standard, en `delai` au plus.
fn sortie_de(programme: &Path, arguments: &[&str], delai: Duration) -> Result<(bool, String), String> {
    let mut enfant = Command::new(programme)
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("{} : {e}", programme.display()))?;
    let mut stdout = enfant.stdout.take().ok_or("pas de sortie")?;
    let lecteur = std::thread::spawn(move || {
        let mut texte = String::new();
        let _ = stdout.read_to_string(&mut texte);
        texte
    });
    let debut = Instant::now();
    let statut = loop {
        if let Some(s) = enfant.try_wait().map_err(|e| e.to_string())? {
            break s;
        }
        if debut.elapsed() > delai {
            let _ = enfant.kill();
            let _ = enfant.wait();
            return Err(format!("`{} {}` ne répond pas", programme.display(), arguments.join(" ")));
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    Ok((statut.success(), lecteur.join().unwrap_or_default()))
}

/// La CLI `claude` : une app lancée du Finder n'a pas le PATH du terminal, on regarde donc aux
/// endroits où ses installateurs la mettent avant le PATH.
pub(crate) fn trouver_claude() -> Option<PathBuf> {
    let maison = PathBuf::from(std::env::var_os("HOME")?);
    let connus = [maison.join(".local/bin/claude"), maison.join(".claude/local/claude"), "/opt/homebrew/bin/claude".into(), "/usr/local/bin/claude".into()];
    let chemin = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).map(|d| d.join("claude")).collect::<Vec<_>>()).unwrap_or_default();
    connus.into_iter().chain(chemin).find(|p| p.is_file())
}

/// La valeur d'un champ texte ou booléen d'un JSON plat, sans dépendance de plus.
fn champ(json: &str, nom: &str) -> Option<String> {
    let compact: String = json.split_whitespace().collect();
    let reste = &compact[compact.find(&format!("\"{nom}\":"))? + nom.len() + 3..];
    Some(match reste.strip_prefix('"') {
        Some(texte) => texte[..texte.find('"')?].to_string(),
        None => reste[..reste.find([',', '}']).unwrap_or(reste.len())].to_string(),
    })
}

fn verifier_claude() -> Result<EtatDeClaude, String> {
    let Some(claude) = trouver_claude() else { return Ok(EtatDeClaude::Absente) };
    let delai = Duration::from_secs(30);
    let (_, version) = sortie_de(&claude, &["--version"], delai)?;
    let version = version.split_whitespace().next().unwrap_or("version inconnue").to_string();
    let (_, statut) = sortie_de(&claude, &["auth", "status"], delai)?;
    match champ(&statut, "loggedIn").as_deref() {
        Some("true") => Ok(EtatDeClaude::Connectee { version, compte: champ(&statut, "authMethod").unwrap_or_else(|| "compte inconnu".into()) }),
        Some(_) => Ok(EtatDeClaude::NonConnectee { version }),
        None => Err(format!("`claude auth status` n'a pas dit si la CLI est connectée (version {version}) : mettre Claude Code à jour.")),
    }
}

impl Outils for OutilsDuMac {
    fn telecharger(&mut self, url: &str, partiel: &Path, taille: u64) -> TacheDe<()> {
        let enfant = Command::new("/usr/bin/curl")
            .args(["--fail", "--location", "--silent", "--show-error", "--retry", "3", "--retry-delay", "2", "--connect-timeout", "30"])
            .args(["--speed-limit", "1024", "--speed-time", "60", "--continue-at", "-", "--output"])
            .arg(partiel)
            .arg(url)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn();
        match enfant {
            Ok(enfant) => Box::new(Curl { enfant, partiel: partiel.to_path_buf(), taille }),
            Err(e) => crate::faux::FausseTache::boite(0.0, Err(format!("curl : {e}"))),
        }
    }

    fn empreinte(&mut self, fichier: &Path) -> TacheDe<String> {
        let fichier = fichier.to_path_buf();
        sur_un_fil(move |progres, arret| {
            let mut f = std::fs::File::open(&fichier).map_err(|e| format!("{} : {e}", fichier.display()))?;
            let total = f.metadata().map_or(1, |m| m.len().max(1));
            let mut sha = Sha256::new();
            let mut tampon = vec![0u8; 8 << 20];
            let mut lu = 0u64;
            loop {
                if arret.load(Ordering::Relaxed) {
                    return Err("arrêté".into());
                }
                let n = f.read(&mut tampon).map_err(|e| format!("{} : {e}", fichier.display()))?;
                if n == 0 {
                    return Ok(hex(&sha.finalize()));
                }
                sha.update(&tampon[..n]);
                lu += n as u64;
                progres.store((lu as f64 / total as f64).to_bits(), Ordering::Relaxed);
            }
        })
    }

    fn installer_python(&mut self, projet: &Path, dossier: &Path) -> TacheDe<()> {
        let (uv, projet, dossier) = (self.uv.clone(), projet.to_path_buf(), dossier.to_path_buf());
        sur_un_fil(move |_, arret| {
            let uv = uv.ok_or("uv embarqué introuvable : installer l'app avec scripts/installer-app.")?;
            let env = environnement_python(&dossier);
            std::fs::create_dir_all(dossier.join("tmp")).map_err(|e| e.to_string())?;
            let mut sync = Command::new(&uv);
            sync.args(["sync", "--frozen", "--project"]).arg(&projet).current_dir(&projet).env_clear().envs(env.iter().cloned());
            lancer(sync, &dossier.join("uv.log"), "uv sync", arret)?;
            let mut import = Command::new(python(&dossier));
            import.args(["-c", "import pyannote.audio"]).current_dir(&dossier).env_clear().envs(env.iter().cloned());
            lancer(import, &dossier.join("import.log"), "import pyannote.audio", arret)?;
            // Le cache d'uv ne sert qu'à l'installation : l'environnement a ses propres copies.
            let _ = std::fs::remove_dir_all(dossier.join("cache"));
            let _ = std::fs::remove_dir_all(dossier.join("tmp"));
            Ok(())
        })
    }

    fn verifier_claude(&mut self) -> TacheDe<EtatDeClaude> {
        sur_un_fil(|_, _| verifier_claude())
    }
}

#[cfg(test)]
mod tests {
    use super::champ;

    #[test]
    fn lit_les_champs_de_claude_auth_status() {
        let json = "{\n  \"loggedIn\": true,\n  \"authMethod\": \"claude.ai\",\n  \"apiProvider\": \"firstParty\"\n}";
        assert_eq!(champ(json, "loggedIn").as_deref(), Some("true"));
        assert_eq!(champ(json, "authMethod").as_deref(), Some("claude.ai"));
        assert_eq!(champ("{\"loggedIn\": false}", "loggedIn").as_deref(), Some("false"));
        assert_eq!(champ("Unknown command auth", "loggedIn"), None);
    }
}
