//! La relecture par la CLI `claude`, en deux passes dans une même session (ADR 0002) :
//!
//! 1. l'agent reçoit les répliques numérotées (`#N [h:mm:ss → h:mm:ss] SPEAKER_xx : texte`, sur
//!    l'entrée standard), le lexique et le contexte ; il rend les corrections sûres, les
//!    changements de locuteur prouvés, les passages douteux (30 au plus) et les ajouts au lexique ;
//! 2. **réécoute** : chaque passage douteux est extrait de l'enregistrement (±1,5 s autour de sa
//!    réplique, 28 s au plus) et retranscrit avec le vocabulaire ;
//! 3. dans la même session, l'agent tranche chaque passage : corrigé, ou à vérifier.
//!
//! L'agent ne touche à aucun fichier (outils en lecture seule) : ce module traduit ses réponses en
//! [`ResultatDeRelecture`], que le cœur applique lui-même. Le prompt est livré avec l'app
//! (`ressources/prompt-relecture.md`) ; sa partie « Deuxième passe » est envoyée après la réécoute.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::Deserialize;
use serde_json::{Value, json};

use super::{AppelAClaude, Claude, Session, nouvelle_session};
use greffier_coeur::adaptateurs::{AjoutAuLexique, ChangementDeLocuteur, CorrectionSure, DemandeDeRelecture, PassageCorrige, PassageSignale, RepliquePourLAgent, ResultatDeRelecture};
use greffier_coeur::horodatage_de_lecture;

/// Le prompt de la relecture livré avec l'app.
pub const PROMPT: &str = include_str!("../../ressources/prompt-relecture.md");

/// Le titre qui ouvre la deuxième partie du prompt.
const DEUXIEME_PASSE: &str = "## Deuxième passe";

/// Au plus autant de passages douteux réécoutés.
pub const PASSAGES_AU_PLUS: usize = 30;

/// Les paliers d'avancement : première passe, réécoute, deuxième passe.
pub const PALIERS: [f64; 3] = [0.0, 0.6, 0.75];

const OUTILS_PERMIS: [&str; 3] = ["Read", "Glob", "Grep"];
const OUTILS_INTERDITS: [&str; 6] = ["Write", "Edit", "Bash", "WebFetch", "WebSearch", "Task"];

/// Retranscrit des extraits de l'enregistrement (début, fin en secondes) avec les réglages de la
/// transcription ; un texte par extrait, vide s'il n'y a rien d'exploitable.
pub trait Reecoute {
    fn reecouter(&self, enregistrement: &Path, extraits: &[(f64, f64)], vocabulaire: &str, arret: &AtomicBool) -> Result<Vec<String>, String>;
}

// --- Les réponses de l'agent -----------------------------------------------------------------

#[derive(Deserialize)]
struct Correction {
    ligne: usize,
    avant: String,
    apres: String,
}

#[derive(Deserialize)]
struct Locuteur {
    ligne: usize,
    locuteur: String,
}

#[derive(Clone, Deserialize)]
struct Doute {
    ligne: usize,
    extrait: String,
    #[serde(default)]
    hypothese: String,
}

#[derive(Deserialize)]
struct Entree {
    entendu: String,
    correct: String,
}

#[derive(Deserialize)]
struct PremierePasse {
    #[serde(default)]
    corrections: Vec<Correction>,
    #[serde(default)]
    locuteurs: Vec<Locuteur>,
    #[serde(default)]
    doutes: Vec<Doute>,
    #[serde(default)]
    lexique: Vec<Entree>,
}

#[derive(Deserialize)]
struct Decision {
    ligne: usize,
    #[serde(default)]
    extrait: String,
    statut: String,
    #[serde(default)]
    apres: String,
}

#[derive(Deserialize)]
struct DeuxiemePasse {
    #[serde(default)]
    decisions: Vec<Decision>,
    #[serde(default)]
    lexique: Vec<Entree>,
}

fn liste(proprietes: Value) -> Value {
    let requises: Vec<&String> = proprietes.as_object().map(|o| o.keys().collect()).unwrap_or_default();
    json!({ "type": "array", "items": { "type": "object", "properties": proprietes, "required": requises, "additionalProperties": false } })
}

fn schema_de_la_premiere_passe() -> String {
    let (texte, entier) = (json!({ "type": "string" }), json!({ "type": "integer" }));
    json!({
        "type": "object",
        "properties": {
            "corrections": liste(json!({ "ligne": entier, "avant": texte, "apres": texte, "raison": texte })),
            "locuteurs": liste(json!({ "ligne": entier, "locuteur": texte, "raison": texte })),
            "doutes": liste(json!({ "ligne": entier, "extrait": texte, "hypothese": texte, "raison": texte })),
            "lexique": liste(json!({ "entendu": texte, "correct": texte })),
        },
        "required": ["corrections", "locuteurs", "doutes", "lexique"],
        "additionalProperties": false,
    })
    .to_string()
}

