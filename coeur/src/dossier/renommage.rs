//! Le renommage d'une réunion quand son titre change : le dossier et chacun de ses fichiers
//! (`<nom> - <rôle>`) prennent le nouveau nom, et les liens internes des fichiers Markdown qui
//! portaient l'ancien nom sont réécrits.

use super::{DossierDeReunions, nom_de_dossier};
use crate::temps::DateHeure;

impl DossierDeReunions {
    /// Renomme le dossier `ancien` et ses fichiers d'après le nouveau titre ; rend le nouveau
    /// nom (l'ancien si le titre donne le même nom). Les caractères interdits deviennent des
    /// tirets, et un nom déjà pris reçoit « (2) », « (3) »…
    ///
    /// Les fichiers sont renommés un à un puis le dossier ; au premier échec, ce qui a été fait
    /// est défait et rien n'a changé. Les fichiers Markdown (bilan, transcription lisible) qui
    /// nomment l'ancien `<nom> - …` sont ensuite réécrits avec le nouveau nom ; la fiche, elle,
    /// est réécrite par le cœur.
    pub fn renommer(&self, ancien: &str, debut: DateHeure, titre: &str) -> Result<String, String> {
        let base = nom_de_dossier(debut, titre);
        if ancien == base || suffixe_de_collision(ancien, &base) {
            return Ok(ancien.to_string());
        }
        let source = self.dossier(ancien);
        if !source.is_dir() {
            return Err(format!("{} : le dossier de la réunion n'existe plus", source.display()));
        }
        // Le disque du Mac ignore la casse : « réunion » → « Réunion » est le même dossier.
        let libre = |nom: &str| !self.dossier(nom).exists() || nom.to_lowercase() == ancien.to_lowercase();
        let mut nouveau = base.clone();
        let mut n = 2;
        while !libre(&nouveau) {
            nouveau = format!("{base} ({n})");
            n += 1;
        }

        let prefixe = format!("{ancien} - ");
        let entrees = std::fs::read_dir(&source).map_err(|e| format!("{} : {e}", source.display()))?;
        let a_renommer: Vec<String> = entrees.flatten().filter_map(|e| e.file_name().into_string().ok()).filter(|f| f.starts_with(&prefixe)).collect();
        let mut faits: Vec<(String, String)> = Vec::new();
        let defaire = |faits: &[(String, String)]| {
            for (avant, apres) in faits.iter().rev() {
                let _ = std::fs::rename(source.join(apres), source.join(avant));
            }
        };
        for fichier in a_renommer {
            let cible = format!("{nouveau} - {}", &fichier[prefixe.len()..]);
            if let Err(e) = std::fs::rename(source.join(&fichier), source.join(&cible)) {
                defaire(&faits);
                return Err(format!("{fichier} : {e}"));
            }
            faits.push((fichier, cible));
        }
        if let Err(e) = std::fs::rename(&source, self.dossier(&nouveau)) {
            defaire(&faits);
            return Err(format!("{} : {e}", source.display()));
        }

        // Les liens vers les fichiers renommés (`[[<nom> - réunion]]`, `<nom> - transcription.md`).
        let nouveau_prefixe = format!("{nouveau} - ");
        for (_, fichier) in &faits {
            let chemin = self.dossier(&nouveau).join(fichier);
            if !fichier.ends_with(".md") {
                continue;
            }
            if let Ok(md) = std::fs::read_to_string(&chemin)
                && md.contains(&prefixe)
            {
                let _ = super::ecrire_atomiquement(&chemin, &md.replace(&prefixe, &nouveau_prefixe));
            }
        }
        Ok(nouveau)
    }
}

/// `nom` est `base (n)` : le même titre, avec le numéro d'une collision.
fn suffixe_de_collision(nom: &str, base: &str) -> bool {
    nom.strip_prefix(base).and_then(|r| r.strip_prefix(" (")).and_then(|r| r.strip_suffix(')')).is_some_and(|n| !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit()))
}
