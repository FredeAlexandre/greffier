//! L'écriture de l'enregistrement par AVAssetWriter, au fil de l'eau, en MP4 fragmenté (un
//! fragment toutes les [`INTERVALLE_DES_FRAGMENTS`] secondes : un arrêt brutal ne perd que le
//! dernier).
//!
//! Format repris du vault : vidéo H.264 à 30 i/s, ~1200 kbps ; trois pistes audio AAC 160 kbps à
//! 48 kHz, dans cet ordre : mixage (celle que lit la transcription, la première), micro seul, son
//! du Mac seul. Les trois pistes audio forment un groupe dont le mixage est la piste par défaut :
//! un lecteur ne joue que lui, au lieu d'additionner les trois.

use std::collections::VecDeque;
use std::path::Path;
use std::ptr::{self, NonNull};
use std::sync::mpsc;
use std::time::Duration;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::AnyThread;
use objc2_av_foundation::{
    AVAssetWriter, AVAssetWriterInput, AVAssetWriterInputGroup, AVAssetWriterInputPixelBufferAdaptor, AVAssetWriterStatus, AVFileTypeMPEG4, AVMediaTypeAudio, AVMediaTypeVideo,
    AVVideoAverageBitRateKey, AVVideoCodecKey, AVVideoCodecTypeH264, AVVideoCompressionPropertiesKey, AVVideoExpectedSourceFrameRateKey, AVVideoHeightKey, AVVideoMaxKeyFrameIntervalKey,
    AVVideoProfileLevelH264HighAutoLevel, AVVideoProfileLevelKey, AVVideoWidthKey,
};
use objc2_core_audio_types::{AudioStreamBasicDescription, kAudioFormatFlagIsFloat, kAudioFormatFlagIsPacked, kAudioFormatLinearPCM};
use objc2_core_foundation::CFRetained;
use objc2_core_media::{CMAudioFormatDescriptionCreate, CMAudioSampleBufferCreateReadyWithPacketDescriptions, CMBlockBuffer, CMFormatDescription, CMSampleBuffer, CMTime, CMTimeRoundingMethod, kCMBlockBufferAssureMemoryNowFlag};
use objc2_core_video::CVPixelBuffer;
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString, NSURL};

use crate::mixage::{Bloc, CANAUX, FREQUENCE};

pub const IMAGES_PAR_SECONDE: i32 = 30;
pub const DEBIT_VIDEO: i32 = 1_200_000;
pub const DEBIT_AUDIO: i32 = 160_000;
pub const INTERVALLE_DES_FRAGMENTS: f64 = 2.0;
/// Au plus 4 images (133 ms) attendent l'audio ; ScreenCaptureKit en a 8 en réserve.
pub const IMAGES_EN_ATTENTE: usize = 4;
/// 'aac '
const FORMAT_AAC: u32 = 0x6161_6320;

pub struct Ecrivain {
    ecrivain: Retained<AVAssetWriter>,
    video: Option<(Retained<AVAssetWriterInput>, Retained<AVAssetWriterInputPixelBufferAdaptor>)>,
    /// Mixage, micro, son du Mac.
    audio: [Retained<AVAssetWriterInput>; 3],
    format_audio: CFRetained<CMFormatDescription>,
    /// L'horodatage du premier échantillon : le début de l'enregistrement.
    origine: Option<CMTime>,
    /// Secondes écrites depuis l'origine.
    duree: f64,
    /// Échantillons refusés faute de place (l'encodeur ne suit pas).
    pub perdus: u64,
    /// Le premier échec de l'écrivain.
    echec: Option<String>,
    /// Les images qui attendent l'audio, et les trames audio déjà écrites.
    /// (instants en 1/90 000 s)
    en_attente: VecDeque<(CFRetained<CVPixelBuffer>, i64)>,
    audio_ecrit: u64,
    /// La dernière image, pour la répéter quand l'écran ne change pas.
    derniere_image: Option<(CFRetained<CVPixelBuffer>, i64)>,
}

// AVAssetWriter peut être utilisé depuis n'importe quel fil, pourvu que ce soit un à la fois :
// l'écrivain vit derrière un Mutex.
unsafe impl Send for Ecrivain {}

