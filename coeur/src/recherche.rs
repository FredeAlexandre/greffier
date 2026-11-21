//! La recherche ⌘K dans les titres, les bilans et les transcriptions de toutes les réunions.
//!
//! Le cœur tient un index : pour chaque dossier de réunion, son titre, les lignes de son bilan
//! (avec l'instant de leur sujet) et ses répliques (avec leur début et leur locuteur). Une entrée
//! est refaite quand les empreintes des fichiers de la réunion changent (écriture de Greffier ou
//! modification hors de l'app) ; elle disparaît avec la réunion (suppression, renommage).
//!
//! La recherche ignore la casse et les accents (« equipe » trouve « Équipe ») et l'apostrophe
//! typographique. Un résultat dit où ouvrir la réunion : la réplique, ou le sujet (la décision)
//! d'une ligne du bilan.
//!
//! **Le cache** (ADR 0001 : le seul état de l'app hors du dossier de réunions, reconstructible) :
//! un fichier JSON par réunion, `<cache>/<nom du dossier>.json`, avec les empreintes des fichiers
//! dont il vient. Au lancement, une entrée du cache sert si ses empreintes sont celles du
//! dossier ; sinon (fichiers changés app fermée, cache abîmé, absent, d'une autre version) elle
//! est refaite depuis la réunion. Ensuite, chaque entrée refaite est réécrite, chaque entrée
//! retirée est effacée.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::bilan::premier_horodatage;
use crate::dossier::surveillance::Empreintes;
use crate::reunion::{IdReunion, Reunion};
use crate::Greffier;

/// Où le mot a été trouvé.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Endroit {
    Titre,
    Bilan,
    Transcription,
}

/// Un passage trouvé.
#[derive(Clone, Debug, PartialEq)]
pub struct Resultat {
    pub reunion: IdReunion,
    pub endroit: Endroit,
    /// L'instant où ouvrir la réunion, en secondes : le début de la réplique, ou celui du sujet
    /// (de la décision) pour une ligne du bilan. Rien : le début de la réunion.
    pub instant: Option<f64>,
    /// Le locuteur de la réplique, tel qu'il s'affiche.
    pub locuteur: Option<String>,
    /// Le passage autour du mot trouvé (coupé par « … »).
    pub extrait: String,
    /// Le mot trouvé dans `extrait`, en octets.
    pub surligne: (usize, usize),
}

/// Au plus tant de lignes du bilan, de répliques par réunion, et de résultats en tout.
const LIGNES_DU_BILAN: usize = 3;
const REPLIQUES: usize = 4;
const RESULTATS: usize = 40;

/// La version du format du cache : une autre version est refaite.
const VERSION: u32 = 1;

/// Une ligne cherchable : le texte, sa version pliée (même nombre de caractères), son instant.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Ligne {
    texte: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    instant: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    locuteur: Option<String>,
    #[serde(skip)]
    plie: String,
}

impl Ligne {
    fn new(texte: String, instant: Option<f64>, locuteur: Option<String>) -> Ligne {
        let plie = plier(&texte);
        Ligne { texte, instant, locuteur, plie }
    }

    /// Le mot `requete` (déjà plié) dans la ligne : l'extrait et le passage surligné.
    fn trouver(&self, requete: &str) -> Option<(String, (usize, usize))> {
        let i = self.plie.find(requete)?;
        let debut = self.plie[..i].chars().count();
        let fin = debut + requete.chars().count();
        let octet = |n: usize| self.texte.char_indices().nth(n).map_or(self.texte.len(), |(o, _)| o);
        Some(extrait(&self.texte, octet(debut), octet(fin)))
    }
}

/// Ce que l'index retient d'une réunion (et le fichier du cache).
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Entree {
    version: u32,
    /// Les empreintes des fichiers quand l'entrée a été faite.
    empreintes: Empreintes,
    titre: Option<Ligne>,
    bilan: Vec<Ligne>,
    transcription: Vec<Ligne>,
}

impl Entree {
    fn de(r: &Reunion, empreintes: Empreintes) -> Entree {
        let titre = (!r.titre.trim().is_empty() && r.titre != "Réunion").then(|| Ligne::new(r.titre.clone(), None, None));
        let transcription = r
            .repliques
            .iter()
            .map(|u| {
                let locuteur = r.locuteurs.get(r.locuteur_affiche(u.locuteur)).map(|l| l.affiche());
                Ligne::new(u.texte.clone(), Some(u.debut), locuteur)
            })
            .collect();
        Entree { version: VERSION, empreintes, titre, bilan: lignes_du_bilan(&r.bilan), transcription }
    }

