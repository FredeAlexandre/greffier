//! Le mixage des deux sources audio en trois pistes : mixage, micro seul, son du Mac seul.
//!
//! ScreenCaptureKit livre le micro et le son du Mac par paquets séparés, chacun avec son
//! horodatage. Le mixeur range chaque paquet à sa place (en échantillons depuis le début de
//! l'enregistrement), puis rend des blocs alignés des trois pistes. Une source qui se tait (pas de
//! paquet) ne bloque pas les autres plus de [`RETARD_MAX`] échantillons : son trou devient du
//! silence. Tout est en stéréo entrelacée, flottants 32 bits, à [`FREQUENCE`] Hz.

use std::collections::VecDeque;

pub const SAMPLE_RATE: u32 = 48_000;
pub const CHANNELS: usize = 2;
/// Au plus une demi-seconde d'attente d'une source en retard.
pub const MAX_DELAY: u64 = SAMPLE_RATE as u64 / 2;
/// Un décalage d'horodatage plus petit (1 ms) est du bruit d'horloge, pas un trou.
const TOLERANCE: u64 = SAMPLE_RATE as u64 / 1000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Micro = 0,
    MacAudio = 1,
}

/// Des trames stéréo alignées sur les trois pistes, à partir de la trame `debut`.
#[derive(Clone, Debug, PartialEq)]
pub struct AudioBlock {
    pub start: u64,
    pub mixing: Vec<f32>,
    pub micro: Vec<f32>,
    pub mac_audio: Vec<f32>,
}

impl AudioBlock {
    pub fn frames(&self) -> usize {
        self.mixing.len() / CHANNELS
    }
}

#[derive(Default)]
struct Track {
    /// Trame du premier échantillon de `tampon`.
    start: u64,
    buffer: VecDeque<f32>,
    /// A reçu au moins un paquet.
    seen: bool,
}

impl Track {
    fn end(&self) -> u64 {
        self.start + (self.buffer.len() / CHANNELS) as u64
    }
}

#[derive(Default)]
pub struct Mixer {
    tracks: [Track; 2],
    active: [bool; 2],
    /// Trames déjà rendues.
    rendered: u64,
}

impl Mixer {
    pub fn new(micro: bool, mac_audio: bool) -> Mixer {
        Mixer { active: [micro, mac_audio], ..Mixer::default() }
    }

    /// Range un paquet de trames stéréo entrelacées qui commence à la trame `debut` (négative si
    /// le paquet commence avant l'enregistrement).
    pub fn push(&mut self, source: Source, start: i64, frames: &[f32]) {
        let rendered = self.rendered;
        let p = &mut self.tracks[source as usize];
        let mut frames = frames;
        let mut start = start;
        // Ce qui tombe avant le début ou avant ce qui est déjà rendu est perdu.
        let floor = rendered.max(if p.seen { p.end() } else { 0 }) as i64;
        if !p.seen && p.buffer.is_empty() {
            p.start = rendered;
        }
        let end = p.end() as i64;
        if p.seen && (start - end).unsigned_abs() <= TOLERANCE {
            start = end;
        }
        if start < floor {
            let cut = ((floor - start) as usize * CHANNELS).min(frames.len());
            frames = &frames[cut..];
            start = floor;
        }
        if frames.is_empty() {
            return;
        }
        let end = p.end() as i64;
        if start > end {
            p.buffer.extend(std::iter::repeat_n(0.0, (start - end) as usize * CHANNELS));
        }
        p.buffer.extend(frames.iter().copied());
        p.seen = true;
    }

    /// Le bloc prêt : jusqu'où toutes les sources actives sont arrivées, ou jusqu'à
    /// [`RETARD_MAX`] derrière la plus avancée. `None` s'il n'y a rien de nouveau.
    pub fn pull(&mut self) -> Option<AudioBlock> {
        let ends: Vec<u64> = (0..2).filter(|i| self.active[*i]).map(|i| self.tracks[i].end()).collect();
        let (min, max) = (ends.iter().copied().min()?, ends.iter().copied().max()?);
        self.render_until(min.max(max.saturating_sub(MAX_DELAY)))
    }

