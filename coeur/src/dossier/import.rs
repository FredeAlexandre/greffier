//! L'import d'une réunion passée (glossaire : Import) : un enregistrement (vidéo ou audio
//! seul), avec ou sans sa transcription, avec ou sans son bilan.
//!
//! [`Import`] dit quels fichiers prendre, avec la date et le titre choisis dans la fenêtre
//! d'import ; `Greffier::proposer_import` le prépare à partir des fichiers déposés. Le cœur
//! copie ensuite les fichiers dans un nouveau dossier de réunion : les originaux ne sont jamais
//! touchés.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::transcription::{self, RepliqueEcrite, TranscriptionStructuree};
use super::{DossierDeReunions, Role, mp4};
use crate::reunion::{Etape, IdReunion, Reunion, Statut};
use crate::temps::DateHeure;

/// Ce qu'on importe, tel que validé dans la fenêtre d'import.
#[derive(Clone, Debug, PartialEq)]
pub struct Import {
    /// Le média : vidéo, ou audio seul.
    pub enregistrement: PathBuf,
    /// Fichier structuré de Greffier (`.json`), WebVTT (`.vtt`) ou SRT (`.srt`).
    pub transcription: Option<PathBuf>,
    /// N'importe quel Markdown, copié tel quel.
    pub bilan: Option<PathBuf>,
    pub debut: DateHeure,
    pub titre: String,
}

/// L'extension d'un fichier, en minuscules (« mp4 »).
pub(crate) fn extension(chemin: &Path) -> String {
    chemin.extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase()
}

/// Les extensions des médias qu'AVFoundation lit, vidéo ou audio seul.
pub const EXTENSIONS_DE_MEDIA: [&str; 17] = ["mp4", "m4v", "mov", "qt", "3gp", "3g2", "m4a", "m4b", "mp3", "wav", "aif", "aiff", "aifc", "caf", "aac", "flac", "amr"];

impl DossierDeReunions {
    /// Copie les fichiers de l'import dans un nouveau dossier de réunion et rend la réunion,
    /// pas encore écrite : la fiche et les transcriptions viennent ensuite avec [`DossierDeReunions::ecrire`].
    /// En cas d'erreur, rien ne reste dans le dossier de réunions.
    pub fn importer(&self, import: &Import, id: IdReunion) -> Result<Reunion, String> {
        let lire = |p: &Path| std::fs::read_to_string(p).map_err(|e| format!("{} : {e}", p.display()));
        if !EXTENSIONS_DE_MEDIA.contains(&extension(&import.enregistrement).as_str()) {
            return Err(format!("{} : ce n'est pas un média que Greffier sait lire.", import.enregistrement.display()));
        }
        if !import.enregistrement.is_file() {
            return Err(format!("{} : fichier introuvable.", import.enregistrement.display()));
        }
        let titre = match import.titre.trim() {
            "" => "Réunion",
            t => t,
        };
        let nom = self.nom_libre(import.debut, titre);
        let dossier = self.dossier(&nom);
        std::fs::create_dir_all(&dossier).map_err(|e| format!("{} : {e}", dossier.display()))?;
        let copier = |de: &Path, vers: PathBuf| std::fs::copy(de, &vers).map(|_| ()).map_err(|e| format!("{} : {e}", de.display()));
        let resultat = (|| {
            let mut r = Reunion::new(id, nom.clone(), import.debut, 0.0);
            r.titre = titre.to_string();
            r.statut = Statut::ATraiter;
            if let Some(t) = &import.transcription {
                appliquer_la_transcription(t, &mut r)?;
                r.etape_faite = Some(Etape::Transcription);
            }
            if let Some(b) = &import.bilan {
                let md = lire(b)?;
                copier(b, self.fichier(&nom, Role::Bilan))?;
                r.resume = crate::bilan::en_bref(&md);
                r.poser_bilan(md);
                // Avec sa transcription, la réunion est complète ; sans, le traitement refera tout.
                if r.etape_faite.is_some() {
                    r.etape_faite = Some(Etape::Bilan);
                    r.statut = Statut::Terminee;
                }
            }
            copier(&import.enregistrement, self.fichier_d_enregistrement(&nom, &extension(&import.enregistrement)))?;
            r.duree = self.enregistrement(&nom).and_then(|f| mp4::duree(&f)).or_else(|| r.repliques.iter().map(|x| x.fin).reduce(f64::max)).unwrap_or(0.0);
            Ok(r)
        })();
        if resultat.is_err() {
            let _ = std::fs::remove_dir_all(&dossier);
        }
        resultat
    }
}