fn cle(s: &str) -> Retained<NSString> {
    NSString::from_str(s)
}

fn dictionnaire(paires: &[(&NSString, Retained<AnyObject>)]) -> Retained<NSDictionary<NSString, AnyObject>> {
    let cles: Vec<&NSString> = paires.iter().map(|(c, _)| *c).collect();
    let valeurs: Vec<&AnyObject> = paires.iter().map(|(_, v)| &**v).collect();
    NSDictionary::from_slices(&cles, &valeurs)
}

fn nombre(n: f64) -> Retained<AnyObject> {
    objet(&*NSNumber::new_f64(n))
}

fn entier(n: i64) -> Retained<AnyObject> {
    objet(&*NSNumber::new_i64(n))
}

fn objet<T: objc2::Message>(o: &T) -> Retained<AnyObject> {
    // SAFETY : tout objet Objective-C est un AnyObject.
    unsafe { Retained::retain(o as *const T as *mut AnyObject) }.expect("objet")
}

/// Le message d'une erreur, avec son domaine, son code et l'erreur sous-jacente.
pub fn erreur(e: &objc2_foundation::NSError) -> String {
    let mut m = format!("{} ({} {})", e.localizedDescription(), e.domain(), e.code());
    let info = e.userInfo();
    if let Some(sous) = info.objectForKey(&cle("NSUnderlyingError"))
        && let Ok(sous) = sous.downcast::<objc2_foundation::NSError>()
    {
        m.push_str(&format!(" ; {}", erreur(&sous)));
    }
    m
}