    /// Du silence jusqu'à la trame `fin`, quand aucune source audio n'est enregistrée.
    pub fn silence_until(&mut self, end: u64) -> Option<AudioBlock> {
        self.render_until(end)
    }

    /// Tout ce qui reste, à l'arrêt.
    pub fn drain(&mut self) -> Option<AudioBlock> {
        let max = self.tracks.iter().map(Track::end).max()?;
        self.render_until(max)
    }

    /// Trames déjà rendues : la durée de l'audio écrit.
    pub fn rendered(&self) -> u64 {
        self.rendered
    }

    fn render_until(&mut self, limit: u64) -> Option<AudioBlock> {
        if limit <= self.rendered {
            return None;
        }
        let n = (limit - self.rendered) as usize;
        let mut outputs = [Vec::with_capacity(n * CHANNELS), Vec::with_capacity(n * CHANNELS)];
        for (p, output) in self.tracks.iter_mut().zip(outputs.iter_mut()) {
            if p.start < self.rendered {
                // Une piste jamais vue reste à la traîne : on la recale.
                let delay = ((self.rendered - p.start) as usize * CHANNELS).min(p.buffer.len());
                p.buffer.drain(..delay);
                p.start = self.rendered;
            }
            let taken = (n * CHANNELS).min(p.buffer.len());
            output.extend(p.buffer.drain(..taken));
            output.resize(n * CHANNELS, 0.0);
            p.start = limit;
        }
        let [micro, mac_audio] = outputs;
        let mixing = micro.iter().zip(&mac_audio).map(|(a, b)| (a + b).clamp(-1.0, 1.0)).collect();
        let block = AudioBlock { start: self.rendered, mixing, micro, mac_audio };
        self.rendered = limit;
        Some(block)
    }
}

/// Convertit un paquet en stéréo entrelacée à [`FREQUENCE`] Hz : `canaux` est le nombre de canaux
/// de `entree` (entrelacés), `frequence` sa fréquence. Le rééchantillonnage est linéaire (le micro
/// arrive en principe déjà à 48 kHz).
pub fn to_stereo_48k(input: &[f32], channels: usize, sample_rate: f64) -> Vec<f32> {
    if channels == 0 {
        return Vec::new();
    }
    let frames = input.len() / channels;
    let frame = |i: usize| -> (f32, f32) {
        let t = &input[i * channels..(i + 1) * channels];
        if channels == 1 { (t[0], t[0]) } else { (t[0], t[1]) }
    };
    if (sample_rate - f64::from(SAMPLE_RATE)).abs() < 0.5 {
        return (0..frames).flat_map(|i| {
            let (g, d) = frame(i);
            [g, d]
        }).collect();
    }
    let output = (frames as f64 * f64::from(SAMPLE_RATE) / sample_rate).round() as usize;
    let step = sample_rate / f64::from(SAMPLE_RATE);
    (0..output)
        .flat_map(|j| {
            let x = j as f64 * step;
            let i = (x.floor() as usize).min(frames.saturating_sub(1));
            let k = (i + 1).min(frames.saturating_sub(1));
            let f = (x - i as f64) as f32;
            let ((g0, d0), (g1, d1)) = (frame(i), frame(k));
            [g0 + (g1 - g0) * f, d0 + (d1 - d0) * f]
        })
        .collect()
}