// --- Transcriptions ------------------------------------------------------------------------

/// Une réplique lue dans un fichier de sous-titres.
#[derive(Debug)]
struct Ligne {
    debut: f64,
    fin: f64,
    locuteur: Option<String>,
    texte: String,
}

/// Lit la transcription importée dans la réunion : fichier structuré de Greffier, WebVTT ou SRT.
/// Elle est réputée non relue. Les locuteurs nommés (« Alexandre ») deviennent SPEAKER_00,
/// SPEAKER_01… dans l'ordre où ils parlent, avec leur nom ; sans locuteur, tout va à SPEAKER_00.
fn appliquer_la_transcription(chemin: &Path, r: &mut Reunion) -> Result<(), String> {
    let texte = std::fs::read_to_string(chemin).map_err(|e| format!("{} : {e}", chemin.display()))?;
    let (transcription, noms) = match extension(chemin).as_str() {
        "json" => {
            let mut t = TranscriptionStructuree::lire(&texte).map_err(|e| format!("{} : {e}", chemin.display()))?;
            t.relecture = None;
            t.brute.clear();
            (t, vec![])
        }
        "vtt" | "srt" => depuis_les_lignes(lire_sous_titres(&texte)),
        _ => return Err(format!("{} : la transcription doit être un fichier de Greffier (.json), WebVTT (.vtt) ou SRT (.srt).", chemin.display())),
    };
    if transcription.repliques.is_empty() {
        return Err(format!("{} : aucune réplique lue.", chemin.display()));
    }
    transcription.appliquer(r);
    for l in &mut r.locuteurs {
        l.nom = noms.iter().find(|(e, _)| *e == l.etiquette).map(|(_, n)| n.clone());
    }
    Ok(())
}

/// Les répliques en transcription structurée, et le nom de chaque étiquette donnée à un locuteur nommé.
fn depuis_les_lignes(lignes: Vec<Ligne>) -> (TranscriptionStructuree, Vec<(String, String)>) {
    let est_une_etiquette = |s: &str| s.strip_prefix("SPEAKER_").is_some_and(|n| !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit()));
    let mut prises: Vec<String> = lignes.iter().filter_map(|l| l.locuteur.clone()).filter(|l| est_une_etiquette(l)).collect();
    let mut noms: Vec<(String, String)> = Vec::new();
    let mut n = 0;
    for nom in lignes.iter().filter_map(|l| l.locuteur.as_deref()).filter(|l| !est_une_etiquette(l)) {
        if noms.iter().any(|(_, x)| x == nom) {
            continue;
        }
        let etiquette = loop {
            let e = format!("SPEAKER_{n:02}");
            n += 1;
            if !prises.contains(&e) {
                break e;
            }
        };
        prises.push(etiquette.clone());
        noms.push((etiquette, nom.to_string()));
    }
    let etiquette = |l: &Option<String>| match l {
        Some(x) if est_une_etiquette(x) => x.clone(),
        Some(x) => noms.iter().find(|(_, n)| n == x).map(|(e, _)| e.clone()).unwrap_or_default(),
        None => "SPEAKER_00".into(),
    };
    let repliques: Vec<RepliqueEcrite> = lignes.iter().map(|l| RepliqueEcrite { locuteur: etiquette(&l.locuteur), debut: l.debut, fin: l.fin, texte: l.texte.clone() }).collect();
    let mut locuteurs: Vec<String> = repliques.iter().map(|x| x.locuteur.clone()).collect();
    locuteurs.sort();
    locuteurs.dedup();
    let t = TranscriptionStructuree {
        version: transcription::VERSION,
        locuteurs,
        mots: vec![],
        tours: vec![],
        repliques,
        brute: vec![],
        relecture: None,
        passages: vec![],
        corrections_a_la_main: vec![],
    };
    (t, noms)
}

