//! Des mots de Whisper aux répliques : bruits retirés, attribution de chaque mot à un locuteur,
//! découpage en répliques. Les règles sont celles du vault :
//!
//! 1. **Locuteur de chaque mot** : le mot est décalé de +0,2 s (Whisper horodate un peu tôt) ;
//!    il va au locuteur qui parle le plus pendant ce temps, sinon au tour le plus proche à moins
//!    d'une seconde, sinon au locuteur du mot précédent. Un mot étiré sur plus de 2 s (whisper.cpp
//!    finit parfois un mot à la reprise de la parole) ne compte que pour ses premiers instants.
//! 2. **Interruptions** : 2 mots ou moins (la ponctuation seule ne compte pas) au milieu des mots
//!    d'un autre locuteur, sans fin de phrase, lui sont rendus : c'est un flottement de la
//!    séparation, pas une réponse.
//! 3. **Recalage** : un changement de locuteur à 3 mots ou moins d'une fin de phrase y est
//!    déplacé, en arrière d'abord (la séparation est le plus souvent en retard).
//! 4. **Étiquettes** : SPEAKER_00, SPEAKER_01… dans l'ordre du premier mot de chacun ; les
//!    locuteurs détectés sans aucun mot suivent, dans l'ordre de leur premier tour. Les tours de
//!    parole prennent ces étiquettes.
//! 5. **Répliques** : une nouvelle à chaque changement de locuteur (une ponctuation seule reste
//!    avec la précédente), après une pause de 2 s, ou à une fin de phrase passé 220 caractères.

use crate::reunion::{Locuteur, Mot, Replique, TEINTES, TourDeParole};

/// Le décalage ajouté aux horodatages de Whisper avant de chercher le locuteur, en secondes (le
/// meilleur recouvrement avec pyannote sur le banc du vault).
const DECALAGE: f64 = 0.2;
/// Au-delà, un mot est étiré sur un silence.
const MOT_ETIRE: f64 = 2.0;
/// Distance maximale au tour le plus proche, quand aucun ne chevauche le mot, en secondes.
const PROCHE: f64 = 1.0;
/// Une interruption rendue compte au plus ce nombre de mots.
const MOTS_D_UNE_INTERRUPTION: usize = 2;
/// Un changement de locuteur est recalé sur une fin de phrase à ce nombre de mots près.
const MOTS_DU_RECALAGE: usize = 3;

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

/// Un texte (la réécoute d'un extrait) sans ses bruits, mêmes règles que `sans_les_bruits`, ses
/// espaces ramenés à un seul.
pub fn texte_sans_les_bruits(texte: &str) -> String {
    let mots = texte.split_whitespace().map(|t| Mot { debut: 0.0, fin: 0.0, texte: t.to_string(), confiance: 1.0, locuteur: None }).collect();
    sans_les_bruits(mots).into_iter().map(|m| m.texte).collect::<Vec<_>>().join(" ")
}

/// `.`, `?`, `!` ou `…`, suivi ou non d'un guillemet fermant.
fn fin_de_phrase(texte: &str) -> bool {
    let t = texte.trim();
    let t = t.strip_suffix('»').or_else(|| t.strip_suffix('"')).unwrap_or(t);
    t.ends_with(['.', '?', '!', '…'])
}

/// Un vrai mot (une lettre ou un chiffre), pas une ponctuation seule.
fn a_une_lettre(texte: &str) -> bool {
    texte.chars().any(|c| c.is_alphabetic() || c.is_ascii_digit())
}

/// Le résultat de l'attribution : la transcription d'une réunion avant relecture.
#[derive(Clone, Debug, PartialEq)]
pub struct Attribution {
    pub locuteurs: Vec<Locuteur>,
    /// Les mots, chacun avec son locuteur.
    pub mots: Vec<Mot>,
    /// Les tours de parole, avec les étiquettes des locuteurs.
    pub tours: Vec<TourDeParole>,
    pub repliques: Vec<Replique>,
}