    /// L'entrée du cache, si elle est lisible et de cette version.
    fn lire(chemin: &Path) -> Option<Entree> {
        let mut e: Entree = serde_json::from_slice(&std::fs::read(chemin).ok()?).ok()?;
        if e.version != VERSION {
            return None;
        }
        for l in e.titre.iter_mut().chain(&mut e.bilan).chain(&mut e.transcription) {
            l.plie = plier(&l.texte);
        }
        Some(e)
    }
}

/// Les lignes du bilan, sans les titres de section. Une ligne prend son propre horodatage (une
/// décision), sinon celui du sujet `### Titre (h:mm:ss)` qui la contient.
fn lignes_du_bilan(md: &str) -> Vec<Ligne> {
    let mut sujet = None;
    let mut lignes = Vec::new();
    for ligne in md.lines() {
        let t = ligne.trim();
        if t.is_empty() || t == "---" {
            continue;
        }
        if t.starts_with("# ") || t.starts_with("## ") {
            sujet = None;
            continue;
        }
        if let Some(titre) = t.strip_prefix("### ") {
            sujet = premier_horodatage(titre);
        }
        let mut texte = t.trim_start_matches(['#', '-', '*', '>', '|', ' ']);
        for case in ["[ ] ", "[x] ", "[X] "] {
            texte = texte.strip_prefix(case).unwrap_or(texte);
        }
        let texte = texte.replace("**", "");
        if !texte.trim().is_empty() {
            lignes.push(Ligne::new(texte.trim().to_string(), premier_horodatage(t).or(sujet), None));
        }
    }
    lignes
}

/// Minuscules, sans accents, apostrophe droite : un caractère pour un caractère, pour retrouver
/// le passage dans le texte d'origine.
fn plier(s: &str) -> String {
    s.chars().map(plier_un).collect()
}

fn plier_un(c: char) -> char {
    let mut l = c.to_lowercase();
    let c = match (l.next(), l.next()) {
        (Some(x), None) => x,
        _ => c,
    };
    match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => 'a',
        'ç' => 'c',
        'è' | 'é' | 'ê' | 'ë' => 'e',
        'ì' | 'í' | 'î' | 'ï' => 'i',
        'ñ' => 'n',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' => 'o',
        'ù' | 'ú' | 'û' | 'ü' => 'u',
        'ý' | 'ÿ' => 'y',
        '’' | '‘' => '\'',
        _ => c,
    }
}

/// Un extrait de `s` autour de `[a, b)` (en octets), coupé entre deux mots, et la place du
/// passage dans l'extrait.
fn extrait(s: &str, a: usize, b: usize) -> (String, (usize, usize)) {
    let mut debut = a.saturating_sub(50);
    while !s.is_char_boundary(debut) {
        debut -= 1;
    }
    let mut fin = (b + 90).min(s.len());
    while !s.is_char_boundary(fin) {
        fin += 1;
    }
    // Couper entre deux mots plutôt qu'au milieu d'un mot.
    if debut > 0 && let Some(i) = s[debut..a].find(' ') {
        debut += i + 1;
    }
    if fin < s.len() && let Some(i) = s[b..fin].rfind(' ') {
        fin = b + i;
    }
    let avant = if debut > 0 { "…" } else { "" };
    let apres = if fin < s.len() { "…" } else { "" };
    let decalage = avant.len() + a - debut;
    (format!("{avant}{}{apres}", &s[debut..fin]), (decalage, decalage + (b - a)))
}

/// L'index de recherche, par nom de dossier de réunion.
#[derive(Default)]
pub(crate) struct Index {
    entrees: HashMap<String, Entree>,
    /// Le dossier du cache, s'il y en a un.
    cache: Option<PathBuf>,
}

impl Index {
    fn fichier(cache: &Path, nom: &str) -> PathBuf {
        cache.join(format!("{nom}.json"))
    }

    /// Écrit l'entrée dans le cache (sans erreur : le cache se refait).
    fn ecrire(&self, nom: &str, e: &Entree) {
        let Some(cache) = &self.cache else { return };
        let Ok(json) = serde_json::to_vec(e) else { return };
        let partiel = cache.join(format!(".{nom}.json.partiel"));
        if std::fs::create_dir_all(cache).is_ok() && std::fs::write(&partiel, json).is_ok() {
            let _ = std::fs::rename(&partiel, Index::fichier(cache, nom));
        }
    }

