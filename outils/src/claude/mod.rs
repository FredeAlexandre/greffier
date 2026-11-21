//! L'agent de Greffier : la CLI `claude` installée sur le Mac (ADR 0002), modèle opus, en mode
//! non interactif (`claude -p … --output-format json --json-schema …`), avec des sessions
//! reprenables.
//!
//! - [`Claude`] : un appel à la CLI, qui rend la sortie structurée ; [`ClaudeDuMac`] la lance
//!   vraiment, les tests la remplacent.
//! - [`relecture`] : la relecture en deux passes, réécoute comprise.
//! - [`bilan`] : le bilan, que l'agent écrit lui-même.
//! - [`AgentClaude`] : l'adaptateur [`Agent`] branché dans l'app.

pub mod bilan;
pub mod relecture;

use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use serde_json::Value;

use crate::processus::{Interruption, Progression, attendre, sur_un_fil};

use greffier_coeur::adaptateurs::{Agent, DemandeDeBilan, DemandeDeRelecture, ResultatDeRelecture, ResultatDuBilan, TacheDe};

/// Le modèle demandé à la CLI.
pub const MODELE: &str = "opus";

/// Une nouvelle conversation, avec l'identifiant qu'on lui donne, ou la reprise d'une conversation.
#[derive(Clone, Debug, PartialEq)]
pub enum Session {
    Nouvelle(String),
    Reprise(String),
}

/// Un appel à la CLI `claude`.
#[derive(Clone, Debug, PartialEq)]
pub struct AppelAClaude {
    pub prompt: String,
    /// Donné sur l'entrée standard.
    pub entree: String,
    /// Le schéma JSON de la réponse structurée.
    pub schema: String,
    pub session: Session,
    /// Le répertoire de travail de l'agent.
    pub repertoire: PathBuf,
    pub outils_permis: Vec<String>,
    pub outils_interdits: Vec<String>,
    /// L'agent écrit des fichiers sans demander (`--permission-mode acceptEdits`).
    pub ecriture: bool,
    /// Des dossiers ouverts à l'agent en plus de son répertoire de travail (`--add-dir`).
    pub dossiers_en_plus: Vec<PathBuf>,
}

impl AppelAClaude {
    /// Les arguments de la CLI (le prompt compris).
    pub fn arguments(&self) -> Vec<String> {
        let mut a: Vec<String> = vec!["-p".into(), self.prompt.clone(), "--model".into(), MODELE.into()];
        match &self.session {
            Session::Nouvelle(id) => a.extend(["--session-id".into(), id.clone()]),
            Session::Reprise(id) => a.extend(["--resume".into(), id.clone()]),
        }
        a.extend(["--output-format".into(), "json".into(), "--json-schema".into(), self.schema.clone()]);
        if self.ecriture {
            a.extend(["--permission-mode".into(), "acceptEdits".into()]);
        }
        if !self.dossiers_en_plus.is_empty() {
            a.push("--add-dir".into());
            a.extend(self.dossiers_en_plus.iter().map(|d| d.display().to_string()));
        }
        a.push("--allowedTools".into());
        a.extend(self.outils_permis.iter().cloned());
        a.push("--disallowedTools".into());
        a.extend(self.outils_interdits.iter().cloned());
        a
    }
}

/// Lance la CLI `claude` et rend sa réponse structurée (`structured_output`), ou un message
/// d'erreur lisible. `arret` l'interrompt.
pub trait Claude {
    fn demander(&self, appel: &AppelAClaude, arret: &AtomicBool) -> Result<Value, String>;
}

