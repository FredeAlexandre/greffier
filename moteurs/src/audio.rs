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

use crate::reechantillonnage::{en_16_bits, reechantillonner};

/// La fréquence d'échantillonnage donnée à whisper.cpp et à pyannote.
pub const FREQUENCE: u32 = 16_000;

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
struct DescriptionDuFlux {
    frequence: f64,
    format: u32,
    drapeaux: u32,
    octets_par_paquet: u32,
    trames_par_paquet: u32,
    octets_par_trame: u32,
    canaux: u32,
    bits_par_canal: u32,
    reserve: u32,
}

#[link(name = "CoreMedia", kind = "framework")]
unsafe extern "C" {
    fn CMAudioFormatDescriptionGetStreamBasicDescription(description: *const c_void) -> *const DescriptionDuFlux;
}

/// La piste de mixage de `media`, en PCM mono 16 kHz sur 16 bits. `progres` reçoit la part lue
/// (0 à 1) ; `arret` interrompt la lecture (rend `Err("arrêté")`).
pub fn mixage(media: &Path, arret: &AtomicBool, mut progres: impl FnMut(f64)) -> Result<Vec<i16>, String> {
    let (mono, frequence) = decoder(media, arret, &mut progres)?;
    Ok(en_16_bits(&reechantillonner(&mono, frequence, FREQUENCE)))
}

/// La première piste audio, décodée et moyennée en mono, à sa fréquence d'origine.
fn decoder(media: &Path, arret: &AtomicBool, progres: &mut dyn FnMut(f64)) -> Result<(Vec<f32>, u32), String> {
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
        let pistes = asset.tracksWithMediaType(audio);
        let piste = pistes.firstObject().ok_or("Le média n'a pas de son.")?;
        let duree = {
            let d = asset.duration();
            if d.timescale > 0 { d.value as f64 / d.timescale as f64 } else { 0.0 }
        };

        // Flottants 32 bits entrelacés ; fréquence et canaux d'origine (non précisés).
        let cles = [AV_FORMAT_ID, AV_LINEAR_PCM_BIT_DEPTH, AV_LINEAR_PCM_IS_FLOAT, AV_LINEAR_PCM_IS_BIG_ENDIAN, AV_LINEAR_PCM_IS_NON_INTERLEAVED].map(NSString::from_str);
        let valeurs: [Retained<NSNumber>; 5] = [NSNumber::new_u32(LINEAR_PCM), NSNumber::new_u32(32), NSNumber::new_bool(true), NSNumber::new_bool(false), NSNumber::new_bool(false)];
        let cles_ref: Vec<&NSString> = cles.iter().map(|c| &**c).collect();
        let valeurs_ref: Vec<&AnyObject> = valeurs.iter().map(|v| -> &AnyObject { v.as_ref() }).collect();
        let reglages = NSDictionary::from_slices(&cles_ref, &valeurs_ref);

        let lecteur = AVAssetReader::initWithAsset_error(AVAssetReader::alloc(), &asset).map_err(|e| format!("Le média est illisible : {}", e.localizedDescription()))?;
        let sortie = AVAssetReaderTrackOutput::assetReaderTrackOutputWithTrack_outputSettings(&piste, Some(&reglages));
        if !lecteur.canAddOutput(&sortie) {
            return Err("AVFoundation ne sait pas décoder ce son.".into());
        }
        lecteur.addOutput(&sortie);
        if !lecteur.startReading() {
            let e = lecteur.error().map(|e| e.localizedDescription().to_string()).unwrap_or_default();
            return Err(format!("Le média est illisible : {e}"));
        }

        let mut mono: Vec<f32> = Vec::new();
        let mut format: Option<(u32, usize)> = None;
        let mut trames: Vec<f32> = Vec::new();
        while let Some(tampon) = sortie.copyNextSampleBuffer() {
            if arret.load(Ordering::Relaxed) {
                lecteur.cancelReading();
                return Err("arrêté".into());
            }
            if format.is_none() {
                let description = tampon.format_description().ok_or("Son sans description.")?;
                let d = CMAudioFormatDescriptionGetStreamBasicDescription((&*description as *const _) as *const c_void);
                let d = d.as_ref().ok_or("Son sans description.")?;
                if d.frequence < 1.0 || d.canaux == 0 {
                    return Err("Son de format inconnu.".into());
                }
                format = Some((d.frequence.round() as u32, d.canaux as usize));
                mono.reserve((duree * d.frequence) as usize + 1);
            }
            let (frequence, canaux) = format.expect("format lu");
            let Some(bloc) = tampon.data_buffer() else { continue };
            let n = bloc.data_length() / 4;
            trames.clear();
            trames.resize(n, 0.0);
            if n > 0 {
                let statut = bloc.copy_data_bytes(0, n * 4, NonNull::new(trames.as_mut_ptr().cast()).expect("tampon"));
                if statut != 0 {
                    return Err(format!("Lecture du son impossible (CoreMedia {statut})."));
                }
            }
            mono.extend(trames.chunks_exact(canaux).map(|t| t.iter().sum::<f32>() / canaux as f32));
            if duree > 0.0 {
                progres((mono.len() as f64 / (duree * frequence as f64)).min(1.0));
            }
        }
        if lecteur.status() != AVAssetReaderStatus::Completed {
            let e = lecteur.error().map(|e| e.localizedDescription().to_string()).unwrap_or_default();
            return Err(format!("La lecture du son s'est arrêtée : {e}"));
        }
        let (frequence, _) = format.ok_or("Le média n'a pas de son.")?;
        Ok((mono, frequence))
    }
}

/// Écrit des échantillons PCM mono 16 kHz sur 16 bits en WAV (pour pyannote, ou pour comparer
/// avec ffmpeg).
pub fn ecrire_wav(chemin: &Path, echantillons: &[i16]) -> std::io::Result<()> {
    let donnees = (echantillons.len() * 2) as u32;
    let mut w = Vec::with_capacity(44 + donnees as usize);
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&(36 + donnees).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes()); // PCM
    w.extend_from_slice(&1u16.to_le_bytes()); // mono
    w.extend_from_slice(&FREQUENCE.to_le_bytes());
    w.extend_from_slice(&(FREQUENCE * 2).to_le_bytes());
    w.extend_from_slice(&2u16.to_le_bytes());
    w.extend_from_slice(&16u16.to_le_bytes());
    w.extend_from_slice(b"data");
    w.extend_from_slice(&donnees.to_le_bytes());
    for e in echantillons {
        w.extend_from_slice(&e.to_le_bytes());
    }
    std::fs::write(chemin, w)
}
