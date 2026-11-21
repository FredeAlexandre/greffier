//! Le mixage des deux sources audio en trois pistes : mixage, micro seul, son du Mac seul.
//!
//! ScreenCaptureKit livre le micro et le son du Mac par paquets séparés, chacun avec son
//! horodatage. Le mixeur range chaque paquet à sa place (en échantillons depuis le début de
//! l'enregistrement), puis rend des blocs alignés des trois pistes. Une source qui se tait (pas de
//! paquet) ne bloque pas les autres plus de [`RETARD_MAX`] échantillons : son trou devient du
//! silence. Tout est en stéréo entrelacée, flottants 32 bits, à [`FREQUENCE`] Hz.

use std::collections::VecDeque;

pub const FREQUENCE: u32 = 48_000;
pub const CANAUX: usize = 2;
/// Au plus une demi-seconde d'attente d'une source en retard.
pub const RETARD_MAX: u64 = FREQUENCE as u64 / 2;
/// Un décalage d'horodatage plus petit (1 ms) est du bruit d'horloge, pas un trou.
const TOLERANCE: u64 = FREQUENCE as u64 / 1000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Micro = 0,
    SonDuMac = 1,
}

/// Des trames stéréo alignées sur les trois pistes, à partir de la trame `debut`.
#[derive(Clone, Debug, PartialEq)]
pub struct Bloc {
    pub debut: u64,
    pub mixage: Vec<f32>,
    pub micro: Vec<f32>,
    pub son_du_mac: Vec<f32>,
}

impl Bloc {
    pub fn trames(&self) -> usize {
        self.mixage.len() / CANAUX
    }
}

#[derive(Default)]
struct Piste {
    /// Trame du premier échantillon de `tampon`.
    debut: u64,
    tampon: VecDeque<f32>,
    /// A reçu au moins un paquet.
    vue: bool,
}

impl Piste {
    fn fin(&self) -> u64 {
        self.debut + (self.tampon.len() / CANAUX) as u64
    }
}

#[derive(Default)]
pub struct Mixeur {
    pistes: [Piste; 2],
    actives: [bool; 2],
    /// Trames déjà rendues.
    rendu: u64,
}

impl Mixeur {
    pub fn new(micro: bool, son_du_mac: bool) -> Mixeur {
        Mixeur { actives: [micro, son_du_mac], ..Mixeur::default() }
    }

    /// Range un paquet de trames stéréo entrelacées qui commence à la trame `debut` (négative si
    /// le paquet commence avant l'enregistrement).
    pub fn pousser(&mut self, source: Source, debut: i64, trames: &[f32]) {
        let rendu = self.rendu;
        let p = &mut self.pistes[source as usize];
        let mut trames = trames;
        let mut debut = debut;
        // Ce qui tombe avant le début ou avant ce qui est déjà rendu est perdu.
        let plancher = rendu.max(if p.vue { p.fin() } else { 0 }) as i64;
        if !p.vue && p.tampon.is_empty() {
            p.debut = rendu;
        }
        let fin = p.fin() as i64;
        if p.vue && (debut - fin).unsigned_abs() <= TOLERANCE {
            debut = fin;
        }
        if debut < plancher {
            let coupe = ((plancher - debut) as usize * CANAUX).min(trames.len());
            trames = &trames[coupe..];
            debut = plancher;
        }
        if trames.is_empty() {
            return;
        }
        let fin = p.fin() as i64;
        if debut > fin {
            p.tampon.extend(std::iter::repeat_n(0.0, (debut - fin) as usize * CANAUX));
        }
        p.tampon.extend(trames.iter().copied());
        p.vue = true;
    }

    /// Le bloc prêt : jusqu'où toutes les sources actives sont arrivées, ou jusqu'à
    /// [`RETARD_MAX`] derrière la plus avancée. `None` s'il n'y a rien de nouveau.
    pub fn tirer(&mut self) -> Option<Bloc> {
        let fins: Vec<u64> = (0..2).filter(|i| self.actives[*i]).map(|i| self.pistes[i].fin()).collect();
        let (min, max) = (fins.iter().copied().min()?, fins.iter().copied().max()?);
        self.rendre_jusqu_a(min.max(max.saturating_sub(RETARD_MAX)))
    }

