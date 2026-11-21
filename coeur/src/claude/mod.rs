//! L'agent réel (ADR 0002) : la CLI `claude` de l'utilisateur (Claude Code), lancée en mode non
//! interactif (`claude -p`) avec une réponse structurée (`--output-format json --json-schema`).
//!
//! Ce module porte ce qui est commun aux deux étapes : trouver la CLI, lancer un appel sur un fil
//! (arrêt et délai compris), lire sa réponse. Le bilan est dans [`bilan`] ; la relecture viendra à
//! côté.

pub mod bilan;

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::adaptateurs::{Agent, DemandeDeBilan, DemandeDeRelecture, ResultatDeRelecture, ResultatDuBilan, TacheDe};

/// Le modèle des deux étapes.
pub const MODELE: &str = "opus";

/// La relecture et le bilan par la CLI `claude`.
#[derive(Default)]
pub struct AgentClaude;

impl AgentClaude {
    pub fn new() -> AgentClaude {
        AgentClaude
    }
}

impl Agent for AgentClaude {
    fn relire(&mut self, _: DemandeDeRelecture) -> TacheDe<ResultatDeRelecture> {
        crate::faux::FausseTache::boite(0.0, Err("La relecture par Claude n'est pas encore disponible dans cette version de Greffier.".into()))
    }

    fn faire_le_bilan(&mut self, demande: DemandeDeBilan) -> TacheDe<ResultatDuBilan> {
        bilan::faire(demande)
    }
}

/// Un appel de `claude -p`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Appel {
    /// Le répertoire de travail de l'agent.
    pub repertoire: PathBuf,
    /// Tout ce qui suit `claude` (le prompt compris).
    pub arguments: Vec<String>,
    pub delai: Duration,
}

/// Lance l'appel jusqu'au bout (ou jusqu'à l'arrêt, ou au délai) et rend le `structured_output`
/// de sa réponse.
pub(crate) fn appeler(appel: &Appel, arret: &AtomicBool) -> Result<serde_json::Value, String> {
    let claude = crate::mac::trouver_claude().ok_or("La CLI claude est introuvable : installer Claude Code (claude.com/claude-code), puis s'y connecter avec `claude`.")?;
    let mut enfant = Command::new(&claude)
        .args(&appel.arguments)
        .current_dir(&appel.repertoire)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("{} : {e}", claude.display()))?;
    let lire = |flux: Option<Box<dyn std::io::Read + Send>>| {
        std::thread::spawn(move || {
            let mut texte = String::new();
            if let Some(mut f) = flux {
                let _ = f.read_to_string(&mut texte);
            }
            texte
        })
    };
    let sortie = lire(enfant.stdout.take().map(|f| Box::new(f) as _));
    let erreurs = lire(enfant.stderr.take().map(|f| Box::new(f) as _));
    let debut = Instant::now();
    let statut = loop {
        if arret.load(Ordering::Relaxed) || debut.elapsed() > appel.delai {
            let _ = enfant.kill();
            let _ = enfant.wait();
            return Err(if arret.load(Ordering::Relaxed) { "Claude : arrêté".into() } else { format!("Claude n'a pas fini en {} min.", appel.delai.as_secs() / 60) });
        }
        match enfant.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) => std::thread::sleep(Duration::from_millis(200)),
            Err(e) => return Err(format!("claude : {e}")),
        }
    };
    lire_la_reponse(statut.success(), &sortie.join().unwrap_or_default(), &erreurs.join().unwrap_or_default())
}

/// La réponse de `claude -p --output-format json` : son `structured_output`, ou l'erreur la
/// plus parlante (le `result` de Claude, l'erreur de l'API, les dernières lignes d'erreur).
pub(crate) fn lire_la_reponse(succes: bool, sortie: &str, erreurs: &str) -> Result<serde_json::Value, String> {
    let json: Option<serde_json::Value> = serde_json::from_str(sortie.trim()).ok();
    let en_erreur = json.as_ref().is_none_or(|j| j.get("is_error").and_then(serde_json::Value::as_bool).unwrap_or(false));
    if !succes || en_erreur {
        let texte = |cle: &str| json.as_ref().and_then(|j| j.get(cle)).and_then(serde_json::Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(String::from);
        let fin: Vec<&str> = erreurs.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
        let fin = fin[fin.len().saturating_sub(3)..].join(" / ");
        let pourquoi = texte("result").or_else(|| texte("api_error_status")).or((!fin.is_empty()).then_some(fin)).unwrap_or_else(|| "pas de réponse".into());
        return Err(format!("Claude : {}", pourquoi.chars().take(300).collect::<String>()));
    }
    json.and_then(|mut j| j.get_mut("structured_output").map(serde_json::Value::take)).filter(|v| !v.is_null()).ok_or_else(|| "Claude n'a pas renvoyé de réponse structurée.".into())
}

/// Un identifiant de session neuf (UUID v4), pour `--session-id` : la conversation pourra être
/// reprise avec `claude --resume <id>` depuis le dossier de réunions.
pub(crate) fn nouvelle_session() -> String {
    use std::io::Read;
    let mut o = [0u8; 16];
    let lu = std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut o));
    if lu.is_err() {
        // Sans /dev/urandom : l'heure et le processus suffisent à ne pas se répéter.
        let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos()) ^ ((std::process::id() as u128) << 64);
        o = n.to_le_bytes();
    }
    o[6] = (o[6] & 0x0f) | 0x40;
    o[8] = (o[8] & 0x3f) | 0x80;
    let h: String = o.iter().map(|b| format!("{b:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &h[..8], &h[8..12], &h[12..16], &h[16..20], &h[20..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rend_la_reponse_structuree() {
        let sortie = r#"{"type":"result","is_error":false,"result":"{}","session_id":"s","structured_output":{"titre":"Budget"}}"#;
        assert_eq!(lire_la_reponse(true, sortie, ""), Ok(serde_json::json!({"titre": "Budget"})));
    }

    #[test]
    fn une_erreur_dit_pourquoi() {
        let sortie = r#"{"type":"result","is_error":true,"result":"Credit balance is too low","structured_output":null}"#;
        assert_eq!(lire_la_reponse(false, sortie, ""), Err("Claude : Credit balance is too low".into()));
        assert_eq!(lire_la_reponse(false, "", "trace\nError: not logged in\n"), Err("Claude : trace / Error: not logged in".into()));
        assert_eq!(lire_la_reponse(true, r#"{"is_error":false,"result":"ok"}"#, ""), Err("Claude n'a pas renvoyé de réponse structurée.".into()));
    }

    #[test]
    fn une_session_est_un_uuid_v4() {
        let s = nouvelle_session();
        assert_eq!(s.len(), 36);
        assert_eq!(&s[14..15], "4");
        assert!(s.chars().all(|c| c == '-' || c.is_ascii_hexdigit()));
        assert_ne!(s, nouvelle_session());
    }
}
