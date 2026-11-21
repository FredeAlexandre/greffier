//! Le bilan par Claude : le prompt livré avec l'app (`ressources/prompt-bilan.md`), rempli pour la
//! réunion ; l'agent démarre dans le dossier de réunions, les dossiers de contexte lui sont
//! ouverts en lecture (`--add-dir`), il peut lire et écrire mais ni lancer de commandes ni aller
//! sur le web.
//! Il écrit le fichier du bilan lui-même et renvoie titre, résumé, 3 actions au plus et ses
//! propositions de noms pour les locuteurs.

use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::SystemTime;

use super::{AppelAClaude, Claude, Session};
use crate::adaptateurs::{DemandeDeBilan, Participant, ResultatDuBilan};
use crate::temps::horodatage_de_lecture;

/// Le prompt du bilan, livré avec l'app.
pub const PROMPT_DU_BILAN: &str = include_str!("../../ressources/prompt-bilan.md");

/// Lecture et écriture ; ni commandes, ni web, ni sous-agents.
pub const OUTILS_PERMIS: [&str; 5] = ["Read", "Glob", "Grep", "Write", "Edit"];
pub const OUTILS_INTERDITS: [&str; 4] = ["Bash", "WebFetch", "WebSearch", "Task"];

/// La réponse structurée demandée à l'agent.
pub const SCHEMA: &str = r#"{"type":"object","properties":{"titre":{"type":"string","description":"Titre de la réunion en quelques mots"},"resume":{"type":"string","description":"Résumé de 3 à 5 lignes, sans titre ni liste"},"actions":{"type":"array","items":{"type":"string"},"maxItems":3,"description":"Les 3 actions les plus importantes : Qui — quoi — échéance"},"participants":{"type":"array","items":{"type":"object","properties":{"locuteur":{"type":"string"},"nom":{"type":"string"},"justification":{"type":"string"},"meme_voix_que":{"type":"string"}},"required":["locuteur","nom","justification","meme_voix_que"],"additionalProperties":false}}},"required":["titre","resume","actions","participants"],"additionalProperties":false}"#;

/// Le prompt rempli pour cette réunion (chemins relatifs au dossier de réunions).
pub fn prompt(d: &DemandeDeBilan) -> String {
    let corps = PROMPT_DU_BILAN.strip_prefix("---\n").and_then(|r| r.split_once("\n---\n")).map_or(PROMPT_DU_BILAN, |(_, corps)| corps).trim();
    let relatif = |p: &Path| p.strip_prefix(&d.dossier_de_reunions).unwrap_or(p).display().to_string();
    let dossier = d.fichier_du_bilan.parent().map(relatif).unwrap_or_default();
    let ou_aucun = |lignes: Vec<String>| if lignes.is_empty() { "(aucun)".to_string() } else { lignes.join("\n") };
    let locuteurs = ou_aucun(
        d.locuteurs
            .iter()
            .map(|l| match (&l.fusionne_dans, &l.nom, &l.proposition) {
                (Some(dans), ..) => format!("- {} : même voix que {dans} (fusionné par l'utilisateur : ses répliques sont sous {dans} dans la transcription)", l.etiquette),
                (None, Some(nom), _) => format!("- {} : nommé « {nom} » par l'utilisateur (sûr)", l.etiquette),
                (None, None, Some(p)) => format!("- {} : sans nom (proposé au bilan précédent : « {p} »)", l.etiquette),
                (None, None, None) => format!("- {} : sans nom", l.etiquette),
            })
            .collect(),
    );
    let reperes = ou_aucun(d.reperes.iter().map(|r| format!("- ({}) {}", horodatage_de_lecture(r.instant), r.libelle)).collect());
    let contexte = ou_aucun(d.dossiers_de_contexte.iter().map(|c| format!("- `{}`", c.display())).collect());
    let bilan_actuel = if d.bilan_actuel.is_some() {
        "Un bilan existe déjà pour cette réunion : on le refait, sans doute après des corrections de la transcription ou des noms donnés aux locuteurs. Remplace-le entièrement, d'après la fiche et la transcription actuelles."
    } else {
        ""
    };
    let debut = d.debut;
    let valeurs = [
        ("DOSSIER", dossier),
        ("TRANSCRIPTION", relatif(&d.transcription)),
        ("FICHE", relatif(&d.fiche)),
        ("BILAN", relatif(&d.fichier_du_bilan)),
        ("DATE", format!("{:04}-{:02}-{:02}", debut.annee, debut.mois, debut.jour)),
        ("DEBUT", format!("{:02}:{:02}", debut.heure, debut.minute)),
        ("DUREE", horodatage_de_lecture(d.duree)),
        ("LOCUTEURS", locuteurs),
        ("REPERES", reperes),
        ("CONTEXTE", contexte),
        ("BILAN_ACTUEL", bilan_actuel.to_string()),
    ];
    let mut texte = corps.to_string();
    for (cle, valeur) in valeurs {
        texte = texte.replace(&format!("{{{{{cle}}}}}"), &valeur);
    }
    // Un remplacement vide (pas de bilan actuel) ne laisse pas de trou de plusieurs lignes.
    while texte.contains("\n\n\n") {
        texte = texte.replace("\n\n\n", "\n\n");
    }
    texte
}

