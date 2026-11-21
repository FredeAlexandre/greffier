//! L'attribution des mots aux locuteurs et le découpage en répliques.
//!
//! Version provisoire : chaque mot va au tour de parole qui le chevauche le plus, et une
//! réplique regroupe les mots consécutifs d'un même tour. Sans tours de parole, tout va à un seul
//! locuteur et une réplique s'arrête après une pause de 2 s. Les règles du vault (décalage de
//! +0,2 s, interruptions, recalage sur les fins de phrase, coupure après 220 caractères)
//! arrivent avec la séparation des locuteurs (#8).

use crate::reunion::{Locuteur, Mot, Replique, TEINTES, TourDeParole};

const PAUSE: f64 = 2.0;

/// Les locuteurs détectés, dans l'ordre de leurs étiquettes, et les répliques.
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
    let mut groupe_courant: Option<usize> = None;
    for mot in mots {
        let (groupe, locuteur) = match tour_du_mot(mot, tours) {
            Some(t) => (t, index(&tours[t].locuteur)),
            None => (usize::MAX, 0),
        };
        let suite = match repliques.last() {
            Some(r) if groupe_courant == Some(groupe) => groupe != usize::MAX || mot.debut - r.fin < PAUSE,
            _ => false,
        };
        if suite {
            let r = repliques.last_mut().expect("réplique en cours");
            r.texte.push(' ');
            r.texte.push_str(&mot.texte);
            r.fin = mot.fin;
        } else {
            repliques.push(Replique { locuteur, debut: mot.debut, fin: mot.fin, texte: mot.texte.clone(), passage: None });
        }
        groupe_courant = Some(groupe);
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
