//! La transcription structurée (`… - transcription.json`) et la transcription lisible
//! (`… - transcription.md`).
//!
//! Le JSON garde tout ce que le traitement a produit : les mots (horodatages, confiance,
//! locuteur), les tours de parole, les répliques, la transcription brute d'avant relecture, le
//! journal de la relecture, les passages à vérifier et les corrections faites à la main. Les
//! locuteurs y sont toujours désignés par leur étiquette (« SPEAKER_00 »), même nommés ou
//! fusionnés ; `noms` et `fusions` donnent la correspondance, la même que dans la fiche (qui fait
//! foi quand elle existe).
//!
//! ```json
//! {
//!   "version": 1,
//!   "locuteurs": ["SPEAKER_00", "SPEAKER_01", "SPEAKER_02"],
//!   "noms": { "SPEAKER_00": "Alexandre", "SPEAKER_01": "Khalid", "SPEAKER_02": "Alexandre" },
//!   "fusions": { "SPEAKER_02": "SPEAKER_00" },
//!   "mots": [{ "debut": 0.0, "fin": 0.25, "texte": "Bonjour", "confiance": 0.9, "locuteur": "SPEAKER_00" }],
//!   "tours": [{ "debut": 0.0, "fin": 2.0, "locuteur": "SPEAKER_00" }],
//!   "repliques": [{ "locuteur": "SPEAKER_00", "debut": 0.0, "fin": 2.0, "texte": "Bonjour Khalid." }],
//!   "brute": [ …les répliques d'avant la relecture, absentes si elle n'est pas faite… ],
//!   "relecture": { "corrections": [{ "replique": 1, "avant": "recète", "apres": "recette" }],
//!                  "changements_de_locuteur": [{ "replique": 3, "locuteur": "SPEAKER_01" }],
//!                  "passages_corriges": [{ "replique": 4, "extrait": "la prod", "apres": "la preprod",
//!                                          "reecoute": "on bascule la preprod" }],
//!                  "ajouts_au_lexique": [{ "entendu": "recète", "correct": "recette" }] },
//!   "passages": [{ "replique": 2, "extrait": "le support", "hypothese": "l'équipe support",
//!                  "reecoute": "…", "statut": "en_attente" }],
//!   "corrections_a_la_main": [{ "replique": 0, "avant": "…", "apres": "…" }]
//! }
//! ```
//!
//! `statut` d'un passage : `en_attente`, `hypothese_acceptee`, `original_garde` ou
//! `corrige_a_la_main` (avec `correction`). `relecture` absente : la relecture n'est pas faite
//! (une transcription importée est réputée non relue).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::adaptateurs::{AjoutAuLexique, ChangementDeLocuteur, CorrectionSure, PassageCorrige};
use crate::reunion::{CorrectionALaMain, JournalDeRelecture, Locuteur, Mot, PassageAVerifier, Replique, Reunion, StatutDuPassage, TEINTES, TourDeParole};
use crate::temps::horodatage_de_lecture;