/// Attribue chaque mot à un locuteur et découpe les répliques. Sans tours de parole, tout va à
/// un seul locuteur, SPEAKER_00.
pub fn attribuer(mots: Vec<Mot>, tours: Vec<TourDeParole>) -> Attribution {
    let mut tours = tours;
    tours.sort_by(|a, b| a.debut.total_cmp(&b.debut));
    // Les locuteurs du séparateur, dans l'ordre de leur premier tour.
    let mut du_separateur: Vec<String> = Vec::new();
    for t in &tours {
        if !du_separateur.contains(&t.locuteur) {
            du_separateur.push(t.locuteur.clone());
        }
    }
    let numeros: Vec<usize> = tours.iter().map(|t| du_separateur.iter().position(|l| *l == t.locuteur).unwrap_or(0)).collect();

    let mut ids = locuteurs_des_mots(&mots, &tours, &numeros);
    rendre_les_interruptions(&mots, &mut ids);
    recaler_sur_les_fins_de_phrase(&mots, &mut ids);

    // Étiquettes : dans l'ordre du premier mot, puis du premier tour.
    let mut ordre: Vec<usize> = Vec::new();
    for &i in ids.iter().chain(&numeros) {
        if !ordre.contains(&i) {
            ordre.push(i);
        }
    }
    let rang = |i: usize| ordre.iter().position(|&x| x == i).unwrap_or(0);
    let locuteurs: Vec<Locuteur> = (0..ordre.len()).map(|i| Locuteur::new(Locuteur::etiquette_numero(i), TEINTES[i % TEINTES.len()])).collect();
    let tours = tours.into_iter().zip(&numeros).map(|(t, &n)| TourDeParole { locuteur: locuteurs[rang(n)].etiquette.clone(), ..t }).collect();
    let mots: Vec<Mot> = mots.into_iter().zip(&ids).map(|(m, &i)| Mot { locuteur: Some(rang(i)), ..m }).collect();
    let repliques = repliques(&mots);
    Attribution { locuteurs, mots, tours, repliques }
}

/// Règle 1 : le locuteur (numéro du séparateur) de chaque mot.
fn locuteurs_des_mots(mots: &[Mot], tours: &[TourDeParole], numeros: &[usize]) -> Vec<usize> {
    let mut ids = Vec::with_capacity(mots.len());
    let mut precedent: Option<usize> = None;
    let mut premier = 0;
    for m in mots {
        let duree = m.fin - m.debut;
        let debut = m.debut + DECALAGE;
        let fin = DECALAGE + if duree > MOT_ETIRE { m.debut + duree.min(0.3 + 0.08 * m.texte.trim().encode_utf16().count() as f64) } else { m.fin };
        while premier < tours.len() && tours[premier].fin < debut - 30.0 {
            premier += 1;
        }
        // Temps de parole de chaque locuteur pendant le mot, dans l'ordre des tours rencontrés.
        let mut parole: Vec<(usize, f64)> = Vec::new();
        let (mut plus_proche, mut ecart) = (None, PROCHE);
        for (t, &n) in tours[premier..].iter().zip(&numeros[premier..]) {
            if t.debut > fin + ecart {
                break;
            }
            let chevauchement = fin.min(t.fin) - debut.max(t.debut);
            if chevauchement > 0.0 {
                match parole.iter_mut().find(|(l, _)| *l == n) {
                    Some((_, s)) => *s += chevauchement,
                    None => parole.push((n, chevauchement)),
                }
            } else if -chevauchement < ecart {
                (ecart, plus_proche) = (-chevauchement, Some(n));
            }
        }
        let mut le_plus: Option<(usize, f64)> = None;
        for &(l, s) in &parole {
            if le_plus.is_none_or(|(_, m)| s > m) {
                le_plus = Some((l, s));
            }
        }
        let id = le_plus.map(|(l, _)| l).or(plus_proche).or(precedent).unwrap_or(0);
        ids.push(id);
        precedent = Some(id);
    }
    ids
}