fn schema_de_la_deuxieme_passe() -> String {
    let (texte, entier) = (json!({ "type": "string" }), json!({ "type": "integer" }));
    json!({
        "type": "object",
        "properties": {
            "decisions": liste(json!({ "ligne": entier, "extrait": texte, "statut": { "type": "string", "enum": ["corrige", "a_verifier"] }, "apres": texte, "raison": texte })),
            "lexique": liste(json!({ "entendu": texte, "correct": texte })),
        },
        "required": ["decisions", "lexique"],
        "additionalProperties": false,
    })
    .to_string()
}

fn lire<T: for<'a> Deserialize<'a>>(v: Value) -> Result<T, String> {
    serde_json::from_value(v).map_err(|e| format!("Claude a rendu une réponse illisible : {e}"))
}

// --- Le prompt -----------------------------------------------------------------------------

/// Le lexique tel qu'on le donne à l'agent : sans frontmatter ni titre principal.
fn lexique_pour_l_agent(md: &str) -> String {
    let mut corps = md;
    if let Some(reste) = corps.strip_prefix("---\n")
        && let Some(fin) = reste.find("\n---\n")
    {
        corps = &reste[fin + 5..];
    }
    let corps: Vec<&str> = corps.lines().collect();
    let sans_titre = match corps.iter().position(|l| l.starts_with("# ")) {
        Some(i) => [&corps[..i], &corps[i + 1..]].concat(),
        None => corps,
    };
    let texte = sans_titre.join("\n");
    if texte.trim().is_empty() { "(vide)".into() } else { texte.trim().to_string() }
}

/// Les deux parties du prompt, `{{…}}` remplacés.
fn prompts(d: &DemandeDeRelecture) -> (String, String) {
    let corps = match PROMPT.strip_prefix("---\n").and_then(|r| r.find("\n---\n").map(|i| &r[i + 5..])) {
        Some(c) => c,
        None => PROMPT,
    };
    let (premiere, deuxieme) = corps.split_at(corps.find(DEUXIEME_PASSE).unwrap_or(corps.len()));
    let mut personnes = d.nombre_de_locuteurs.map(|n| format!("{n} personnes dans la réunion.")).unwrap_or_default();
    let mut noms: Vec<String> = d.repliques.iter().filter_map(|r| r.nom.as_ref().map(|n| format!("{} = {n}", r.locuteur))).collect();
    noms.sort();
    noms.dedup();
    if !noms.is_empty() {
        personnes = format!("{personnes} Noms déjà donnés : {}.", noms.join(", ")).trim().to_string();
    }
    let date = format!("{:04}-{:02}-{:02}", d.debut.annee, d.debut.mois, d.debut.jour);
    let remplir = |t: &str| {
        t.replace("{{DATE}}", &date)
            .replace("{{TITRE}}", &d.titre)
            .replace("{{DOSSIER}}", &d.nom_du_dossier)
            .replace("{{PERSONNES}}", &personnes)
            .replace("{{LEXIQUE}}", &lexique_pour_l_agent(&d.lexique))
            .trim()
            .to_string()
    };
    (remplir(premiere), remplir(deuxieme))
}

/// `#N [h:mm:ss → h:mm:ss] SPEAKER_xx : texte`, numérotées à partir de 1.
fn repliques_numerotees(repliques: &[RepliquePourLAgent]) -> String {
    let lignes: Vec<String> = repliques.iter().enumerate().map(|(i, r)| format!("#{} [{} → {}] {} : {}", i + 1, horodatage_de_lecture(r.debut), horodatage_de_lecture(r.fin), r.locuteur, r.texte)).collect();
    lignes.join("\n")
}

/// Une hypothèse qui n'en est pas une (vide, « ? », « ? (peut-être…) ») ne compte pas.
fn hypothese(h: &str) -> Option<String> {
    let h = h.trim();
    (!h.is_empty() && !h.starts_with('?')).then(|| h.to_string())
}

// --- La relecture --------------------------------------------------------------------------