/// L'appel à la CLI pour ce bilan : une nouvelle session dans le dossier de réunions, les
/// dossiers de contexte ouverts en plus, l'écriture permise sauf dans les dossiers de contexte
/// (ils se lisent : règles `Write(//chemin/**)` et `Edit(//chemin/**)` de la CLI, `//` = chemin
/// absolu). Un dossier de contexte qui contient le dossier de réunions reste ouvert à l'écriture,
/// le bilan s'y écrit.
pub fn appel(d: &DemandeDeBilan, session: &str) -> AppelAClaude {
    let mut outils_interdits: Vec<String> = OUTILS_INTERDITS.map(String::from).to_vec();
    for dossier in d.dossiers_de_contexte.iter().filter(|c| !d.dossier_de_reunions.starts_with(c)) {
        let motif = format!("/{}/**", dossier.display().to_string().trim_end_matches('/'));
        outils_interdits.extend([format!("Write({motif})"), format!("Edit({motif})")]);
    }
    AppelAClaude {
        prompt: prompt(d),
        entree: String::new(),
        schema: SCHEMA.into(),
        session: Session::Nouvelle(session.into()),
        repertoire: d.dossier_de_reunions.clone(),
        outils_permis: OUTILS_PERMIS.map(String::from).to_vec(),
        outils_interdits,
        ecriture: true,
        dossiers_en_plus: d.dossiers_de_contexte.clone(),
    }
}

/// Lit la réponse structurée de l'agent.
pub fn resultat(reponse: &serde_json::Value, session: &str) -> Result<ResultatDuBilan, String> {
    let texte = |v: &serde_json::Value, cle: &str| v.get(cle).and_then(serde_json::Value::as_str).map(str::trim).unwrap_or_default().to_string();
    let titre = texte(reponse, "titre");
    if titre.is_empty() {
        return Err("Claude n'a pas renvoyé le titre de la réunion.".into());
    }
    let liste = |cle: &str| reponse.get(cle).and_then(serde_json::Value::as_array).cloned().unwrap_or_default();
    let actions = liste("actions").iter().filter_map(|a| a.as_str()).map(|a| a.trim().trim_start_matches("- ").trim_start_matches("[ ]").trim().to_string()).filter(|a| !a.is_empty()).take(3).collect();
    let participants = liste("participants")
        .iter()
        .map(|p| Participant { locuteur: texte(p, "locuteur"), nom_propose: texte(p, "nom"), justification: texte(p, "justification"), meme_voix_que: Some(texte(p, "meme_voix_que")).filter(|m| !m.is_empty()) })
        .filter(|p| !p.locuteur.is_empty() && (!p.nom_propose.is_empty() || !p.justification.is_empty()))
        .collect();
    Ok(ResultatDuBilan { session: Some(session.to_string()), titre, resume: texte(reponse, "resume"), actions, participants })
}

fn modifie(fichier: &Path) -> Option<SystemTime> {
    std::fs::metadata(fichier).and_then(|m| m.modified()).ok()
}

