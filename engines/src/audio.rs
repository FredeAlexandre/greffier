//! L'audio donné aux moteurs : la piste de mixage d'un média en PCM mono 16 kHz sur 16 bits (ce
//! qu'en tirait `ffmpeg -vn -ac 1 -ar 16000 -c:a pcm_s16le` dans le vault).
//!
//! AVFoundation décode la piste en flottants, à sa fréquence et avec ses canaux d'origine ; les
//! canaux sont moyennés, puis le signal passe à 16 kHz par [`crate::reechantillonnage`] (le
//! convertisseur de fréquence d'AVFoundation ne rend pas deux fois le même signal).
//!
//! La piste de mixage est la première piste audio : l'enregistrement de Greffier porte le
//! mixage, puis le micro seul, puis le son du Mac seul ; un média importé n'en a souvent qu'une.

use std::ffi::c_void;
use std::path::Path;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, Ordering};

use objc2::AnyThread;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_av_foundation::{AVAssetReader, AVAssetReaderStatus, AVAssetReaderTrackOutput, AVMediaTypeAudio, AVURLAsset};
use objc2_foundation::{NSDictionary, NSNumber, NSString, NSURL};

use crate::resampling::{to_16_bit, resample};

/// La fréquence d'échantillonnage donnée à whisper.cpp et à pyannote.
pub const SAMPLE_RATE: u32 = 16_000;

/// Les clés d'AVAudioSettings.h (leurs valeurs sont leurs noms).
const AV_FORMAT_ID: &str = "AVFormatIDKey";
const AV_LINEAR_PCM_BIT_DEPTH: &str = "AVLinearPCMBitDepthKey";
const AV_LINEAR_PCM_IS_FLOAT: &str = "AVLinearPCMIsFloatKey";
const AV_LINEAR_PCM_IS_BIG_ENDIAN: &str = "AVLinearPCMIsBigEndianKey";
const AV_LINEAR_PCM_IS_NON_INTERLEAVED: &str = "AVLinearPCMIsNonInterleaved";
/// kAudioFormatLinearPCM, « lpcm ».
const LINEAR_PCM: u32 = u32::from_be_bytes(*b"lpcm");

/// AudioStreamBasicDescription (CoreAudioTypes.h).
#[repr(C)]
struct StreamDescription {
    sample_rate: f64,
    format: u32,
    flags: u32,
    bytes_per_packet: u32,
    frames_per_packet: u32,
    bytes_per_frame: u32,
    channels: u32,
    bits_per_channel: u32,
    reserve: u32,
}

#[link(name = "CoreMedia", kind = "framework")]
unsafe extern "C" {
    fn CMAudioFormatDescriptionGetStreamBasicDescription(description: *const c_void) -> *const StreamDescription;
}

/// La piste de mixage de `media`, en PCM mono 16 kHz sur 16 bits. `progres` reçoit la part lue
/// (0 à 1) ; `arret` interrompt la lecture (rend `Err("arrêté")`).
pub fn mixing(media: &Path, stop: &AtomicBool, mut progress: impl FnMut(f64)) -> Result<Vec<i16>, String> {
    let (mono, sample_rate) = decode(media, stop, &mut progress)?;
    Ok(to_16_bit(&resample(&mono, sample_rate, SAMPLE_RATE)))
}