    /// Du silence jusqu'à la trame `fin`, quand aucune source audio n'est enregistrée.
    pub fn silence_jusqu_a(&mut self, fin: u64) -> Option<Bloc> {
        self.rendre_jusqu_a(fin)
    }

    /// Tout ce qui reste, à l'arrêt.
    pub fn vider(&mut self) -> Option<Bloc> {
        let max = self.pistes.iter().map(Piste::fin).max()?;
        self.rendre_jusqu_a(max)
    }

    /// Trames déjà rendues : la durée de l'audio écrit.
    pub fn rendu(&self) -> u64 {
        self.rendu
    }

    fn rendre_jusqu_a(&mut self, limite: u64) -> Option<Bloc> {
        if limite <= self.rendu {
            return None;
        }
        let n = (limite - self.rendu) as usize;
        let mut sorties = [Vec::with_capacity(n * CANAUX), Vec::with_capacity(n * CANAUX)];
        for (p, sortie) in self.pistes.iter_mut().zip(sorties.iter_mut()) {
            if p.debut < self.rendu {
                // Une piste jamais vue reste à la traîne : on la recale.
                let retard = ((self.rendu - p.debut) as usize * CANAUX).min(p.tampon.len());
                p.tampon.drain(..retard);
                p.debut = self.rendu;
            }
            let pris = (n * CANAUX).min(p.tampon.len());
            sortie.extend(p.tampon.drain(..pris));
            sortie.resize(n * CANAUX, 0.0);
            p.debut = limite;
        }
        let [micro, son_du_mac] = sorties;
        let mixage = micro.iter().zip(&son_du_mac).map(|(a, b)| (a + b).clamp(-1.0, 1.0)).collect();
        let bloc = Bloc { debut: self.rendu, mixage, micro, son_du_mac };
        self.rendu = limite;
        Some(bloc)
    }
}

/// Convertit un paquet en stéréo entrelacée à [`FREQUENCE`] Hz : `canaux` est le nombre de canaux
/// de `entree` (entrelacés), `frequence` sa fréquence. Le rééchantillonnage est linéaire (le micro
/// arrive en principe déjà à 48 kHz).
pub fn en_stereo_48k(entree: &[f32], canaux: usize, frequence: f64) -> Vec<f32> {
    if canaux == 0 {
        return Vec::new();
    }
    let trames = entree.len() / canaux;
    let trame = |i: usize| -> (f32, f32) {
        let t = &entree[i * canaux..(i + 1) * canaux];
        if canaux == 1 { (t[0], t[0]) } else { (t[0], t[1]) }
    };
    if (frequence - f64::from(FREQUENCE)).abs() < 0.5 {
        return (0..trames).flat_map(|i| {
            let (g, d) = trame(i);
            [g, d]
        }).collect();
    }
    let sortie = (trames as f64 * f64::from(FREQUENCE) / frequence).round() as usize;
    let pas = frequence / f64::from(FREQUENCE);
    (0..sortie)
        .flat_map(|j| {
            let x = j as f64 * pas;
            let i = (x.floor() as usize).min(trames.saturating_sub(1));
            let k = (i + 1).min(trames.saturating_sub(1));
            let f = (x - i as f64) as f32;
            let ((g0, d0), (g1, d1)) = (trame(i), trame(k));
            [g0 + (g1 - g0) * f, d0 + (d1 - d0) * f]
        })
        .collect()
}