/// Fait le bilan : l'agent écrit le fichier, puis rend sa réponse. Échoue si le fichier du bilan
/// n'a pas été (ré)écrit.
pub fn faire(claude: &dyn Claude, d: &DemandeDeBilan, arret: &AtomicBool) -> Result<ResultatDuBilan, String> {
    let session = super::nouvelle_session();
    let avant = modifie(&d.fichier_du_bilan);
    let reponse = claude.demander(&appel(d, &session), arret)?;
    let apres = modifie(&d.fichier_du_bilan);
    if apres.is_none() || apres == avant {
        return Err("Claude a terminé sans écrire le bilan.".into());
    }
    resultat(&reponse, &session)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adaptateurs::LocuteurPourLAgent;
    use crate::reunion::Repere;
    use crate::temps::DateHeure;
    use std::path::PathBuf;

    fn demande() -> DemandeDeBilan {
        let racine = PathBuf::from("/Users/odile/Réunions");
        let nom = "2026-11-21 14h02 - Réunion";
        let f = |role: &str| racine.join(nom).join(format!("{nom} - {role}"));
        DemandeDeBilan {
            dossier_de_reunions: racine.clone(),
            dossiers_de_contexte: vec![PathBuf::from("/Users/odile/Notes")],
            fichier_du_bilan: f("bilan.md"),
            fiche: f("réunion.md"),
            transcription: f("transcription.md"),
            titre: "Réunion".into(),
            debut: DateHeure::new(2026, 11, 21, 14, 2),
            duree: 3723.0,
            repliques: vec![],
            locuteurs: vec![
                LocuteurPourLAgent { etiquette: "SPEAKER_00".into(), nom: Some("Odile".into()), proposition: None, justification: String::new(), meme_voix_proposee: None, fusionne_dans: None },
                LocuteurPourLAgent { etiquette: "SPEAKER_01".into(), nom: None, proposition: Some("Bastien".into()), justification: String::new(), meme_voix_proposee: None, fusionne_dans: None },
                LocuteurPourLAgent { etiquette: "SPEAKER_02".into(), nom: None, proposition: None, justification: String::new(), meme_voix_proposee: None, fusionne_dans: Some("SPEAKER_00".into()) },
            ],
            reperes: vec![Repere { instant: 312.0, libelle: "Repère 1".into() }],
            bilan_actuel: None,
        }
    }

    #[test]
    fn le_prompt_est_rempli_pour_la_reunion() {
        let p = prompt(&demande());
        assert!(!p.contains("{{"), "{p}");
        assert!(!p.starts_with("---"), "le frontmatter du fichier n'est pas envoyé");
        assert!(p.contains("Dossier de la réunion : `2026-11-21 14h02 - Réunion`"), "{p}");
        assert!(p.contains("- Transcription : `2026-11-21 14h02 - Réunion/2026-11-21 14h02 - Réunion - transcription.md`"), "{p}");
        assert!(p.contains("Crée `2026-11-21 14h02 - Réunion/2026-11-21 14h02 - Réunion - bilan.md`"), "{p}");
        assert!(p.contains("- Date : 2026-11-21, début 14:02, durée 1:02:03"), "{p}");
        assert!(
            p.contains("- SPEAKER_00 : nommé « Odile » par l'utilisateur (sûr)\n- SPEAKER_01 : sans nom (proposé au bilan précédent : « Bastien »)\n- SPEAKER_02 : même voix que SPEAKER_00 (fusionné par l'utilisateur : ses répliques sont sous SPEAKER_00 dans la transcription)"),
            "{p}"
        );
        assert!(p.contains("`participants` : pour chaque locuteur de la liste qui n'est pas fusionné"), "une proposition par locuteur : {p}");
        assert!(p.contains("- (0:05:12) Repère 1"), "{p}");
        assert!(p.contains("- `/Users/odile/Notes`"), "{p}");
        assert!(!p.contains("Un bilan existe déjà"));
        assert!(!p.contains("\n\n\n"));

        let refait = prompt(&DemandeDeBilan { bilan_actuel: Some("## En bref".into()), dossiers_de_contexte: vec![], reperes: vec![], ..demande() });
        assert!(refait.contains("Un bilan existe déjà pour cette réunion"));
        assert!(refait.contains("si un nom, un projet ou un sigle n'est pas clair :\n(aucun)"), "{refait}");
    }

    #[test]
    fn l_agent_lit_et_ecrit_sans_commandes_ni_web_avec_les_dossiers_de_contexte() {
        let a = appel(&demande(), "s-1").arguments();
        let apres = |drapeau: &str| a.iter().position(|x| x == drapeau).map(|i| a[i + 1].as_str());
        assert_eq!(a[0], "-p");
        assert_eq!(apres("--model"), Some("opus"));
        assert_eq!(apres("--session-id"), Some("s-1"));
        assert_eq!(apres("--output-format"), Some("json"));
        assert_eq!(apres("--permission-mode"), Some("acceptEdits"));
        assert_eq!(apres("--add-dir"), Some("/Users/odile/Notes"));
        let i = a.iter().position(|x| x == "--allowedTools").unwrap();
        assert_eq!(a[i + 1..i + 6], ["Read", "Glob", "Grep", "Write", "Edit"]);
        // Les dossiers de contexte se lisent, ne s'écrivent pas.
        let i = a.iter().position(|x| x == "--disallowedTools").unwrap();
        assert_eq!(a[i + 1..], ["Bash", "WebFetch", "WebSearch", "Task", "Write(//Users/odile/Notes/**)", "Edit(//Users/odile/Notes/**)"]);
        assert!(serde_json::from_str::<serde_json::Value>(SCHEMA).is_ok());
        assert_eq!(appel(&demande(), "s").repertoire, PathBuf::from("/Users/odile/Réunions"));

        let sans_contexte = appel(&DemandeDeBilan { dossiers_de_contexte: vec![], ..demande() }, "s").arguments();
        assert!(!sans_contexte.contains(&"--add-dir".to_string()));

        // Un dossier de contexte qui contient le dossier de réunions n'est pas fermé à l'écriture :
        // le bilan s'y écrit.
        let parent = appel(&DemandeDeBilan { dossiers_de_contexte: vec![PathBuf::from("/Users/odile")], ..demande() }, "s");
        assert_eq!(parent.outils_interdits, ["Bash", "WebFetch", "WebSearch", "Task"]);
    }

    /// Une fausse CLI : écrit (ou non) le fichier du bilan, et rend la réponse donnée.
    struct FausseCli {
        ecrire: bool,
        reponse: serde_json::Value,
    }

    impl Claude for FausseCli {
        fn demander(&self, appel: &AppelAClaude, _: &AtomicBool) -> Result<serde_json::Value, String> {
            if self.ecrire {
                let fichier = appel.prompt.split("Crée `").nth(1).and_then(|r| r.split('`').next()).expect("le fichier du bilan dans le prompt");
                std::fs::write(appel.repertoire.join(fichier), "## En bref\n").map_err(|e| e.to_string())?;
            }
            Ok(self.reponse.clone())
        }
    }

    #[test]
    fn le_bilan_echoue_si_l_agent_n_ecrit_pas_le_fichier() {
        let racine = std::env::temp_dir().join(format!("greffier-bilan-claude-{}", std::process::id()));
        let nom = "2026-11-21 14h02 - Réunion";
        std::fs::create_dir_all(racine.join(nom)).unwrap();
        let d = DemandeDeBilan { dossier_de_reunions: racine.clone(), fichier_du_bilan: racine.join(nom).join(format!("{nom} - bilan.md")), ..demande() };
        let reponse = serde_json::json!({"titre": "Budget", "resume": "R.", "actions": [], "participants": []});
        let arret = AtomicBool::new(false);

        let muette = FausseCli { ecrire: false, reponse: reponse.clone() };
        assert_eq!(faire(&muette, &d, &arret), Err("Claude a terminé sans écrire le bilan.".into()));

        let r = faire(&FausseCli { ecrire: true, reponse }, &d, &arret).unwrap();
        assert_eq!(r.titre, "Budget");
        assert_eq!(r.session.as_deref().map(str::len), Some(36), "la session donnée à la CLI");
        let _ = std::fs::remove_dir_all(&racine);
    }

    #[test]
    fn la_reponse_donne_titre_resume_actions_et_participants() {
        let reponse = serde_json::json!({
            "titre": " Budget du trimestre ",
            "resume": "On garde le budget.",
            "actions": ["- [ ] Odile — envoyer le devis — lundi", "Bastien — relire", "C", "D"],
            "participants": [
                {"locuteur": "SPEAKER_01", "nom": "Bastien", "justification": "Odile l'appelle Bastien (0:00:04).", "meme_voix_que": ""},
                {"locuteur": "SPEAKER_02", "nom": "Bastien", "justification": "Même voix.", "meme_voix_que": "SPEAKER_01"},
                {"locuteur": "SPEAKER_03", "nom": " ", "justification": "Personne ne le nomme.", "meme_voix_que": ""},
                {"locuteur": "", "nom": "Odile", "justification": "Sans locuteur.", "meme_voix_que": ""}
            ]
        });
        let r = resultat(&reponse, "s-1").unwrap();
        assert_eq!(r.session.as_deref(), Some("s-1"));
        assert_eq!(r.titre, "Budget du trimestre");
        assert_eq!(r.resume, "On garde le budget.");
        assert_eq!(r.actions, ["Odile — envoyer le devis — lundi", "Bastien — relire", "C"]);
        assert_eq!(r.participants[0].meme_voix_que, None);
        assert_eq!(r.participants[1].meme_voix_que.as_deref(), Some("SPEAKER_01"));
        assert_eq!(r.participants.len(), 3, "un locuteur sans nom proposé garde sa justification");
        assert_eq!((r.participants[2].nom_propose.as_str(), r.participants[2].justification.as_str()), ("", "Personne ne le nomme."));
        assert!(resultat(&serde_json::json!({"titre": " ", "resume": "", "actions": [], "participants": []}), "s").is_err());
    }
}