/// Un identifiant de session neuf (UUID v4), comme `claude --session-id` l'attend.
pub fn nouvelle_session() -> String {
    let mut octets = [0u8; 16];
    let lu = std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut octets));
    if lu.is_err() {
        // Sans /dev/urandom : l'heure et l'aléa de la table de hachage de la bibliothèque standard.
        use std::hash::{BuildHasher, Hasher};
        for (i, morceau) in octets.chunks_mut(8).enumerate() {
            let mut h = std::collections::hash_map::RandomState::new().build_hasher();
            h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos()) + i as u128);
            morceau.copy_from_slice(&h.finish().to_le_bytes());
        }
    }
    octets[6] = (octets[6] & 0x0f) | 0x40;
    octets[8] = (octets[8] & 0x3f) | 0x80;
    let h: String = octets.iter().map(|o| format!("{o:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

fn tronquer(s: &str, n: usize) -> String {
    let s = s.trim();
    match s.char_indices().nth(n) {
        Some((i, _)) => format!("{}…", &s[..i]),
        None => s.to_string(),
    }
}

/// Lit ce que la CLI a rendu (`--output-format json`) : la réponse structurée, sinon un message
/// qui dit pourquoi (erreur de Claude, fin de la sortie d'erreur…).
pub fn lire_la_reponse(sortie: &str, erreurs: &str, reussi: bool) -> Result<Value, String> {
    let fin_des_erreurs = || {
        let lignes: Vec<&str> = erreurs.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
        lignes[lignes.len().saturating_sub(2)..].join(" / ")
    };
    let json: Option<Value> = serde_json::from_str(sortie.trim()).ok().or_else(|| sortie.lines().rev().find_map(|l| serde_json::from_str(l.trim()).ok()));
    let Some(json) = json.filter(Value::is_object) else {
        let fin = fin_des_erreurs();
        return Err(if fin.is_empty() { "Claude s'est arrêté sans rien dire.".into() } else { format!("Claude : {}", tronquer(&fin, 300)) });
    };
    let texte = |cle: &str| json.get(cle).and_then(|v| v.as_str().map(str::to_string).or_else(|| (!v.is_null()).then(|| v.to_string()))).filter(|s| !s.trim().is_empty());
    if !reussi || json.get("is_error").and_then(Value::as_bool) == Some(true) {
        let pourquoi = texte("result").or_else(|| texte("api_error_status")).unwrap_or_else(fin_des_erreurs);
        return Err(if pourquoi.is_empty() { "Claude a échoué sans dire pourquoi.".into() } else { format!("Claude : {}", tronquer(&pourquoi, 300)) });
    }
    match json.get("structured_output") {
        Some(v) if !v.is_null() => Ok(v.clone()),
        _ => Err(format!("Claude n'a pas rendu de réponse structurée : {}", tronquer(&texte("result").unwrap_or_default(), 300))),
    }
}

/// La vraie CLI `claude`, trouvée là où ses installateurs la mettent.
pub struct ClaudeDuMac {
    /// Au-delà, l'appel est abandonné.
    pub delai: Duration,
}

impl Default for ClaudeDuMac {
    fn default() -> ClaudeDuMac {
        ClaudeDuMac { delai: Duration::from_secs(40 * 60) }
    }
}

impl Claude for ClaudeDuMac {
    fn demander(&self, appel: &AppelAClaude, arret: &AtomicBool) -> Result<Value, String> {
        let Some(claude) = crate::mac::trouver_claude() else {
            return Err("La CLI `claude` est introuvable : installer Claude Code (https://claude.com/claude-code), s'y connecter (`claude` dans un terminal), puis réessayer cette étape.".into());
        };
        // Lancée du Finder, l'app n'a pas le PATH du terminal : on y met au moins le dossier de la CLI.
        let mut chemin: Vec<PathBuf> = claude.parent().map(|d| vec![d.to_path_buf()]).unwrap_or_default();
        chemin.extend(std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect::<Vec<_>>()).unwrap_or_default());
        chemin.extend(["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin", "/usr/sbin", "/sbin"].map(PathBuf::from));
        let chemin = std::env::join_paths(chemin).unwrap_or_default();
        let mut enfant = Command::new(&claude)
            .args(appel.arguments())
            .current_dir(&appel.repertoire)
            .env("PATH", chemin)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Impossible de lancer `{}` : {e}", claude.display()))?;
        let lire = |flux: Option<Box<dyn Read + Send>>| {
            std::thread::spawn(move || {
                let mut texte = String::new();
                if let Some(mut f) = flux {
                    let _ = f.read_to_string(&mut texte);
                }
                texte
            })
        };
        let sortie = lire(enfant.stdout.take().map(|s| Box::new(s) as Box<dyn Read + Send>));
        let erreurs = lire(enfant.stderr.take().map(|s| Box::new(s) as Box<dyn Read + Send>));
        if let Some(mut entree) = enfant.stdin.take() {
            let texte = appel.entree.clone();
            std::thread::spawn(move || {
                let _ = entree.write_all(texte.as_bytes());
            });
        }
        let statut = match attendre(&mut enfant, arret, Some(self.delai), Duration::from_millis(200), || {}) {
            Ok(s) => s,
            Err(Interruption::Arrete) => return Err("arrêté".into()),
            Err(Interruption::Delai) => return Err(format!("Claude n'a pas répondu en {} min.", self.delai.as_secs() / 60)),
            Err(Interruption::Erreur(e)) => return Err(format!("claude : {e}")),
        };
        lire_la_reponse(&sortie.join().unwrap_or_default(), &erreurs.join().unwrap_or_default(), statut.success())
    }
}

/// L'agent branché dans l'app : la relecture (avec la réécoute donnée) et le bilan par la CLI
/// `claude`.
pub struct AgentClaude {
    claude: Arc<dyn Claude + Send + Sync>,
    reecoute: Arc<dyn relecture::Reecoute + Send + Sync>,
}

impl AgentClaude {
    pub fn new(claude: Arc<dyn Claude + Send + Sync>, reecoute: Arc<dyn relecture::Reecoute + Send + Sync>) -> AgentClaude {
        AgentClaude { claude, reecoute }
    }
}

impl Agent for AgentClaude {
    fn relire(&mut self, demande: DemandeDeRelecture) -> TacheDe<ResultatDeRelecture> {
        let (claude, reecoute) = (self.claude.clone(), self.reecoute.clone());
        sur_un_fil("claude", Progression::Paliers(&relecture::PALIERS), move |arret, progres| relecture::relire(claude.as_ref(), reecoute.as_ref(), &demande, arret, progres))
    }

    fn faire_le_bilan(&mut self, demande: DemandeDeBilan) -> TacheDe<ResultatDuBilan> {
        let claude = self.claude.clone();
        sur_un_fil("claude", Progression::Paliers(&[]), move |arret, _| bilan::faire(claude.as_ref(), &demande, arret))
    }
}