/// Le niveau affiché d'un paquet, de 0 à 1 : sa puissance en dB, de -60 dB (0) à 0 dB (1) ; le
/// bruit d'une pièce calme (vers -60 dB) se voit à peine, une voix (-30 à -20 dB) bien.
pub fn niveau(trames: &[f32]) -> f32 {
    if trames.is_empty() {
        return 0.0;
    }
    let rms = (trames.iter().map(|x| x * x).sum::<f32>() / trames.len() as f32).sqrt();
    if rms <= 0.0 {
        return 0.0;
    }
    ((20.0 * rms.log10() + 60.0) / 60.0).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn constante(trames: usize, v: f32) -> Vec<f32> {
        vec![v; trames * CANAUX]
    }

    #[test]
    fn deux_sources_alignees_sont_additionnees() {
        let mut m = Mixeur::new(true, true);
        m.pousser(Source::Micro, 0, &constante(100, 0.25));
        assert_eq!(m.tirer(), None, "le son du Mac n'est pas encore arrivé");
        m.pousser(Source::SonDuMac, 0, &constante(60, 0.5));
        let b = m.tirer().expect("bloc");
        assert_eq!((b.debut, b.trames()), (0, 60));
        assert_eq!(b.mixage[0], 0.75);
        assert_eq!(b.micro[0], 0.25);
        assert_eq!(b.son_du_mac[0], 0.5);
        let reste = m.vider().expect("reste");
        assert_eq!((reste.debut, reste.trames()), (60, 40));
        assert_eq!(reste.mixage[0], 0.25);
        assert_eq!(m.rendu(), 100);
    }

    #[test]
    fn une_source_muette_ne_bloque_qu_une_demi_seconde() {
        let mut m = Mixeur::new(true, true);
        m.pousser(Source::Micro, 0, &constante(48_000, 0.1));
        let b = m.tirer().expect("bloc");
        assert_eq!(b.trames(), 24_000);
        assert!(b.son_du_mac.iter().all(|x| *x == 0.0));
        // Le son du Mac arrive enfin, à sa place.
        m.pousser(Source::SonDuMac, 30_000, &constante(18_000, 0.2));
        let b = m.tirer().expect("bloc");
        assert_eq!((b.debut, b.trames()), (24_000, 24_000));
        assert_eq!(b.son_du_mac[(5_999) * CANAUX], 0.0);
        assert_eq!(b.son_du_mac[6_000 * CANAUX], 0.2);
    }

    #[test]
    fn un_trou_devient_du_silence_et_un_petit_decalage_est_ignore() {
        let mut m = Mixeur::new(true, false);
        m.pousser(Source::Micro, 0, &constante(100, 0.1));
        m.pousser(Source::Micro, 120, &constante(100, 0.1));
        m.pousser(Source::Micro, 1_000, &constante(10, 0.3));
        let b = m.vider().expect("bloc");
        assert_eq!(b.trames(), 1_010);
        assert_eq!(b.micro[150 * CANAUX], 0.1, "20 trames de décalage : collées");
        assert_eq!(b.micro[500 * CANAUX], 0.0);
        assert_eq!(b.micro[1_000 * CANAUX], 0.3);
    }

    #[test]
    fn ce_qui_precede_le_debut_est_coupe() {
        let mut m = Mixeur::new(true, false);
        m.pousser(Source::Micro, -10, &[vec![0.9; 10 * CANAUX], constante(5, 0.1)].concat());
        let b = m.vider().expect("bloc");
        assert_eq!((b.debut, b.trames()), (0, 5));
        assert_eq!(b.micro[0], 0.1);
    }

    #[test]
    fn le_mixage_ne_sature_pas() {
        let mut m = Mixeur::new(true, true);
        m.pousser(Source::Micro, 0, &constante(1, 0.8));
        m.pousser(Source::SonDuMac, 0, &constante(1, 0.8));
        assert_eq!(m.tirer().expect("bloc").mixage, [1.0, 1.0]);
    }

    #[test]
    fn mono_et_autres_frequences_passent_en_stereo_48k() {
        assert_eq!(en_stereo_48k(&[0.1, 0.2], 1, 48_000.0), [0.1, 0.1, 0.2, 0.2]);
        assert_eq!(en_stereo_48k(&[0.1, 0.2, 0.3, 0.4], 2, 48_000.0), [0.1, 0.2, 0.3, 0.4]);
        assert_eq!(en_stereo_48k(&vec![0.5; 441], 1, 44_100.0).len(), 480 * CANAUX);
    }

    #[test]
    fn le_niveau_va_de_moins_60_db_a_0_db() {
        assert_eq!(niveau(&[0.0; 8]), 0.0);
        assert_eq!(niveau(&[0.0005; 8]), 0.0);
        assert_eq!(niveau(&[1.0; 8]), 1.0);
        assert!((niveau(&[0.1; 8]) - 2.0 / 3.0).abs() < 1e-3, "-20 dB");
    }
}