/// Les deux passes et la réécoute, sur le fil appelant. `progres` reçoit le palier atteint
/// ([`PALIERS`]). Un échec de la réécoute n'arrête pas la relecture : l'agent tranche sans elle.
pub fn relire(claude: &dyn Claude, reecoute: &dyn Reecoute, d: &DemandeDeRelecture, arret: &AtomicBool, progres: &mut dyn FnMut(f64)) -> Result<ResultatDeRelecture, String> {
    let arrete = || if arret.load(Ordering::Relaxed) { Err("arrêté".to_string()) } else { Ok(()) };
    let session = nouvelle_session();
    let (premiere, deuxieme) = prompts(d);
    let appel = |prompt: String, entree: String, schema: String, session: Session| AppelAClaude {
        prompt,
        entree,
        schema,
        session,
        repertoire: d.dossier_de_reunions.clone(),
        outils_permis: OUTILS_PERMIS.map(String::from).to_vec(),
        outils_interdits: OUTILS_INTERDITS.map(String::from).to_vec(),
        ecriture: false,
        dossiers_en_plus: Vec::new(),
    };
    progres(PALIERS[0]);
    let p1: PremierePasse = lire(claude.demander(&appel(premiere, repliques_numerotees(&d.repliques), schema_de_la_premiere_passe(), Session::Nouvelle(session.clone())), arret)?)?;
    arrete()?;

    // La ligne N de l'agent est la N-ième réplique envoyée.
    let replique = |ligne: usize| ligne.checked_sub(1).and_then(|i| d.repliques.get(i));
    let mut resultat = ResultatDeRelecture { session: Some(session.clone()), ..Default::default() };
    resultat.corrections = p1.corrections.into_iter().filter_map(|c| Some(CorrectionSure { replique: replique(c.ligne)?.numero, avant: c.avant, apres: c.apres })).collect();
    resultat.changements_de_locuteur = p1.locuteurs.into_iter().filter_map(|c| Some(ChangementDeLocuteur { replique: replique(c.ligne)?.numero, locuteur: c.locuteur })).collect();
    resultat.ajouts_au_lexique = p1.lexique.into_iter().map(|e| AjoutAuLexique { entendu: e.entendu, correct: e.correct }).collect();

    let mut doutes: Vec<Doute> = Vec::new();
    for x in p1.doutes {
        let vu = doutes.iter().any(|y| y.ligne == x.ligne && y.extrait == x.extrait);
        if replique(x.ligne).is_some() && !x.extrait.trim().is_empty() && !vu && doutes.len() < PASSAGES_AU_PLUS {
            doutes.push(x);
        }
    }
    if doutes.is_empty() {
        progres(1.0);
        return Ok(resultat);
    }

    progres(PALIERS[1]);
    let extraits: Vec<(f64, f64)> = doutes.iter().map(|x| {
        let r = replique(x.ligne).expect("ligne vérifiée");
        greffier_coeur::extrait_a_reecouter(r.debut, r.fin, &r.texte, &x.extrait)
    }).collect();
    let (reecoutes, pour_l_agent): (Vec<String>, Vec<String>) = match reecoute.reecouter(&d.enregistrement, &extraits, &d.vocabulaire, arret) {
        Ok(textes) => {
            let textes: Vec<String> = (0..doutes.len()).map(|i| textes.get(i).map(|t| greffier_coeur::attribution::texte_sans_les_bruits(t)).unwrap_or_default()).collect();
            let lus = textes.iter().map(|t| if t.is_empty() { "(rien d'exploitable)".to_string() } else { t.clone() }).collect();
            (textes, lus)
        }
        Err(e) => (vec![String::new(); doutes.len()], vec![format!("(réécoute impossible : {e})"); doutes.len()]),
    };
    arrete()?;

    progres(PALIERS[2]);
    let listage: Vec<String> = doutes
        .iter()
        .zip(&pour_l_agent)
        .map(|(x, entendu)| {
            let h = if x.hypothese.trim().is_empty() { "?" } else { x.hypothese.trim() };
            let debut = replique(x.ligne).map_or(0.0, |r| r.debut);
            format!("- ligne #{} [{}] · extrait : « {} » · ton hypothèse : « {h} »\n  réécoute : « {entendu} »", x.ligne, horodatage_de_lecture(debut), x.extrait)
        })
        .collect();
    let prompt = format!("{deuxieme}\n\n{}", listage.join("\n"));
    let p2: DeuxiemePasse = lire(claude.demander(&appel(prompt, String::new(), schema_de_la_deuxieme_passe(), Session::Reprise(session)), arret)?)?;

    for (x, reecoute) in doutes.into_iter().zip(reecoutes) {
        let numero = replique(x.ligne).expect("ligne vérifiée").numero;
        let decision = p2.decisions.iter().find(|y| y.ligne == x.ligne && y.extrait == x.extrait).or_else(|| p2.decisions.iter().find(|y| y.ligne == x.ligne));
        match decision {
            Some(y) if y.statut == "corrige" && hypothese(&y.apres).is_some() => {
                // Corrigé en lui-même : l'extrait était juste, rien à changer.
                if y.apres.trim() != x.extrait.trim() {
                    resultat.passages_corriges.push(PassageCorrige { replique: numero, extrait: x.extrait, apres: y.apres.trim().to_string(), reecoute });
                }
            }
            _ => {
                let h = decision.and_then(|y| hypothese(&y.apres)).or_else(|| hypothese(&x.hypothese));
                resultat.passages_a_verifier.push(PassageSignale { replique: numero, extrait: x.extrait, hypothese: h, reecoute });
            }
        }
    }
    resultat.ajouts_au_lexique.extend(p2.lexique.into_iter().map(|e| AjoutAuLexique { entendu: e.entendu, correct: e.correct }));
    progres(1.0);
    Ok(resultat)
}
