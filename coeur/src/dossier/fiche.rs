//! La fiche (`… - réunion.md`) : le fichier d'entrée d'une réunion, écrit par Greffier seul.
//!
//! ```markdown
//! ---
//! titre: "Revue de release"
//! date: 2026-11-20
//! debut: "10:00"
//! duree: "0:12:34"
//! statut: echec                 # enregistrement_en_cours, enregistrement_interrompu, a_traiter,
//! etape: relecture              #   en_attente, en_cours, echec, terminee ; `etape` pour les trois
//! erreur: "La CLI claude …"     #   avant-derniers, `erreur` pour un échec
//! etape_faite: transcription    # la dernière étape menée à bien
//! nombre_de_locuteurs: 3        # absent : détecté
//! locuteurs:                    # SPEAKER_xx → nom donné par l'utilisateur
//!   SPEAKER_00: "Alexandre"
//! fusions:                      # SPEAKER_xx fusionné dans SPEAKER_yy
//!   SPEAKER_02: SPEAKER_00
//! propositions:                 # ce que l'agent du bilan propose
//!   SPEAKER_01:
//!     nom: "Khalid"
//!     justification: "Alexandre l'appelle Khalid à 0:00:04."
//!     meme_voix_que: SPEAKER_00
//! reperes:
//!   - instant: "0:05:12"
//!     libelle: "Repère 1"
//! sessions:                     # identifiants des sessions de l'agent
//!   relecture: "…"
//!   bilan: "…"
//! nouvelle: true                # le bilan n'a pas encore été ouvert
//! ---
//!
//! # Revue de release
//!
//! Le résumé.
//!
//! ## Actions principales
//!
//! - Alexandre — publier la release — lundi
//!
//! ## Fichiers
//!
//! - [Bilan](<2026-11-20 10h00 - Revue de release - bilan.md>)
//! ```
//!
//! Le frontmatter est un sous-ensemble de YAML (tables, listes, chaînes entre guillemets doubles
//! échappées comme en JSON) : tout lecteur YAML le lit, et ce module relit ce qu'il écrit.

use crate::dossier::Role;
use crate::reunion::{Etape, Repere, Reunion, Sessions, Statut};
use crate::temps::{DateHeure, horodatage_de_lecture};

/// Ce que l'agent du bilan propose pour un locuteur.
#[derive(Clone, Debug, PartialEq)]
pub struct Proposition {
    /// « SPEAKER_01 »
    pub locuteur: String,
    pub nom: String,
    pub justification: String,
    /// Sans doute la même voix qu'un autre locuteur (« SPEAKER_00 »).
    pub meme_voix_que: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Fiche {
    pub titre: String,
    pub debut: DateHeure,
    /// En secondes (écrite à la seconde près).
    pub duree: f64,
    /// `EnCours` est écrit sans son avancement (relu à 0).
    pub statut: Statut,
    pub etape_faite: Option<Etape>,
    pub nombre_de_locuteurs: Option<u8>,
    /// Les noms donnés : (« SPEAKER_00 », « Alexandre »).
    pub noms: Vec<(String, String)>,
    /// Les fusions : (« SPEAKER_02 », « SPEAKER_00 ») — le premier est la même voix que le second.
    pub fusions: Vec<(String, String)>,
    pub propositions: Vec<Proposition>,
    pub reperes: Vec<Repere>,
    pub sessions: Sessions,
    pub nouvelle: bool,
    pub resume: String,
    pub actions_principales: Vec<String>,
}

impl Fiche {
    pub fn depuis(r: &Reunion) -> Fiche {
        let etiquette = |i: usize| r.locuteurs[i].etiquette.clone();
        Fiche {
            titre: r.titre.clone(),
            debut: r.debut,
            duree: r.duree,
            statut: r.statut.clone(),
            etape_faite: r.etape_faite,
            nombre_de_locuteurs: r.nombre_de_locuteurs,
            noms: r.locuteurs.iter().filter_map(|l| Some((l.etiquette.clone(), l.nom.clone()?))).collect(),
            fusions: r.locuteurs.iter().filter_map(|l| Some((l.etiquette.clone(), etiquette(l.fusionne_dans?)))).collect(),
            propositions: r
                .locuteurs
                .iter()
                .filter_map(|l| {
                    Some(Proposition {
                        locuteur: l.etiquette.clone(),
                        nom: l.proposition.clone()?,
                        justification: l.justification.clone(),
                        meme_voix_que: l.fusion_proposee.map(etiquette),
                    })
                })
                .collect(),
            reperes: r.reperes.clone(),
            sessions: r.sessions.clone(),
            nouvelle: r.nouvelle,
            resume: r.resume.clone(),
            actions_principales: r.actions_principales.clone(),
        }
    }

