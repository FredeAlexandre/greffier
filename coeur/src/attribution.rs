//! Des mots de Whisper aux répliques : bruits retirés, attribution aux locuteurs, découpage.
//!
//! Le retrait des bruits et le découpage en répliques sont ceux du vault. L'attribution est
//! encore provisoire : chaque mot va au tour de parole qui le chevauche le plus ; sans tours de
//! parole, tout va à un seul locuteur. Les règles du vault (décalage de +0,2 s, interruptions,
//! recalage sur les fins de phrase) arrivent avec la séparation des locuteurs (#8).

use crate::reunion::{Locuteur, Mot, Replique, TEINTES, TourDeParole};

/// Une pause qui commence une nouvelle réplique, en secondes.
const PAUSE: f64 = 2.0;
/// Passé cette longueur (en unités UTF-16, comme le vault), une réplique s'arrête à la première
/// fin de phrase.
const LONGUEUR_DE_REPLIQUE: usize = 220;
/// Au-delà de ce nombre de mots, une parenthèse ou un crochet qui ne se ferme pas n'est pas un
/// bruit.
const MOTS_D_UN_BRUIT: usize = 6;

/// Les mots sans les bruits notés entre crochets ou parenthèses (« [Musique] », « (rires) »,
/// « [bruit de fond] ») ni les mots vides.
pub fn sans_les_bruits(mots: Vec<Mot>) -> Vec<Mot> {
    let ouvre = |t: &str| t.starts_with('[') || t.starts_with('(');
    let ferme = |t: &str| t.ends_with(']') || t.ends_with(')');
    let mut gardes = Vec::with_capacity(mots.len());
    let mut bruit: Option<Vec<Mot>> = None;
    for m in mots.into_iter().filter(|m| !m.texte.trim().is_empty()) {
        let t = m.texte.trim().to_string();
        if let Some(b) = &mut bruit {
            b.push(m);
            if ferme(&t) {
                bruit = None;
            } else if b.len() > MOTS_D_UN_BRUIT {
                gardes.append(b);
                bruit = None;
            }
            continue;
        }
        if ouvre(&t) {
            if !ferme(&t) {
                bruit = Some(vec![m]);
            }
            continue;
        }
        gardes.push(m);
    }
    gardes.extend(bruit.unwrap_or_default());
    gardes
}

/// `.`, `?`, `!` ou `…`, suivi ou non d'un guillemet fermant.
fn fin_de_phrase(texte: &str) -> bool {
    let t = texte.strip_suffix('»').or_else(|| texte.strip_suffix('"')).unwrap_or(texte);
    t.ends_with(['.', '?', '!', '…'])
}

/// Les locuteurs détectés, dans l'ordre de leurs étiquettes, et les répliques. Une nouvelle
/// réplique commence à chaque changement de locuteur (une ponctuation seule reste avec la
/// réplique précédente), après une pause de 2 s, ou à une fin de phrase passé 220 caractères.
pub fn attribuer(mots: &[Mot], tours: &[TourDeParole]) -> (Vec<Locuteur>, Vec<Replique>) {
    let mut etiquettes: Vec<&str> = tours.iter().map(|t| t.locuteur.as_str()).collect();
    etiquettes.sort_unstable();
    etiquettes.dedup();
    if etiquettes.is_empty() && !mots.is_empty() {
        etiquettes.push("SPEAKER_00");
    }
    let locuteurs: Vec<Locuteur> = etiquettes.iter().enumerate().map(|(i, e)| Locuteur::new(*e, TEINTES[i % TEINTES.len()])).collect();
    let index = |etiquette: &str| etiquettes.iter().position(|e| *e == etiquette).unwrap_or(0);

    let mut repliques: Vec<Replique> = Vec::new();
    for mot in mots {
        let texte = mot.texte.trim();
        let locuteur = tour_du_mot(mot, tours).map_or(0, |t| index(&tours[t].locuteur));
        let ponctuation = !texte.chars().any(char::is_alphanumeric);
        let suite = match repliques.last() {
            None => false,
            Some(r) => {
                let longue = r.texte.encode_utf16().count() >= LONGUEUR_DE_REPLIQUE && fin_de_phrase(&r.texte);
                (r.locuteur == locuteur || ponctuation) && mot.debut - r.fin < PAUSE && !longue
            }
        };
        if suite {
            let r = repliques.last_mut().expect("réplique en cours");
            r.texte.push(' ');
            r.texte.push_str(texte);
            r.fin = r.fin.max(mot.fin);
        } else {
            repliques.push(Replique { locuteur, debut: mot.debut, fin: mot.fin, texte: texte.to_string(), passage: None });
        }
    }
    (locuteurs, repliques)
}

/// Le tour qui chevauche le plus le mot, sinon le plus proche.
fn tour_du_mot(mot: &Mot, tours: &[TourDeParole]) -> Option<usize> {
    let chevauchement = |t: &TourDeParole| (mot.fin.min(t.fin) - mot.debut.max(t.debut)).max(0.0);
    let distance = |t: &TourDeParole| (t.debut - mot.fin).max(mot.debut - t.fin).max(0.0);
    let mut meilleur: Option<(usize, f64, f64)> = None;
    for (i, t) in tours.iter().enumerate() {
        let (c, d) = (chevauchement(t), distance(t));
        let mieux = match meilleur {
            None => true,
            Some((_, mc, md)) => c > mc || (c == mc && c == 0.0 && d < md),
        };
        if mieux {
            meilleur = Some((i, c, d));
        }
    }
    meilleur.map(|(i, _, _)| i)
}