impl Ecrivain {
    /// Prépare le fichier. `video` : la taille des images, s'il y a une piste vidéo. Les images
    /// attendent l'audio : sans source audio, il faut écrire du silence ([`Mixeur::silence_jusqu_a`]).
    ///
    /// [`Mixeur::silence_jusqu_a`]: crate::mixage::Mixeur::silence_jusqu_a
    pub fn new(fichier: &Path, video: Option<(usize, usize)>) -> Result<Ecrivain, String> {
        let url = NSURL::from_file_path(fichier).ok_or("chemin de l'enregistrement invalide")?;
        let _ = std::fs::remove_file(fichier);
        unsafe {
            let ecrivain = AVAssetWriter::initWithURL_fileType_error(AVAssetWriter::alloc(), &url, AVFileTypeMPEG4.expect("AVFileTypeMPEG4")).map_err(|e| erreur(&e))?;
            ecrivain.setShouldOptimizeForNetworkUse(false);
            ecrivain.setMovieFragmentInterval(CMTime::with_seconds(INTERVALLE_DES_FRAGMENTS, 600));

            let video = match video {
                Some((largeur, hauteur)) => {
                    let compression = dictionnaire(&[
                        (AVVideoAverageBitRateKey.expect("clé"), entier(i64::from(DEBIT_VIDEO))),
                        (AVVideoExpectedSourceFrameRateKey.expect("clé"), entier(i64::from(IMAGES_PAR_SECONDE))),
                        (AVVideoMaxKeyFrameIntervalKey.expect("clé"), entier(i64::from(2 * IMAGES_PAR_SECONDE))),
                        (AVVideoProfileLevelKey.expect("clé"), objet(AVVideoProfileLevelH264HighAutoLevel.expect("clé"))),
                    ]);
                    let reglages = dictionnaire(&[
                        (AVVideoCodecKey.expect("clé"), objet(AVVideoCodecTypeH264.expect("clé"))),
                        (AVVideoWidthKey.expect("clé"), entier(largeur as i64)),
                        (AVVideoHeightKey.expect("clé"), entier(hauteur as i64)),
                        (AVVideoCompressionPropertiesKey.expect("clé"), objet(&*compression)),
                    ]);
                    let entree = AVAssetWriterInput::initWithMediaType_outputSettings(AVAssetWriterInput::alloc(), AVMediaTypeVideo.expect("vidéo"), Some(&reglages));
                    entree.setExpectsMediaDataInRealTime(true);
                    if !ecrivain.canAddInput(&entree) {
                        return Err("piste vidéo refusée par AVAssetWriter".into());
                    }
                    ecrivain.addInput(&entree);
                    let adaptateur = AVAssetWriterInputPixelBufferAdaptor::assetWriterInputPixelBufferAdaptorWithAssetWriterInput_sourcePixelBufferAttributes(&entree, None);
                    Some((entree, adaptateur))
                }
                None => None,
            };

            let reglages_audio = dictionnaire(&[
                (&cle("AVFormatIDKey"), entier(i64::from(FORMAT_AAC))),
                (&cle("AVSampleRateKey"), nombre(f64::from(FREQUENCE))),
                (&cle("AVNumberOfChannelsKey"), entier(CANAUX as i64)),
                (&cle("AVEncoderBitRateKey"), entier(i64::from(DEBIT_AUDIO))),
                // Débit constant, comme l'AAC d'OBS dans le vault.
                (&cle("AVEncoderBitRateStrategyKey"), objet(&*cle("AVAudioBitRateStrategy_Constant"))),
            ]);
            let piste = || -> Result<Retained<AVAssetWriterInput>, String> {
                let entree = AVAssetWriterInput::initWithMediaType_outputSettings(AVAssetWriterInput::alloc(), AVMediaTypeAudio.expect("audio"), Some(&reglages_audio));
                entree.setExpectsMediaDataInRealTime(true);
                if !ecrivain.canAddInput(&entree) {
                    return Err("piste audio refusée par AVAssetWriter".into());
                }
                ecrivain.addInput(&entree);
                Ok(entree)
            };
            let audio = [piste()?, piste()?, piste()?];
            let groupe = AVAssetWriterInputGroup::initWithInputs_defaultInput(AVAssetWriterInputGroup::alloc(), &NSArray::from_retained_slice(&audio), Some(&audio[0]));
            if ecrivain.canAddInputGroup(&groupe) {
                ecrivain.addInputGroup(&groupe);
            }

            let asbd = AudioStreamBasicDescription {
                mSampleRate: f64::from(FREQUENCE),
                mFormatID: kAudioFormatLinearPCM,
                mFormatFlags: kAudioFormatFlagIsFloat | kAudioFormatFlagIsPacked,
                mBytesPerPacket: (4 * CANAUX) as u32,
                mFramesPerPacket: 1,
                mBytesPerFrame: (4 * CANAUX) as u32,
                mChannelsPerFrame: CANAUX as u32,
                mBitsPerChannel: 32,
                mReserved: 0,
            };
            let mut format: *const CMFormatDescription = ptr::null();
            let statut = CMAudioFormatDescriptionCreate(None, NonNull::from(&asbd), 0, ptr::null(), 0, ptr::null(), None, NonNull::from(&mut format));
            if statut != 0 || format.is_null() {
                return Err(format!("format audio ({statut})"));
            }
            let format_audio = CFRetained::from_raw(NonNull::new_unchecked(format as *mut CMFormatDescription));

            if !ecrivain.startWriting() {
                return Err(ecrivain.error().map_or("AVAssetWriter ne démarre pas".into(), |e| erreur(&e)));
            }
            Ok(Ecrivain { ecrivain, video, audio, format_audio, origine: None, duree: 0.0, perdus: 0, echec: None, en_attente: VecDeque::new(), audio_ecrit: 0, derniere_image: None })
        }
    }

    /// L'origine de l'enregistrement : fixée par le premier échantillon reçu.
    pub fn origine(&mut self, t: CMTime) -> CMTime {
        let t = echelle_video(t);
        *self.origine.get_or_insert_with(|| {
            unsafe { self.ecrivain.startSessionAtSourceTime(t) };
            t
        })
    }

    pub fn duree(&self) -> f64 {
        self.duree
    }

    fn noter(&mut self, fin: CMTime) {
        if let Some(o) = self.origine {
            let d = unsafe { fin.subtract(o).seconds() };
            if d.is_finite() && d > self.duree {
                self.duree = d;
            }
        }
    }

