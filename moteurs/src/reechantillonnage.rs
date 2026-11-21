//! Le passage à 16 kHz, fait ici plutôt que par AVFoundation : son convertisseur de fréquence ne
//! rend pas deux fois le même signal (et la transcription change alors d'un lancement à
//! l'autre), alors que son décodage à la fréquence d'origine est stable.
//!
//! Filtre polyphasé à sinc fenêtré, réglé comme le rééchantillonneur par défaut de ffmpeg
//! (swresample : 32 zéros de part et d'autre à la fréquence de coupure, coupure à 0,97 de la
//! fréquence de Nyquist la plus basse, fenêtre de Kaiser β = 9), sans retard : l'échantillon de
//! sortie k est centré sur l'instant k / fréquence de sortie.

/// Longueur du filtre, relative à la fréquence de coupure (`filter_size` de swresample).
const TAILLE: f64 = 32.0;
const COUPURE: f64 = 0.97;
const BETA: f64 = 9.0;

fn pgcd(a: u32, b: u32) -> u32 {
    if b == 0 { a } else { pgcd(b, a % b) }
}

/// Fonction de Bessel modifiée de première espèce, d'ordre 0.
fn bessel_i0(x: f64) -> f64 {
    let (mut somme, mut terme, mut k) = (1.0, 1.0, 1.0);
    loop {
        terme *= (x / (2.0 * k)).powi(2);
        somme += terme;
        if terme < somme * 1e-12 {
            return somme;
        }
        k += 1.0;
    }
}

/// `entree` (échantillonné à `de` Hz) rééchantillonné à `vers` Hz.
pub fn reechantillonner(entree: &[f32], de: u32, vers: u32) -> Vec<f32> {
    if de == vers || entree.is_empty() {
        return entree.to_vec();
    }
    let g = pgcd(de, vers);
    let (phases, pas) = ((vers / g) as usize, (de / g) as usize);
    // Coupure relative à la fréquence de Nyquist de l'entrée.
    let coupure = COUPURE * (vers as f64 / de as f64).min(1.0);
    let demi = (TAILLE / coupure / 2.0).ceil() as i64;
    let largeur = (2 * demi) as usize;
    let i0_beta = bessel_i0(BETA);

    // Les coefficients de chaque phase : la sortie tombe à `p / phases` après l'échantillon
    // d'entrée de base ; on prend les échantillons base - demi + 1 ..= base + demi.
    let mut table = vec![0f32; phases * largeur];
    for p in 0..phases {
        let f = p as f64 / phases as f64;
        let mut c = vec![0f64; largeur];
        for (j, cj) in c.iter_mut().enumerate() {
            let x = (j as i64 - demi + 1) as f64 - f;
            let sinc = if x == 0.0 { 1.0 } else { (std::f64::consts::PI * coupure * x).sin() / (std::f64::consts::PI * coupure * x) };
            let r = x / demi as f64;
            let fenetre = if r.abs() >= 1.0 { 0.0 } else { bessel_i0(BETA * (1.0 - r * r).sqrt()) / i0_beta };
            *cj = sinc * fenetre;
        }
        let somme: f64 = c.iter().sum();
        for (j, cj) in c.iter().enumerate() {
            table[p * largeur + j] = (cj / somme) as f32;
        }
    }

    let n = entree.len();
    let sorties = (n as u64 * phases as u64).div_ceil(pas as u64) as usize;
    let mut sortie = Vec::with_capacity(sorties);
    for k in 0..sorties {
        let position = k * pas;
        let (base, p) = ((position / phases) as i64, position % phases);
        let coefs = &table[p * largeur..(p + 1) * largeur];
        let premier = base - demi + 1;
        let v = if premier >= 0 && premier as usize + largeur <= n {
            let fenetre = &entree[premier as usize..premier as usize + largeur];
            fenetre.iter().zip(coefs).map(|(a, b)| a * b).sum()
        } else {
            // Bords : les échantillons hors du signal valent 0.
            coefs.iter().enumerate().map(|(j, c)| {
                let i = premier + j as i64;
                if i >= 0 && (i as usize) < n { entree[i as usize] * c } else { 0.0 }
            }).sum()
        };
        sortie.push(v);
    }
    sortie
}

/// Flottants (−1 à 1) en entiers 16 bits, arrondis au plus proche (pair en cas d'égalité, comme
/// `lrintf`) et écrêtés, sans tramage : comme swresample.
pub fn en_16_bits(entree: &[f32]) -> Vec<i16> {
    entree.iter().map(|&x| (x * 32768.0).round_ties_even().clamp(-32768.0, 32767.0) as i16).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sinus(frequence: f64, de: u32, secondes: f64) -> Vec<f32> {
        (0..(de as f64 * secondes) as usize).map(|i| (0.5 * (2.0 * std::f64::consts::PI * frequence * i as f64 / de as f64).sin()) as f32).collect()
    }

    #[test]
    fn un_sinus_dans_la_bande_garde_son_amplitude_et_sa_phase() {
        for de in [48_000, 44_100, 22_050] {
            let s = reechantillonner(&sinus(1000.0, de, 1.0), de, 16_000);
            assert!((s.len() as i64 - 16_000).abs() <= 1, "{de} : {}", s.len());
            let attendu = sinus(1000.0, 16_000, 1.0);
            let ecart = s[500..15_500].iter().zip(&attendu[500..15_500]).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
            assert!(ecart < 0.01, "{de} : {ecart}");
        }
    }

    #[test]
    fn ce_qui_depasse_8_khz_est_coupe() {
        let s = reechantillonner(&sinus(10_000.0, 48_000, 1.0), 48_000, 16_000);
        let crete = s[500..15_500].iter().map(|x| x.abs()).fold(0.0, f32::max);
        assert!(crete < 0.005, "{crete}");
    }

    #[test]
    fn le_resultat_ne_change_pas_d_un_appel_a_l_autre() {
        let e = sinus(440.0, 44_100, 0.5);
        assert_eq!(reechantillonner(&e, 44_100, 16_000), reechantillonner(&e, 44_100, 16_000));
    }

    #[test]
    fn la_conversion_en_16_bits_arrondit_et_ecrete() {
        assert_eq!(en_16_bits(&[0.0, 0.5, -0.5, 1.5, -1.5, 1.0 / 65536.0]), [0, 16384, -16384, 32767, -32768, 0]);
    }
}