/// Les répliques d'un WebVTT ou d'un SRT. Le locuteur est lu en tête de réplique : `<v Nom>`,
/// `[Nom]:` ou `Nom :`. Cette dernière forme n'est retenue que si au moins la moitié des
/// répliques la portent (sinon « Bref : on y va » serait pris pour un locuteur). Une réplique sans
/// locuteur dans un fichier qui en a garde celui de la précédente.
fn lire_sous_titres(texte: &str) -> Vec<Ligne> {
    let texte = texte.trim_start_matches('\u{feff}').replace("\r\n", "\n").replace('\r', "\n");
    let mut lignes = Vec::new();
    // Pour chaque réplique sans locuteur sûr : (nom, reste) si elle commence par « Nom : ».
    let mut candidats: Vec<Option<(String, String)>> = Vec::new();
    let mut it = texte.lines().peekable();
    while let Some(l) = it.next() {
        let Some((a, b)) = l.split_once("-->") else { continue };
        let (Some(debut), Some(fin)) = (horodatage(a.trim()), b.split_whitespace().next().and_then(horodatage)) else { continue };
        let mut corps = Vec::new();
        while let Some(l) = it.next_if(|l| !l.trim().is_empty()) {
            corps.push(l.trim());
        }
        let corps = corps.join(" ");
        let voix = corps.strip_prefix("<v").filter(|r| r.starts_with([' ', '.'])).and_then(|r| r.split_once('>')).and_then(|(balise, _)| balise.split_once(' ')).map(|(_, nom)| nom.trim().to_string()).filter(|n| !n.is_empty());
        let texte = sans_balises(&corps);
        let (locuteur, texte) = match voix {
            Some(v) => (Some(v), texte),
            None => match entre_crochets(&texte) {
                Some((nom, reste)) => (Some(nom), reste),
                None => {
                    candidats.push(nom_et_deux_points(&texte));
                    (None, texte)
                }
            },
        };
        lignes.push(Ligne { debut, fin, locuteur, texte });
    }
    let sans = lignes.iter().filter(|l| l.locuteur.is_none()).count();
    if sans > 0 && candidats.iter().filter(|c| c.is_some()).count() * 2 >= sans {
        let mut c = candidats.into_iter();
        for l in lignes.iter_mut().filter(|l| l.locuteur.is_none()) {
            if let Some(Some((nom, reste))) = c.next() {
                l.locuteur = Some(nom);
                l.texte = reste;
            }
        }
    }
    let mut precedent = None;
    for l in &mut lignes {
        match &l.locuteur {
            Some(x) => precedent = Some(x.clone()),
            None => l.locuteur = precedent.clone(),
        }
    }
    lignes.retain(|l| !l.texte.is_empty());
    lignes
}

/// `hh:mm:ss.mmm`, `mm:ss.mmm` (WebVTT) ou `hh:mm:ss,mmm` (SRT), en secondes.
fn horodatage(s: &str) -> Option<f64> {
    let s = s.replace(',', ".");
    let parties: Vec<&str> = s.split(':').collect();
    let nombre = |x: &str| -> Option<f64> { x.bytes().all(|c| c.is_ascii_digit() || c == b'.').then(|| x.parse().ok()).flatten() };
    match parties[..] {
        [m, s] => Some(nombre(m)? * 60.0 + nombre(s)?),
        [h, m, s] => Some(nombre(h)? * 3600.0 + nombre(m)? * 60.0 + nombre(s)?),
        _ => None,
    }
}

/// Le texte sans balises (`<i>`, `<v Nom>`, `<00:00:01.000>`…) ni entités, les espaces resserrés.
fn sans_balises(s: &str) -> String {
    let mut out = String::new();
    let mut dans = false;
    for c in s.chars() {
        match c {
            '<' => dans = true,
            '>' if dans => dans = false,
            c if !dans => out.push(c),
            _ => {}
        }
    }
    let out = out.replace("&lt;", "<").replace("&gt;", ">").replace("&nbsp;", " ").replace("&lrm;", "").replace("&rlm;", "").replace("&amp;", "&");
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `[Nom]: texte` ou `[Nom] : texte`.
fn entre_crochets(s: &str) -> Option<(String, String)> {
    let (nom, reste) = s.strip_prefix('[')?.split_once(']')?;
    let reste = reste.trim_start().strip_prefix(':')?;
    let nom = nom.trim();
    (!nom.is_empty()).then(|| (nom.to_string(), reste.trim().to_string()))
}

/// `Nom : texte` ou `Nom: texte` : un nom court (quatre mots au plus) qui commence par une
/// majuscule, suivi de deux-points puis d'une espace (« 10:30 » n'en est pas un).
fn nom_et_deux_points(s: &str) -> Option<(String, String)> {
    let (nom, reste) = s.split_once(':')?;
    if !reste.is_empty() && !reste.starts_with(char::is_whitespace) {
        return None;
    }
    let nom = nom.trim();
    let permis = |c: char| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.' | '\'' | '’');
    let court = nom.chars().count() <= 40 && nom.split_whitespace().count() <= 4;
    (nom.starts_with(char::is_uppercase) && court && nom.chars().all(permis)).then(|| (nom.to_string(), reste.trim().to_string()))
}