    /// Une image de l'écran, à son horodatage. Deux règles, sans lesquelles AVAssetWriter échoue
    /// (-11800 / -16341) en écrivant un fragment :
    /// - la vidéo est à cadence fixe : ScreenCaptureKit n'envoie rien quand l'écran ne change pas,
    ///   on répète alors la dernière image (comme OBS) ;
    /// - une image attend que l'audio l'ait rattrapée (au plus [`IMAGES_EN_ATTENTE`] images).
    pub fn image(&mut self, image: &CVPixelBuffer, t: CMTime) {
        let t = echelle_video(t);
        let origine = self.origine(t);
        if unsafe { t.compare(origine) } < 0 || self.video.is_none() {
            return;
        }
        // SAFETY : une référence de plus sur le tampon, le temps de l'attente et des répétitions.
        let image = unsafe { CFRetained::retain(NonNull::from(image)) };
        let t = t.value;
        match &mut self.derniere_image {
            // Les répétitions sont déjà passées par là : l'image servira aux suivantes.
            Some((d, td)) if *td >= t - PERIODE / 2 => *d = image,
            _ => {
                self.combler(t - PERIODE / 2);
                self.en_attente.push_back((image.clone(), t));
                self.derniere_image = Some((image, t));
            }
        }
        self.ecrire_les_images(false);
    }

    /// Répète la dernière image, à cadence fixe, jusqu'à `jusqu_a` (en 1/90 000 s).
    fn combler(&mut self, jusqu_a: i64) {
        let Some((image, t)) = &mut self.derniere_image else { return };
        while *t + PERIODE <= jusqu_a {
            *t += PERIODE;
            self.en_attente.push_back((image.clone(), *t));
        }
    }

    /// L'instant jusqu'où l'audio est écrit, en 1/90 000 s.
    fn audio_jusqu_a(&self) -> Option<i64> {
        self.origine.map(|o| o.value + (self.audio_ecrit as i64) * i64::from(ECHELLE_VIDEO) / i64::from(FREQUENCE))
    }

    /// Écrit les images que l'audio a rattrapées (toutes si `tout`).
    fn ecrire_les_images(&mut self, tout: bool) {
        let audio = self.audio_jusqu_a();
        while let Some((_, t)) = self.en_attente.front() {
            let rattrapee = audio.is_some_and(|a| *t <= a);
            if !(tout || rattrapee || self.en_attente.len() > IMAGES_EN_ATTENTE) {
                break;
            }
            let Some((image, t)) = self.en_attente.pop_front() else { break };
            let Some((entree, adaptateur)) = &self.video else { return };
            let t = unsafe { CMTime::new(t, ECHELLE_VIDEO) };
            let ok = unsafe { entree.isReadyForMoreMediaData() && adaptateur.appendPixelBuffer_withPresentationTime(&image, t) };
            if ok {
                self.noter(t);
            } else {
                self.perdu("image", t);
            }
        }
    }

    /// Les trois pistes d'un bloc du mixeur, placé à `origine + debut / 48 kHz`.
    pub fn audio(&mut self, bloc: &Bloc) {
        let Some(origine) = self.origine else { return };
        let n = bloc.trames();
        if n == 0 {
            return;
        }
        let t = unsafe { origine.convert_scale(FREQUENCE as i32, CMTimeRoundingMethod::RoundHalfAwayFromZero).add(CMTime::new(bloc.debut as i64, FREQUENCE as i32)) };
        if crate::mac::trace() {
            eprintln!("bloc debut={} n={n}", bloc.debut);
        }
        for (i, donnees) in [&bloc.mixage, &bloc.micro, &bloc.son_du_mac].into_iter().enumerate() {
            let Some(tampon) = echantillons(&self.format_audio, donnees, n, t) else {
                self.perdus += 1;
                continue;
            };
            let entree = &self.audio[i];
            let ok = unsafe { entree.isReadyForMoreMediaData() && entree.appendSampleBuffer(&tampon) };
            if !ok {
                self.perdu("audio", t);
            }
        }
        self.noter(unsafe { t.add(CMTime::new(n as i64, FREQUENCE as i32)) });
        self.audio_ecrit = bloc.debut + n as u64;
        // L'écran immobile : la vidéo suit l'audio, avec un peu de retard pour laisser arriver
        // une vraie image.
        if let Some(a) = self.audio_jusqu_a() {
            self.combler(a - RETARD_DES_REPETITIONS);
        }
        self.ecrire_les_images(false);
    }