    fn effacer(&self, nom: &str) {
        if let Some(cache) = &self.cache {
            let _ = std::fs::remove_file(Index::fichier(cache, nom));
        }
    }

    /// Prend `cache` comme dossier du cache : l'index repart des entrées qui y sont à jour
    /// (`a_jour(nom, empreintes)`), les autres fichiers sont effacés.
    fn ouvrir(&mut self, cache: Option<PathBuf>, a_jour: impl Fn(&str, &Empreintes) -> bool) {
        self.entrees.clear();
        self.cache = cache;
        let Some(cache) = &self.cache else { return };
        for f in std::fs::read_dir(cache).into_iter().flatten().flatten() {
            let chemin = f.path();
            let nom = f.file_name().to_string_lossy().strip_suffix(".json").map(str::to_owned);
            match nom.and_then(|nom| Some((Entree::lire(&chemin).filter(|e| a_jour(&nom, &e.empreintes))?, nom))) {
                Some((e, nom)) => _ = self.entrees.insert(nom, e),
                // Périmé, illisible, ou une écriture interrompue (`.….json.partiel`).
                None => _ = std::fs::remove_file(&chemin),
            }
        }
    }
}

impl Greffier {
    /// Les passages qui contiennent `requete` (sans tenir compte de la casse ni des accents),
    /// des réunions les plus récentes aux plus anciennes : pour chacune, le titre, puis quelques
    /// lignes du bilan, puis quelques répliques.
    pub fn rechercher(&self, requete: &str) -> Vec<Resultat> {
        let q = plier(requete.trim());
        let mut resultats = Vec::new();
        if q.is_empty() {
            return resultats;
        }
        for r in &self.reunions {
            let Some(e) = self.index.entrees.get(&r.nom_du_dossier) else { continue };
            let titre = e.titre.as_ref().and_then(|l| Some((Endroit::Titre, (l, l.trouver(&q)?))));
            let bilan = e.bilan.iter().filter_map(|l| Some((l, l.trouver(&q)?))).take(LIGNES_DU_BILAN).map(|x| (Endroit::Bilan, x));
            let transcription = e.transcription.iter().filter_map(|l| Some((l, l.trouver(&q)?))).take(REPLIQUES).map(|x| (Endroit::Transcription, x));
            for (endroit, (ligne, (extrait, surligne))) in titre.into_iter().chain(bilan).chain(transcription) {
                resultats.push(Resultat { reunion: r.id, endroit, instant: ligne.instant, locuteur: ligne.locuteur.clone(), extrait, surligne });
            }
            if resultats.len() >= RESULTATS {
                break;
            }
        }
        resultats
    }

    /// Met l'index à jour : une entrée refaite pour chaque réunion dont les fichiers ont changé
    /// depuis, une entrée retirée pour chaque réunion qui n'existe plus.
    pub(crate) fn indexer(&mut self) {
        for r in &self.reunions {
            let empreintes = self.empreintes.get(&r.nom_du_dossier).cloned().unwrap_or_default();
            if self.index.entrees.get(&r.nom_du_dossier).is_some_and(|e| e.empreintes == empreintes) {
                continue;
            }
            let e = Entree::de(r, empreintes);
            self.index.ecrire(&r.nom_du_dossier, &e);
            self.index.entrees.insert(r.nom_du_dossier.clone(), e);
        }
        let noms: HashSet<&str> = self.reunions.iter().map(|r| r.nom_du_dossier.as_str()).collect();
        let partis: Vec<String> = self.index.entrees.keys().filter(|nom| !noms.contains(nom.as_str())).cloned().collect();
        for nom in partis {
            self.index.effacer(&nom);
            self.index.entrees.remove(&nom);
        }
    }

    /// Garde l'index dans ce dossier (ou en mémoire seulement) : voir le cache plus haut.
    pub(crate) fn utiliser_le_cache_de_recherche(&mut self, cache: Option<PathBuf>) {
        let attendues: HashMap<&str, Empreintes> = self.reunions.iter().map(|r| (r.nom_du_dossier.as_str(), self.empreintes.get(&r.nom_du_dossier).cloned().unwrap_or_default())).collect();
        self.index.ouvrir(cache, |nom, e| attendues.get(nom) == Some(e));
        self.indexer();
    }
}