/// La première piste audio, décodée et moyennée en mono, à sa fréquence d'origine.
fn decode(media: &Path, stop: &AtomicBool, progress: &mut dyn FnMut(f64)) -> Result<(Vec<f32>, u32), String> {
    if !media.exists() {
        return Err(format!("L'enregistrement est introuvable ({}).", media.display()));
    }
    // SAFETY : appels AVFoundation et CoreMedia documentés, sur des objets créés ici et gardés en
    // vie par leurs `Retained` ; le tampon de destination de `copy_data_bytes` a la taille
    // demandée ; la description du flux appartient au tampon, lu tant qu'il est vivant.
    unsafe {
        let url = NSURL::fileURLWithPath(&NSString::from_str(&media.to_string_lossy()));
        let asset = AVURLAsset::URLAssetWithURL_options(&url, None);
        let audio = AVMediaTypeAudio.ok_or("AVFoundation sans type audio")?;
        #[allow(deprecated)]
        let tracks = asset.tracksWithMediaType(audio);
        let track = tracks.firstObject().ok_or("Le média n'a pas de son.")?;
        let duration = {
            let d = asset.duration();
            if d.timescale > 0 { d.value as f64 / d.timescale as f64 } else { 0.0 }
        };

        // Flottants 32 bits entrelacés ; fréquence et canaux d'origine (non précisés).
        let keys = [AV_FORMAT_ID, AV_LINEAR_PCM_BIT_DEPTH, AV_LINEAR_PCM_IS_FLOAT, AV_LINEAR_PCM_IS_BIG_ENDIAN, AV_LINEAR_PCM_IS_NON_INTERLEAVED].map(NSString::from_str);
        let values: [Retained<NSNumber>; 5] = [NSNumber::new_u32(LINEAR_PCM), NSNumber::new_u32(32), NSNumber::new_bool(true), NSNumber::new_bool(false), NSNumber::new_bool(false)];
        let key_refs: Vec<&NSString> = keys.iter().map(|c| &**c).collect();
        let value_refs: Vec<&AnyObject> = values.iter().map(|v| -> &AnyObject { v.as_ref() }).collect();
        let settings = NSDictionary::from_slices(&key_refs, &value_refs);

        let reader = AVAssetReader::initWithAsset_error(AVAssetReader::alloc(), &asset).map_err(|e| format!("Le média est illisible : {}", e.localizedDescription()))?;
        let output = AVAssetReaderTrackOutput::assetReaderTrackOutputWithTrack_outputSettings(&track, Some(&settings));
        if !reader.canAddOutput(&output) {
            return Err("AVFoundation ne sait pas décoder ce son.".into());
        }
        reader.addOutput(&output);
        if !reader.startReading() {
            let e = reader.error().map(|e| e.localizedDescription().to_string()).unwrap_or_default();
            return Err(format!("Le média est illisible : {e}"));
        }

        let mut mono: Vec<f32> = Vec::new();
        let mut format: Option<(u32, usize)> = None;
        let mut frames: Vec<f32> = Vec::new();
        while let Some(buffer) = output.copyNextSampleBuffer() {
            if stop.load(Ordering::Relaxed) {
                reader.cancelReading();
                return Err("arrêté".into());
            }
            if format.is_none() {
                let description = buffer.format_description().ok_or("Son sans description.")?;
                let d = CMAudioFormatDescriptionGetStreamBasicDescription((&*description as *const _) as *const c_void);
                let d = d.as_ref().ok_or("Son sans description.")?;
                if d.sample_rate < 1.0 || d.channels == 0 {
                    return Err("Son de format inconnu.".into());
                }
                format = Some((d.sample_rate.round() as u32, d.channels as usize));
                mono.reserve((duration * d.sample_rate) as usize + 1);
            }
            let (sample_rate, channels) = format.expect("format lu");
            let Some(block) = buffer.data_buffer() else { continue };
            let n = block.data_length() / 4;
            frames.clear();
            frames.resize(n, 0.0);
            if n > 0 {
                let status = block.copy_data_bytes(0, n * 4, NonNull::new(frames.as_mut_ptr().cast()).expect("tampon"));
                if status != 0 {
                    return Err(format!("Lecture du son impossible (CoreMedia {status})."));
                }
            }
            mono.extend(frames.chunks_exact(channels).map(|t| t.iter().sum::<f32>() / channels as f32));
            if duration > 0.0 {
                progress((mono.len() as f64 / (duration * sample_rate as f64)).min(1.0));
            }
        }
        if reader.status() != AVAssetReaderStatus::Completed {
            let e = reader.error().map(|e| e.localizedDescription().to_string()).unwrap_or_default();
            return Err(format!("La lecture du son s'est arrêtée : {e}"));
        }
        let (sample_rate, _) = format.ok_or("Le média n'a pas de son.")?;
        Ok((mono, sample_rate))
    }
}

/// Écrit des échantillons PCM mono 16 kHz sur 16 bits en WAV (pour pyannote, ou pour comparer
/// avec ffmpeg).
pub fn write_wav(path: &Path, samples: &[i16]) -> std::io::Result<()> {
    let data = (samples.len() * 2) as u32;
    let mut w = Vec::with_capacity(44 + data as usize);
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&(36 + data).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes()); // PCM
    w.extend_from_slice(&1u16.to_le_bytes()); // mono
    w.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    w.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes());
    w.extend_from_slice(&2u16.to_le_bytes());
    w.extend_from_slice(&16u16.to_le_bytes());
    w.extend_from_slice(b"data");
    w.extend_from_slice(&data.to_le_bytes());
    for e in samples {
        w.extend_from_slice(&e.to_le_bytes());
    }
    std::fs::write(path, w)
}