    /// Note un échantillon refusé ; dit pourquoi au premier échec de l'écrivain.
    fn perdu(&mut self, quoi: &str, t: CMTime) {
        self.perdus += 1;
        let statut = unsafe { self.ecrivain.status() };
        if statut == AVAssetWriterStatus::Failed && self.echec.is_none() {
            let m = unsafe { self.ecrivain.error() }.map_or("?".into(), |e| erreur(&e));
            eprintln!("greffier-capture : l'écriture a échoué sur {quoi} à {:.3} s : {m}", unsafe { t.seconds() });
            self.echec = Some(m);
        }
    }

    /// Ferme les pistes et finit le fichier ; rend la durée écrite.
    pub fn finir(mut self) -> Result<f64, String> {
        if let Some(a) = self.audio_jusqu_a() {
            self.combler(a);
        }
        self.ecrire_les_images(true);
        if self.origine.is_none() {
            unsafe { self.ecrivain.cancelWriting() };
            return Err("rien n'a été capturé".into());
        }
        unsafe {
            if let Some((entree, _)) = &self.video {
                entree.markAsFinished();
            }
            for entree in &self.audio {
                entree.markAsFinished();
            }
            let (tx, rx) = mpsc::channel();
            let fini = RcBlock::new(move || {
                let _ = tx.send(());
            });
            self.ecrivain.finishWritingWithCompletionHandler(&fini);
            rx.recv_timeout(Duration::from_secs(60)).map_err(|_| "AVAssetWriter ne finit pas d'écrire")?;
            if self.ecrivain.status() != AVAssetWriterStatus::Completed {
                return Err(self.ecrivain.error().map_or("écriture de l'enregistrement en échec".into(), |e| erreur(&e)));
            }
        }
        Ok(self.duree)
    }
}

/// Les horodatages de ScreenCaptureKit sont en nanosecondes : à cette échelle, une durée de piste
/// dépasse 32 bits après 4,29 s et AVAssetWriter échoue au fragment suivant. On les ramène à 90 kHz.
fn echelle_video(t: CMTime) -> CMTime {
    if t.timescale == ECHELLE_VIDEO {
        return t;
    }
    unsafe { t.convert_scale(ECHELLE_VIDEO, CMTimeRoundingMethod::RoundHalfAwayFromZero) }
}

const ECHELLE_VIDEO: i32 = 90_000;
/// Une image à 30 i/s, en 1/90 000 s.
const PERIODE: i64 = ECHELLE_VIDEO as i64 / IMAGES_PAR_SECONDE as i64;
/// Les répétitions suivent l'audio à 100 ms.
const RETARD_DES_REPETITIONS: i64 = ECHELLE_VIDEO as i64 / 10;

/// Un tampon CoreMedia de `n` trames stéréo flottantes entrelacées.
fn echantillons(format: &CMFormatDescription, donnees: &[f32], n: usize, t: CMTime) -> Option<CFRetained<CMSampleBuffer>> {
    let octets = std::mem::size_of_val(donnees);
    unsafe {
        let mut bloc: *mut CMBlockBuffer = ptr::null_mut();
        let s = CMBlockBuffer::create_with_memory_block(None, ptr::null_mut(), octets, None, ptr::null(), 0, octets, kCMBlockBufferAssureMemoryNowFlag, NonNull::from(&mut bloc));
        if s != 0 || bloc.is_null() {
            return None;
        }
        let bloc = CFRetained::from_raw(NonNull::new_unchecked(bloc));
        if CMBlockBuffer::replace_data_bytes(NonNull::new_unchecked(donnees.as_ptr() as *mut _), &bloc, 0, octets) != 0 {
            return None;
        }
        let mut tampon: *mut CMSampleBuffer = ptr::null_mut();
        let s = CMAudioSampleBufferCreateReadyWithPacketDescriptions(None, &bloc, format, n as isize, t, ptr::null(), NonNull::from(&mut tampon));
        if s != 0 || tampon.is_null() {
            return None;
        }
        Some(CFRetained::from_raw(NonNull::new_unchecked(tampon)))
    }
}