    /// Reporte la fiche sur une réunion dont les locuteurs sont déjà connus (ceux de la
    /// transcription) ; un locuteur nommé dans la fiche mais inconnu est ignoré.
    pub fn appliquer(&self, r: &mut Reunion) {
        r.titre = self.titre.clone();
        r.debut = self.debut;
        if self.duree > 0.0 {
            r.duree = self.duree;
        }
        r.statut = self.statut.clone();
        r.nombre_de_locuteurs = self.nombre_de_locuteurs;
        let index = |r: &Reunion, e: &str| r.locuteurs.iter().position(|l| l.etiquette == e);
        for (e, nom) in &self.noms {
            if let Some(i) = index(r, e) {
                r.locuteurs[i].nom = Some(nom.clone());
            }
        }
        for (e, dans) in &self.fusions {
            if let (Some(i), Some(j)) = (index(r, e), index(r, dans))
                && i != j
            {
                r.locuteurs[i].fusionne_dans = Some(j);
            }
        }
        for p in &self.propositions {
            if let Some(i) = index(r, &p.locuteur) {
                let meme = p.meme_voix_que.as_deref().and_then(|e| index(r, e)).filter(|j| *j != i);
                let l = &mut r.locuteurs[i];
                l.proposition = Some(p.nom.clone());
                l.justification = p.justification.clone();
                l.fusion_proposee = meme;
            }
        }
        r.reperes = self.reperes.clone();
        r.sessions = self.sessions.clone();
        r.nouvelle = self.nouvelle;
        r.resume = self.resume.clone();
        r.actions_principales = self.actions_principales.clone();
    }

    /// Le fichier de la fiche, pour le dossier `nom_du_dossier`.
    pub fn markdown(&self, nom_du_dossier: &str) -> String {
        let mut y = String::from("---\n");
        let mut ligne = |cle: &str, valeur: String| y.push_str(&format!("{cle}: {valeur}\n"));
        ligne("titre", chaine(&self.titre));
        ligne("date", format!("{:04}-{:02}-{:02}", self.debut.annee, self.debut.mois, self.debut.jour));
        ligne("debut", chaine(&format!("{:02}:{:02}", self.debut.heure, self.debut.minute)));
        ligne("duree", chaine(&instant(self.duree)));
        let (statut, etape, erreur) = match &self.statut {
            Statut::EnregistrementEnCours => ("enregistrement_en_cours", None, None),
            Statut::EnregistrementInterrompu => ("enregistrement_interrompu", None, None),
            Statut::ATraiter => ("a_traiter", None, None),
            Statut::EnAttente(e) => ("en_attente", Some(*e), None),
            Statut::EnCours { etape, .. } => ("en_cours", Some(*etape), None),
            Statut::Echec { etape, message } => ("echec", Some(*etape), Some(message)),
            Statut::Terminee => ("terminee", None, None),
        };
        ligne("statut", statut.into());
        if let Some(e) = etape {
            ligne("etape", nom_d_etape(e).into());
        }
        if let Some(m) = erreur {
            ligne("erreur", chaine(m));
        }
        if let Some(e) = self.etape_faite {
            ligne("etape_faite", nom_d_etape(e).into());
        }
        if let Some(n) = self.nombre_de_locuteurs {
            ligne("nombre_de_locuteurs", n.to_string());
        }
        if !self.noms.is_empty() {
            y.push_str("locuteurs:\n");
            for (e, nom) in &self.noms {
                y.push_str(&format!("  {e}: {}\n", chaine(nom)));
            }
        }
        if !self.fusions.is_empty() {
            y.push_str("fusions:\n");
            for (e, dans) in &self.fusions {
                y.push_str(&format!("  {e}: {dans}\n"));
            }
        }
        if !self.propositions.is_empty() {
            y.push_str("propositions:\n");
            for p in &self.propositions {
                y.push_str(&format!("  {}:\n    nom: {}\n    justification: {}\n", p.locuteur, chaine(&p.nom), chaine(&p.justification)));
                if let Some(m) = &p.meme_voix_que {
                    y.push_str(&format!("    meme_voix_que: {m}\n"));
                }
            }
        }
        if !self.reperes.is_empty() {
            y.push_str("reperes:\n");
            for r in &self.reperes {
                y.push_str(&format!("  - instant: {}\n    libelle: {}\n", chaine(&instant(r.instant)), chaine(&r.libelle)));
            }
        }
        if self.sessions != Sessions::default() {
            y.push_str("sessions:\n");
            for (cle, s) in [("relecture", &self.sessions.relecture), ("bilan", &self.sessions.bilan)] {
                if let Some(s) = s {
                    y.push_str(&format!("  {cle}: {}\n", chaine(s)));
                }
            }
        }
        if self.nouvelle {
            y.push_str("nouvelle: true\n");
        }
        y.push_str("---\n\n");
        y.push_str(&format!("# {}\n\n", self.titre));
        if !self.resume.is_empty() {
            y.push_str(&format!("{}\n\n", self.resume.trim()));
        }
        if !self.actions_principales.is_empty() {
            y.push_str("## Actions principales\n\n");
            for a in &self.actions_principales {
                y.push_str(&format!("- {a}\n"));
            }
            y.push('\n');
        }
        y.push_str("## Fichiers\n\n");
        for (libelle, role) in [
            ("Bilan", Role::Bilan),
            ("Transcription", Role::TranscriptionLisible),
            ("Transcription structurée", Role::TranscriptionStructuree),
            ("Enregistrement", Role::Enregistrement),
        ] {
            y.push_str(&format!("- [{libelle}](<{nom_du_dossier} - {}>)\n", role.suffixe()));
        }
        y
    }