/// Règle 2 : les interruptions de 2 mots ou moins, sans fin de phrase, rendues au locuteur en
/// cours. Une seule passe, de gauche à droite.
///
/// « Au milieu de la phrase de quelqu'un d'autre » se lit comme dans le vault (`assign()`,
/// repris à l'identique, spec #1) : le même locuteur de part et d'autre et aucune fin de phrase
/// dans l'interruption. Le vault ne regarde pas si le mot d'avant finit une phrase ; on ne le
/// regarde pas non plus, pour garder les mêmes répliques que lui.
fn rendre_les_interruptions(mots: &[Mot], ids: &mut [usize]) {
    let mut i = 0;
    while i < ids.len() {
        let mut j = i;
        while j + 1 < ids.len() && ids[j + 1] == ids[i] {
            j += 1;
        }
        if i > 0 && j + 1 < ids.len() {
            let (avant, apres) = (ids[i - 1], ids[j + 1]);
            let suite = &mots[i..=j];
            let vrais_mots = suite.iter().filter(|m| a_une_lettre(&m.texte)).count();
            if avant == apres && avant != ids[i] && vrais_mots <= MOTS_D_UNE_INTERRUPTION && !suite.iter().any(|m| fin_de_phrase(&m.texte)) {
                ids[i..=j].fill(avant);
            }
        }
        i = j + 1;
    }
}

/// Règle 3 : chaque changement de locuteur qui ne suit pas une fin de phrase est déplacé sur la
/// fin de phrase la plus proche à 3 mots près, en arrière d'abord. Les changements déjà faits
/// comptent pour les suivants.
///
/// « La plus proche » suit le vault (`assign()`, repris à l'identique, spec #1) : la plus proche
/// en arrière (le retard habituel de la séparation) si elle existe, sinon la plus proche en
/// avant, même quand celle d'avant est plus près.
fn recaler_sur_les_fins_de_phrase(mots: &[Mot], ids: &mut [usize]) {
    let finit = |k: usize| fin_de_phrase(&mots[k].texte);
    for i in 1..ids.len() {
        if ids[i] == ids[i - 1] || finit(i - 1) {
            continue;
        }
        let (ancien, nouveau) = (ids[i - 1], ids[i]);
        let arriere = (1..=MOTS_DU_RECALAGE).find(|&k| i > k && ids[i - k..i].iter().all(|&x| x == ancien) && finit(i - k - 1));
        if let Some(k) = arriere {
            ids[i - k..i].fill(nouveau);
            continue;
        }
        let avant = (1..=MOTS_DU_RECALAGE).find(|&k| i + k <= ids.len() && ids[i..i + k].iter().all(|&x| x == nouveau) && finit(i + k - 1));
        if let Some(k) = avant {
            ids[i..i + k].fill(ancien);
        }
    }
}

/// Règle 5 : les répliques, des mots déjà attribués.
fn repliques(mots: &[Mot]) -> Vec<Replique> {
    let mut repliques: Vec<Replique> = Vec::new();
    for mot in mots {
        let texte = mot.texte.trim();
        let locuteur = mot.locuteur.unwrap_or(0);
        let suite = match repliques.last() {
            None => false,
            Some(r) => {
                let longue = r.texte.encode_utf16().count() >= LONGUEUR_DE_REPLIQUE && fin_de_phrase(&r.texte);
                (r.locuteur == locuteur || !a_une_lettre(texte)) && mot.debut - r.fin < PAUSE && !longue
            }
        };
        if suite {
            let r = repliques.last_mut().expect("réplique en cours");
            r.texte.push(' ');
            r.texte.push_str(texte);
            r.fin = r.fin.max(mot.fin);
        } else {
            repliques.push(Replique { locuteur, debut: mot.debut, fin: mot.fin, texte: texte.to_string(), passages: vec![] });
        }
    }
    repliques
}