// --- La proposition de la fenêtre d'import ---------------------------------------------------

/// Ce que la fenêtre d'import propose pour ces fichiers déposés : le média, la transcription et
/// le bilan (au plus un de chaque, rangés d'après leur extension), la date (celle de création
/// inscrite dans le média, sinon celle de modification du fichier) et le titre (celui du bilan,
/// sinon le nom du média).
pub(crate) fn proposer(chemins: &[PathBuf], en_heure_locale: impl Fn(SystemTime) -> DateHeure) -> Result<Import, String> {
    let (mut medias, mut transcriptions, mut bilans) = (vec![], vec![], vec![]);
    for c in chemins {
        let e = extension(c);
        match e.as_str() {
            e if EXTENSIONS_DE_MEDIA.contains(&e) => medias.push(c.clone()),
            "json" | "vtt" | "srt" => transcriptions.push(c.clone()),
            "md" | "markdown" => bilans.push(c.clone()),
            _ => return Err(format!("« {} » : Greffier importe un média (vidéo ou audio), une transcription (.vtt, .srt, .json) et un bilan (.md).", nom_du_fichier(c))),
        }
    }
    let un = |v: Vec<PathBuf>, quoi: &str| match v.len() {
        0 | 1 => Ok(v.into_iter().next()),
        _ => Err(format!("Un seul fichier de {quoi} par réunion.")),
    };
    let enregistrement = un(medias, "média")?.ok_or("Il faut un média (vidéo ou audio) pour importer une réunion.")?;
    let transcription = un(transcriptions, "transcription")?;
    let bilan = un(bilans, "bilan")?;
    let instant = mp4::date_de_creation(&enregistrement).or_else(|| std::fs::metadata(&enregistrement).and_then(|m| m.modified()).ok());
    let debut = instant.map(en_heure_locale).ok_or_else(|| format!("{} : fichier introuvable.", enregistrement.display()))?;
    let titre = bilan.as_ref().and_then(|b| std::fs::read_to_string(b).ok()).and_then(|md| titre_du_bilan(&md));
    let titre = titre.unwrap_or_else(|| enregistrement.file_stem().map(|s| s.to_string_lossy().trim().to_string()).unwrap_or_default());
    Ok(Import { enregistrement, transcription, bilan, debut, titre })
}

fn nom_du_fichier(c: &Path) -> String {
    c.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Le titre d'un bilan : `titre:` ou `title:` de son en-tête, sinon son premier `# Titre`, sans
/// un « Bilan — » de tête.
fn titre_du_bilan(md: &str) -> Option<String> {
    let md = md.trim_start_matches('\u{feff}');
    let mut corps = md;
    if let Some(reste) = md.strip_prefix("---\n").or_else(|| md.strip_prefix("---\r\n")) {
        let (entete, apres) = reste.split_once("\n---").unwrap_or((reste, ""));
        corps = apres;
        for l in entete.lines() {
            if let Some(v) = l.strip_prefix("titre:").or_else(|| l.strip_prefix("title:")) {
                let v = v.trim().trim_matches(['"', '\'']).trim();
                if !v.is_empty() {
                    return Some(v.to_string());
                }
            }
        }
    }
    let h1 = corps.lines().find_map(|l| l.strip_prefix("# "))?.trim();
    let titre = match h1.strip_prefix("Bilan") {
        Some(r) if r.starts_with([' ', ':']) => r.trim_start_matches([' ', '—', '–', '-', ':']).trim(),
        _ => h1,
    };
    (!titre.is_empty()).then(|| titre.to_string())
}