    /// Relit une fiche. Les clés inconnues sont ignorées ; il faut au moins un frontmatter.
    pub fn lire(md: &str) -> Result<Fiche, String> {
        let md = md.strip_prefix('\u{feff}').unwrap_or(md);
        let reste = md.strip_prefix("---\n").or_else(|| md.strip_prefix("---\r\n")).ok_or("la fiche n'a pas de frontmatter")?;
        let fin = reste.find("\n---").ok_or("le frontmatter de la fiche n'est pas fermé")?;
        let (yaml, corps) = (&reste[..fin + 1], &reste[fin + 4..]);
        let racine = yaml::lire(yaml)?;
        let texte = |cle: &str| racine.get(cle).and_then(yaml::Valeur::texte);

        let date = texte("date").ok_or("la fiche n'a pas de date")?;
        let (annee, mois, jour) = lire_date(date).ok_or_else(|| format!("date illisible : {date}"))?;
        let (heure, minute) = texte("debut").and_then(lire_heure).unwrap_or((0, 0));
        let etape = |cle: &str| texte(cle).and_then(lire_etape);
        let statut = match texte("statut").unwrap_or("a_traiter") {
            "enregistrement_en_cours" => Statut::EnregistrementEnCours,
            "enregistrement_interrompu" => Statut::EnregistrementInterrompu,
            "en_attente" => Statut::EnAttente(etape("etape").unwrap_or(Etape::Transcription)),
            "en_cours" => Statut::EnCours { etape: etape("etape").unwrap_or(Etape::Transcription), progres: 0.0 },
            "echec" => Statut::Echec { etape: etape("etape").unwrap_or(Etape::Transcription), message: texte("erreur").unwrap_or_default().to_string() },
            "terminee" => Statut::Terminee,
            _ => Statut::ATraiter,
        };
        let paires = |cle: &str| -> Vec<(String, String)> {
            racine.get(cle).map(|t| t.entrees().filter_map(|(k, v)| Some((k.to_string(), v.texte()?.to_string()))).collect()).unwrap_or_default()
        };
        let propositions = racine
            .get("propositions")
            .map(|t| {
                t.entrees()
                    .filter_map(|(k, v)| {
                        Some(Proposition {
                            locuteur: k.to_string(),
                            nom: v.get("nom")?.texte()?.to_string(),
                            justification: v.get("justification").and_then(yaml::Valeur::texte).unwrap_or_default().to_string(),
                            meme_voix_que: v.get("meme_voix_que").and_then(yaml::Valeur::texte).map(str::to_string),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        let reperes = racine
            .get("reperes")
            .map(|l| {
                l.elements()
                    .filter_map(|v| {
                        Some(Repere {
                            instant: lire_horodatage(v.get("instant")?.texte()?)?,
                            libelle: v.get("libelle").and_then(yaml::Valeur::texte).unwrap_or_default().to_string(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        let session = |cle: &str| racine.get("sessions").and_then(|s| s.get(cle)).and_then(yaml::Valeur::texte).map(str::to_string);
        let (resume, actions_principales) = lire_corps(corps);
        Ok(Fiche {
            titre: texte("titre").unwrap_or("Réunion").to_string(),
            debut: DateHeure::new(annee, mois, jour, heure, minute),
            duree: texte("duree").and_then(lire_horodatage).unwrap_or(0.0),
            statut,
            etape_faite: etape("etape_faite"),
            nombre_de_locuteurs: texte("nombre_de_locuteurs").and_then(|n| n.parse().ok()),
            noms: paires("locuteurs"),
            fusions: paires("fusions"),
            propositions,
            reperes,
            sessions: Sessions { relecture: session("relecture"), bilan: session("bilan") },
            nouvelle: texte("nouvelle") == Some("true"),
            resume,
            actions_principales,
        })
    }
}

/// Le résumé (entre le titre et la première section) et les actions principales.
fn lire_corps(corps: &str) -> (String, Vec<String>) {
    let mut resume = Vec::new();
    let mut actions = Vec::new();
    let mut section: Option<&str> = None;
    for l in corps.lines() {
        if let Some(h) = l.strip_prefix("## ") {
            section = Some(h.trim());
        } else if l.starts_with("# ") {
            continue;
        } else if section.is_none() {
            resume.push(l);
        } else if section == Some("Actions principales")
            && let Some(a) = l.trim_start().strip_prefix("- ")
        {
            actions.push(a.trim().to_string());
        }
    }
    (resume.join("\n").trim().to_string(), actions)
}

pub(crate) fn nom_d_etape(e: Etape) -> &'static str {
    match e {
        Etape::Transcription => "transcription",
        Etape::Relecture => "relecture",
        Etape::Bilan => "bilan",
    }
}

fn lire_etape(s: &str) -> Option<Etape> {
    Etape::TOUTES.into_iter().find(|e| nom_d_etape(*e) == s)
}

/// Une chaîne YAML entre guillemets doubles (les échappements de JSON en sont un sous-ensemble).
fn chaine(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}

fn lire_date(s: &str) -> Option<(i32, u8, u8)> {
    let mut it = s.trim().splitn(3, '-');
    Some((it.next()?.parse().ok()?, it.next()?.parse().ok()?, it.next()?.parse().ok()?))
}

fn lire_heure(s: &str) -> Option<(u8, u8)> {
    let (h, m) = s.trim().split_once([':', 'h'])?;
    Some((h.parse().ok()?, m.parse().ok()?))
}

/// « 0:12:34 », avec les centièmes s'il y en a (« 0:12:34.5 »).
fn instant(t: f64) -> String {
    let cs = (t.max(0.0) * 100.0).round() as u64;
    let base = horodatage_de_lecture((cs / 100) as f64);
    match cs % 100 {
        0 => base,
        c if c % 10 == 0 => format!("{base}.{}", c / 10),
        c => format!("{base}.{c:02}"),
    }
}

/// « 0:12:34 » ou « 12:34 » en secondes.
fn lire_horodatage(s: &str) -> Option<f64> {
    s.trim().split(':').try_fold(0.0, |acc, x| Some(acc * 60.0 + x.parse::<f64>().ok()?))
}

/// Le sous-ensemble de YAML de la fiche.
mod yaml {
    #[derive(Clone, Debug, PartialEq)]
    pub enum Valeur {
        Texte(String),
        Liste(Vec<Valeur>),
        Table(Vec<(String, Valeur)>),
    }

    impl Valeur {
        pub fn texte(&self) -> Option<&str> {
            match self {
                Valeur::Texte(s) => Some(s),
                _ => None,
            }
        }
        pub fn get(&self, cle: &str) -> Option<&Valeur> {
            self.entrees().find(|(k, _)| *k == cle).map(|(_, v)| v)
        }
        pub fn entrees(&self) -> impl Iterator<Item = (&str, &Valeur)> {
            let t: &[(String, Valeur)] = match self {
                Valeur::Table(t) => t,
                _ => &[],
            };
            t.iter().map(|(k, v)| (k.as_str(), v))
        }
        pub fn elements(&self) -> impl Iterator<Item = &Valeur> {
            let l: &[Valeur] = match self {
                Valeur::Liste(l) => l,
                _ => &[],
            };
            l.iter()
        }
    }

    pub fn lire(texte: &str) -> Result<Valeur, String> {
        let mut lignes: Vec<(usize, String)> = texte
            .lines()
            .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
            .map(|l| (l.len() - l.trim_start().len(), l.trim().to_string()))
            .collect();
        let mut i = 0;
        let v = bloc(&mut lignes, &mut i, 0);
        match lignes.get(i) {
            None => Ok(v),
            Some((_, l)) => Err(format!("frontmatter illisible à « {l} »")),
        }
    }

    fn est_un_tiret(l: &str) -> bool {
        l == "-" || l.starts_with("- ")
    }

    fn bloc(l: &mut Vec<(usize, String)>, i: &mut usize, retrait: usize) -> Valeur {
        if l.get(*i).is_some_and(|(_, x)| est_un_tiret(x)) { liste(l, i, retrait) } else { table(l, i, retrait) }
    }

    /// `cle: valeur` ou `cle:` ; renvoie la clé et le reste.
    fn cle(l: &str) -> Option<(&str, &str)> {
        if l.starts_with(['"', '\'']) {
            return None;
        }
        if let Some(k) = l.strip_suffix(':') {
            return Some((k.trim(), ""));
        }
        let (k, v) = l.split_once(": ")?;
        Some((k.trim(), v.trim()))
    }

    fn table(l: &mut Vec<(usize, String)>, i: &mut usize, retrait: usize) -> Valeur {
        let mut t = Vec::new();
        while let Some((r, ligne)) = l.get(*i).cloned() {
            if r != retrait || est_un_tiret(&ligne) {
                break;
            }
            let Some((k, v)) = cle(&ligne) else { break };
            *i += 1;
            let valeur = if !v.is_empty() {
                Valeur::Texte(scalaire(v))
            } else {
                match l.get(*i) {
                    Some((r2, x)) if *r2 > retrait || (*r2 == retrait && est_un_tiret(x)) => {
                        let r2 = *r2;
                        bloc(l, i, r2)
                    }
                    _ => Valeur::Texte(String::new()),
                }
            };
            t.push((k.to_string(), valeur));
        }
        Valeur::Table(t)
    }

    fn liste(l: &mut Vec<(usize, String)>, i: &mut usize, retrait: usize) -> Valeur {
        let mut v = Vec::new();
        while let Some((r, ligne)) = l.get(*i).cloned() {
            if r != retrait || !est_un_tiret(&ligne) {
                break;
            }
            let element = ligne[1..].trim().to_string();
            if element.is_empty() {
                *i += 1;
                match l.get(*i) {
                    Some((r2, _)) if *r2 > retrait => {
                        let r2 = *r2;
                        v.push(bloc(l, i, r2));
                    }
                    _ => v.push(Valeur::Texte(String::new())),
                }
            } else if cle(&element).is_some() {
                // `- cle: valeur` ouvre une table alignée sur la clé.
                l[*i] = (retrait + 2, element);
                v.push(table(l, i, retrait + 2));
            } else {
                *i += 1;
                v.push(Valeur::Texte(scalaire(&element)));
            }
        }
        Valeur::Liste(v)
    }

    fn scalaire(s: &str) -> String {
        let s = s.trim();
        if s.starts_with('"') {
            if let Ok(v) = serde_json::from_str::<String>(s) {
                return v;
            }
            // Un commentaire après la chaîne.
            if let Some(fin) = s[1..].find("\" #").or_else(|| s[1..].rfind('"')) {
                return serde_json::from_str::<String>(&s[..fin + 2]).unwrap_or_else(|_| s[1..fin + 1].to_string());
            }
        }
        if let Some(r) = s.strip_prefix('\'')
            && let Some(fin) = r.rfind('\'')
        {
            return r[..fin].replace("''", "'");
        }
        match s.find(" #") {
            Some(i) => s[..i].trim().to_string(),
            None => s.to_string(),
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn lit_tables_listes_et_chaines() {
            let v = lire("a: \"x: \\\"y\\\"\" # note\nb:\n  c: 1\n  d:\n    e: 'l''f'\nl:\n  - k: 1\n    m: 2\n  - 3\nn:\n- 4\n").unwrap();
            assert_eq!(v.get("a").and_then(Valeur::texte), Some("x: \"y\""));
            assert_eq!(v.get("b").and_then(|b| b.get("d")).and_then(|d| d.get("e")).and_then(Valeur::texte), Some("l'f"));
            let l: Vec<&Valeur> = v.get("l").unwrap().elements().collect();
            assert_eq!(l[0].get("m").and_then(Valeur::texte), Some("2"));
            assert_eq!(l[1].texte(), Some("3"));
            assert_eq!(v.get("n").unwrap().elements().count(), 1);
        }
    }
}