/// Le niveau affiché d'un paquet, de 0 à 1 : sa puissance en dB, de -60 dB (0) à 0 dB (1) ; le
/// bruit d'une pièce calme (vers -60 dB) se voit à peine, une voix (-30 à -20 dB) bien.
pub fn level(frames: &[f32]) -> f32 {
    if frames.is_empty() {
        return 0.0;
    }
    let rms = (frames.iter().map(|x| x * x).sum::<f32>() / frames.len() as f32).sqrt();
    if rms <= 0.0 {
        return 0.0;
    }
    ((20.0 * rms.log10() + 60.0) / 60.0).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn constant(frames: usize, v: f32) -> Vec<f32> {
        vec![v; frames * CHANNELS]
    }

    #[test]
    fn two_aligned_sources_are_summed() {
        let mut m = Mixer::new(true, true);
        m.push(Source::Micro, 0, &constant(100, 0.25));
        assert_eq!(m.pull(), None, "le son du Mac n'est pas encore arrivé");
        m.push(Source::MacAudio, 0, &constant(60, 0.5));
        let b = m.pull().expect("bloc");
        assert_eq!((b.start, b.frames()), (0, 60));
        assert_eq!(b.mixing[0], 0.75);
        assert_eq!(b.micro[0], 0.25);
        assert_eq!(b.mac_audio[0], 0.5);
        let rest = m.drain().expect("reste");
        assert_eq!((rest.start, rest.frames()), (60, 40));
        assert_eq!(rest.mixing[0], 0.25);
        assert_eq!(m.rendered(), 100);
    }

    #[test]
    fn a_silent_source_blocks_only_half_a_second() {
        let mut m = Mixer::new(true, true);
        m.push(Source::Micro, 0, &constant(48_000, 0.1));
        let b = m.pull().expect("bloc");
        assert_eq!(b.frames(), 24_000);
        assert!(b.mac_audio.iter().all(|x| *x == 0.0));
        // Le son du Mac arrive enfin, à sa place.
        m.push(Source::MacAudio, 30_000, &constant(18_000, 0.2));
        let b = m.pull().expect("bloc");
        assert_eq!((b.start, b.frames()), (24_000, 24_000));
        assert_eq!(b.mac_audio[(5_999) * CHANNELS], 0.0);
        assert_eq!(b.mac_audio[6_000 * CHANNELS], 0.2);
    }

    #[test]
    fn a_gap_becomes_silence_and_a_small_offset_is_ignored() {
        let mut m = Mixer::new(true, false);
        m.push(Source::Micro, 0, &constant(100, 0.1));
        m.push(Source::Micro, 120, &constant(100, 0.1));
        m.push(Source::Micro, 1_000, &constant(10, 0.3));
        let b = m.drain().expect("bloc");
        assert_eq!(b.frames(), 1_010);
        assert_eq!(b.micro[150 * CHANNELS], 0.1, "20 trames de décalage : collées");
        assert_eq!(b.micro[500 * CHANNELS], 0.0);
        assert_eq!(b.micro[1_000 * CHANNELS], 0.3);
    }

    #[test]
    fn content_before_the_start_is_cut() {
        let mut m = Mixer::new(true, false);
        m.push(Source::Micro, -10, &[vec![0.9; 10 * CHANNELS], constant(5, 0.1)].concat());
        let b = m.drain().expect("bloc");
        assert_eq!((b.start, b.frames()), (0, 5));
        assert_eq!(b.micro[0], 0.1);
    }

    #[test]
    fn mixing_does_not_clip() {
        let mut m = Mixer::new(true, true);
        m.push(Source::Micro, 0, &constant(1, 0.8));
        m.push(Source::MacAudio, 0, &constant(1, 0.8));
        assert_eq!(m.pull().expect("bloc").mixing, [1.0, 1.0]);
    }

    #[test]
    fn mono_and_other_sample_rates_become_stereo_48k() {
        assert_eq!(to_stereo_48k(&[0.1, 0.2], 1, 48_000.0), [0.1, 0.1, 0.2, 0.2]);
        assert_eq!(to_stereo_48k(&[0.1, 0.2, 0.3, 0.4], 2, 48_000.0), [0.1, 0.2, 0.3, 0.4]);
        assert_eq!(to_stereo_48k(&vec![0.5; 441], 1, 44_100.0).len(), 480 * CHANNELS);
    }

    #[test]
    fn level_ranges_from_minus_60_db_to_0_db() {
        assert_eq!(level(&[0.0; 8]), 0.0);
        assert_eq!(level(&[0.0005; 8]), 0.0);
        assert_eq!(level(&[1.0; 8]), 1.0);
        assert!((level(&[0.1; 8]) - 2.0 / 3.0).abs() < 1e-3, "-20 dB");
    }
}
