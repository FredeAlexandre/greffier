//! Le passage à 16 kHz, fait ici plutôt que par AVFoundation : son convertisseur de fréquence ne
//! rend pas deux fois le même signal (et la transcription change alors d'un lancement à
//! l'autre), alors que son décodage à la fréquence d'origine est stable.
//!
//! Filtre polyphasé à sinc fenêtré, réglé comme le rééchantillonneur par défaut de ffmpeg
//! (swresample : 32 zéros de part et d'autre à la fréquence de coupure, coupure à 0,97 de la
//! fréquence de Nyquist la plus basse, fenêtre de Kaiser β = 9), sans retard : l'échantillon de
//! sortie k est centré sur l'instant k / fréquence de sortie.

/// Longueur du filtre, relative à la fréquence de coupure (`filter_size` de swresample).
const FILTER_SIZE: f64 = 32.0;
const CUTOFF: f64 = 0.97;
const BETA: f64 = 9.0;

fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// Fonction de Bessel modifiée de première espèce, d'ordre 0.
fn bessel_i0(x: f64) -> f64 {
    let (mut sum, mut term, mut k) = (1.0, 1.0, 1.0);
    loop {
        term *= (x / (2.0 * k)).powi(2);
        sum += term;
        if term < sum * 1e-12 {
            return sum;
        }
        k += 1.0;
    }
}

/// `entree` (échantillonné à `de` Hz) rééchantillonné à `vers` Hz.
pub fn resample(input: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || input.is_empty() {
        return input.to_vec();
    }
    let g = gcd(from, to);
    let (phases, step) = ((to / g) as usize, (from / g) as usize);
    // Coupure relative à la fréquence de Nyquist de l'entrée.
    let cutoff = CUTOFF * (to as f64 / from as f64).min(1.0);
    let half = (FILTER_SIZE / cutoff / 2.0).ceil() as i64;
    let width = (2 * half) as usize;
    let i0_beta = bessel_i0(BETA);

    // Les coefficients de chaque phase : la sortie tombe à `p / phases` après l'échantillon
    // d'entrée de base ; on prend les échantillons base - demi + 1 ..= base + demi.
    let mut table = vec![0f32; phases * width];
    for p in 0..phases {
        let f = p as f64 / phases as f64;
        let mut c = vec![0f64; width];
        for (j, cj) in c.iter_mut().enumerate() {
            let x = (j as i64 - half + 1) as f64 - f;
            let sinc = if x == 0.0 { 1.0 } else { (std::f64::consts::PI * cutoff * x).sin() / (std::f64::consts::PI * cutoff * x) };
            let r = x / half as f64;
            let window = if r.abs() >= 1.0 { 0.0 } else { bessel_i0(BETA * (1.0 - r * r).sqrt()) / i0_beta };
            *cj = sinc * window;
        }
        let sum: f64 = c.iter().sum();
        for (j, cj) in c.iter().enumerate() {
            table[p * width + j] = (cj / sum) as f32;
        }
    }

    let n = input.len();
    let outputs = (n as u64 * phases as u64).div_ceil(step as u64) as usize;
    let mut output = Vec::with_capacity(outputs);
    for k in 0..outputs {
        let position = k * step;
        let (base, p) = ((position / phases) as i64, position % phases);
        let coeffs = &table[p * width..(p + 1) * width];
        let first = base - half + 1;
        let v = if first >= 0 && first as usize + width <= n {
            let window = &input[first as usize..first as usize + width];
            window.iter().zip(coeffs).map(|(a, b)| a * b).sum()
        } else {
            // Bords : les échantillons hors du signal valent 0.
            coeffs.iter().enumerate().map(|(j, c)| {
                let i = first + j as i64;
                if i >= 0 && (i as usize) < n { input[i as usize] * c } else { 0.0 }
            }).sum()
        };
        output.push(v);
    }
    output
}

/// Flottants (−1 à 1) en entiers 16 bits, arrondis au plus proche (pair en cas d'égalité, comme
/// `lrintf`) et écrêtés, sans tramage : comme swresample.
pub fn to_16_bit(input: &[f32]) -> Vec<i16> {
    input.iter().map(|&x| (x * 32768.0).round_ties_even().clamp(-32768.0, 32767.0) as i16).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(sample_rate: f64, from: u32, seconds: f64) -> Vec<f32> {
        (0..(from as f64 * seconds) as usize).map(|i| (0.5 * (2.0 * std::f64::consts::PI * sample_rate * i as f64 / from as f64).sin()) as f32).collect()
    }

    #[test]
    fn an_in_band_sine_keeps_its_amplitude_and_phase() {
        for from in [48_000, 44_100, 22_050] {
            let s = resample(&sine(1000.0, from, 1.0), from, 16_000);
            assert!((s.len() as i64 - 16_000).abs() <= 1, "{from} : {}", s.len());
            let expected = sine(1000.0, 16_000, 1.0);
            let distance = s[500..15_500].iter().zip(&expected[500..15_500]).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
            assert!(distance < 0.01, "{from} : {distance}");
        }
    }

    #[test]
    fn content_above_8_khz_is_cut() {
        let s = resample(&sine(10_000.0, 48_000, 1.0), 48_000, 16_000);
        let peak = s[500..15_500].iter().map(|x| x.abs()).fold(0.0, f32::max);
        assert!(peak < 0.005, "{peak}");
    }

    #[test]
    fn result_does_not_change_between_calls() {
        let e = sine(440.0, 44_100, 0.5);
        assert_eq!(resample(&e, 44_100, 16_000), resample(&e, 44_100, 16_000));
    }

    #[test]
    fn conversion_to_16_bits_rounds_and_clips() {
        assert_eq!(to_16_bit(&[0.0, 0.5, -0.5, 1.5, -1.5, 1.0 / 65536.0]), [0, 16384, -16384, 32767, -32768, 0]);
    }
}