pub const VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TranscriptionStructuree {
    pub version: u32,
    /// Les étiquettes des locuteurs, dans l'ordre (leurs couleurs en dépendent).
    #[serde(default)]
    pub locuteurs: Vec<String>,
    /// SPEAKER_xx → nom donné par l'utilisateur, fusions comprises.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub noms: BTreeMap<String, String>,
    /// SPEAKER_xx → SPEAKER_yy : le premier est la même voix que le second, qui le remplace.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fusions: BTreeMap<String, String>,
    #[serde(default)]
    pub mots: Vec<MotEcrit>,
    #[serde(default)]
    pub tours: Vec<TourEcrit>,
    #[serde(default)]
    pub repliques: Vec<RepliqueEcrite>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub brute: Vec<RepliqueEcrite>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relecture: Option<RelectureEcrite>,
    #[serde(default)]
    pub passages: Vec<PassageEcrit>,
    #[serde(default)]
    pub corrections_a_la_main: Vec<CorrectionEcrite>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MotEcrit {
    pub debut: f64,
    pub fin: f64,
    pub texte: String,
    #[serde(default = "confiance_pleine")]
    pub confiance: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locuteur: Option<String>,
}

fn confiance_pleine() -> f32 {
    1.0
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TourEcrit {
    pub debut: f64,
    pub fin: f64,
    pub locuteur: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RepliqueEcrite {
    pub locuteur: String,
    pub debut: f64,
    pub fin: f64,
    pub texte: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CorrectionEcrite {
    pub replique: usize,
    pub avant: String,
    pub apres: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChangementEcrit {
    pub replique: usize,
    pub locuteur: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RelectureEcrite {
    #[serde(default)]
    pub corrections: Vec<CorrectionEcrite>,
    #[serde(default)]
    pub changements_de_locuteur: Vec<ChangementEcrit>,
    /// Les passages douteux corrigés après la réécoute.
    #[serde(default)]
    pub passages_corriges: Vec<PassageCorrigeEcrit>,
    /// Les erreurs écrites dans le lexique.
    #[serde(default)]
    pub ajouts_au_lexique: Vec<AjoutEcrit>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PassageCorrigeEcrit {
    pub replique: usize,
    pub extrait: String,
    pub apres: String,
    #[serde(default)]
    pub reecoute: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AjoutEcrit {
    pub entendu: String,
    pub correct: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PassageEcrit {
    pub replique: usize,
    pub extrait: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hypothese: Option<String>,
    #[serde(default)]
    pub reecoute: String,
    #[serde(default = "en_attente")]
    pub statut: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correction: Option<String>,
}

fn en_attente() -> String {
    "en_attente".into()
}

impl TranscriptionStructuree {
    pub fn lire(json: &str) -> Result<TranscriptionStructuree, String> {
        serde_json::from_str(json).map_err(|e| format!("transcription structurée illisible : {e}"))
    }

    pub fn json(&self) -> String {
        let mut s = serde_json::to_string_pretty(self).unwrap_or_default();
        s.push('\n');
        s
    }

    pub fn depuis(r: &Reunion) -> TranscriptionStructuree {
        let etiquette = |i: usize| r.locuteurs.get(i).map(|l| l.etiquette.clone()).unwrap_or_else(|| "SPEAKER_00".into());
        let ecrire = |x: &Replique| RepliqueEcrite { locuteur: etiquette(x.locuteur), debut: x.debut, fin: x.fin, texte: x.texte.clone() };
        // Le locuteur d'un mot : celui que l'attribution lui a donné, sinon (mots relus d'une
        // transcription qui n'en avait pas) celui de la réplique où il commence.
        let locuteur_du_mot = |m: &Mot| match m.locuteur {
            Some(i) => Some(etiquette(i)),
            None => r.repliques.iter().find(|x| m.debut >= x.debut - 1e-6 && m.debut <= x.fin + 1e-6).map(|x| etiquette(x.locuteur)),
        };
        let correction = |c: &CorrectionSure| CorrectionEcrite { replique: c.replique, avant: c.avant.clone(), apres: c.apres.clone() };
        TranscriptionStructuree {
            version: VERSION,
            locuteurs: r.locuteurs.iter().map(|l| l.etiquette.clone()).collect(),
            noms: (0..r.locuteurs.len()).filter_map(|i| Some((etiquette(i), r.nom_du_locuteur(i)?.to_string()))).collect(),
            fusions: r.locuteurs.iter().filter_map(|l| Some((l.etiquette.clone(), etiquette(l.fusionne_dans?)))).collect(),
            mots: r.mots.iter().map(|m| MotEcrit { debut: m.debut, fin: m.fin, texte: m.texte.clone(), confiance: m.confiance, locuteur: locuteur_du_mot(m) }).collect(),
            tours: r.tours.iter().map(|t| TourEcrit { debut: t.debut, fin: t.fin, locuteur: t.locuteur.clone() }).collect(),
            repliques: r.repliques.iter().map(ecrire).collect(),
            brute: r.brute.iter().map(ecrire).collect(),
            relecture: r.relecture.as_ref().map(|j| RelectureEcrite {
                corrections: j.corrections.iter().map(correction).collect(),
                changements_de_locuteur: j.changements_de_locuteur.iter().map(|c| ChangementEcrit { replique: c.replique, locuteur: c.locuteur.clone() }).collect(),
                passages_corriges: j.passages_corriges.iter().map(|p| PassageCorrigeEcrit { replique: p.replique, extrait: p.extrait.clone(), apres: p.apres.clone(), reecoute: p.reecoute.clone() }).collect(),
                ajouts_au_lexique: j.ajouts_au_lexique.iter().map(|a| AjoutEcrit { entendu: a.entendu.clone(), correct: a.correct.clone() }).collect(),
            }),
            passages: r
                .passages
                .iter()
                .map(|p| {
                    let (statut, correction) = match &p.statut {
                        StatutDuPassage::EnAttente => ("en_attente", None),
                        StatutDuPassage::HypotheseAcceptee => ("hypothese_acceptee", None),
                        StatutDuPassage::OriginalGarde => ("original_garde", None),
                        StatutDuPassage::CorrigeALaMain(t) => ("corrige_a_la_main", Some(t.clone())),
                    };
                    PassageEcrit { replique: p.replique, extrait: p.extrait.clone(), hypothese: p.hypothese.clone(), reecoute: p.reecoute.clone(), statut: statut.into(), correction }
                })
                .collect(),
            corrections_a_la_main: r.corrections_a_la_main.iter().map(|c| CorrectionEcrite { replique: c.replique, avant: c.avant.clone(), apres: c.apres.clone() }).collect(),
        }
    }

    /// Remplace la transcription de la réunion (locuteurs, mots, répliques, relecture,
    /// passages). Sans locuteur nulle part, tout va à un seul locuteur, SPEAKER_00.
    pub fn appliquer(self, r: &mut Reunion) {
        let mut etiquettes = self.locuteurs;
        let autres = self.repliques.iter().chain(&self.brute).map(|x| &x.locuteur).chain(self.tours.iter().map(|t| &t.locuteur)).chain(self.mots.iter().filter_map(|m| m.locuteur.as_ref()));
        let mut nouvelles: Vec<String> = autres.filter(|e| !etiquettes.contains(e)).cloned().collect();
        nouvelles.sort();
        nouvelles.dedup();
        etiquettes.extend(nouvelles);
        if etiquettes.is_empty() && !self.repliques.is_empty() {
            etiquettes.push("SPEAKER_00".into());
        }
        let index = |e: &str| etiquettes.iter().position(|x| x == e).unwrap_or(0);
        let lire = |x: &RepliqueEcrite| Replique { locuteur: index(&x.locuteur), debut: x.debut, fin: x.fin, texte: x.texte.clone(), passage: None };
        r.locuteurs = etiquettes.iter().enumerate().map(|(i, e)| Locuteur::new(e.clone(), TEINTES[i % TEINTES.len()])).collect();
        for (e, nom) in self.noms {
            if let Some(l) = r.locuteurs.iter_mut().find(|l| l.etiquette == e) {
                l.nom = Some(nom).filter(|n| !n.trim().is_empty());
            }
        }
        for (e, dans) in &self.fusions {
            if let (Some(i), Some(j)) = (etiquettes.iter().position(|x| x == e), etiquettes.iter().position(|x| x == dans)) {
                r.relire_fusion(i, j);
            }
        }
        r.oublier_les_noms_des_fusionnes();
        r.mots = self.mots.into_iter().map(|m| Mot { locuteur: m.locuteur.as_deref().map(index), debut: m.debut, fin: m.fin, texte: m.texte, confiance: m.confiance }).collect();
        r.tours = self.tours.into_iter().map(|t| TourDeParole { debut: t.debut, fin: t.fin, locuteur: t.locuteur }).collect();
        r.repliques = self.repliques.iter().map(lire).collect();
        r.brute = self.brute.iter().map(lire).collect();
        r.relecture = self.relecture.map(|j| JournalDeRelecture {
            corrections: j.corrections.into_iter().map(|c| CorrectionSure { replique: c.replique, avant: c.avant, apres: c.apres }).collect(),
            changements_de_locuteur: j.changements_de_locuteur.into_iter().map(|c| ChangementDeLocuteur { replique: c.replique, locuteur: c.locuteur }).collect(),
            passages_corriges: j.passages_corriges.into_iter().map(|p| PassageCorrige { replique: p.replique, extrait: p.extrait, apres: p.apres, reecoute: p.reecoute }).collect(),
            ajouts_au_lexique: j.ajouts_au_lexique.into_iter().map(|a| AjoutAuLexique { entendu: a.entendu, correct: a.correct }).collect(),
        });
        r.corrections = r.relecture.as_ref().map_or(0, |j| (j.corrections.len() + j.passages_corriges.len()) as u32);
        r.passages.clear();
        for p in self.passages {
            let Some(x) = r.repliques.get_mut(p.replique) else { continue };
            if x.passage.is_some() {
                continue;
            }
            x.passage = Some(r.passages.len());
            let statut = match (p.statut.as_str(), p.correction) {
                ("hypothese_acceptee", _) if p.hypothese.is_some() => StatutDuPassage::HypotheseAcceptee,
                ("original_garde", _) => StatutDuPassage::OriginalGarde,
                ("corrige_a_la_main", Some(t)) => StatutDuPassage::CorrigeALaMain(t),
                _ => StatutDuPassage::EnAttente,
            };
            r.passages.push(PassageAVerifier { replique: p.replique, extrait: p.extrait, hypothese: p.hypothese, reecoute: p.reecoute, statut });
        }
        r.corrections_a_la_main = self.corrections_a_la_main.into_iter().map(|c| CorrectionALaMain { replique: c.replique, avant: c.avant, apres: c.apres }).collect();
    }
}

/// La transcription lisible : une ligne par réplique, `[h:mm:ss] Nom : texte`, avec le nom du
/// locuteur quand il est connu (fusions suivies), et les passages à vérifier en citation dessous.
pub fn transcription_lisible(r: &Reunion) -> String {
    let mut s = format!("# {} — transcription\n\nRéunion du {:04}-{:02}-{:02} à {}, {}.\n", r.titre, r.debut.annee, r.debut.mois, r.debut.jour, r.debut.heure_courte(), horodatage_de_lecture(r.duree.round()));
    for x in &r.repliques {
        let nom = r.nom_du_locuteur(x.locuteur).unwrap_or(&r.locuteurs[r.locuteur_affiche(x.locuteur)].etiquette);
        s.push_str(&format!("\n[{}] {nom} : {}\n", horodatage_de_lecture(x.debut), x.texte));
        if let Some(p) = x.passage.and_then(|i| r.passages.get(i)).filter(|p| p.statut == StatutDuPassage::EnAttente) {
            match &p.hypothese {
                Some(h) => s.push_str(&format!("> À vérifier : « {} » (hypothèse : « {h} »)\n", p.extrait)),
                None => s.push_str(&format!("> À vérifier : « {} »\n", p.extrait)),
            }
        }
    }
    s
}
