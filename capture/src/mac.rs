//! [`CaptureDuMac`] : la capture par ScreenCaptureKit, un seul flux pour l'écran principal, le son
//! du Mac (tout l'audio du système, sauf celui de Greffier) et le micro par défaut (macOS 15 et
//! plus). Les échantillons arrivent sur une file de répartition série ; l'écrivain et le mixeur
//! vivent derrière un Mutex que seule cette file touche pendant l'enregistrement.
//!
//! Autorisations : l'écran et le son du Mac dépendent de la même autorisation de macOS
//! (« Enregistrement de l'écran et de l'audio du système ») ; le micro de la sienne. Elles ne
//! s'obtiennent que dans `Greffier.app` installée et signée (scripts/installer-app), pas par
//! `cargo run` (c'est alors le terminal qui les demande).

use std::ptr::{self, NonNull};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use block2::RcBlock;
use dispatch2::{DispatchQueue, DispatchRetained};
use objc2::rc::Retained;
use objc2::runtime::{Bool, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{AnyThread, DefinedClass, define_class, msg_send};
use objc2_av_foundation::{AVAuthorizationStatus, AVCaptureDevice, AVMediaTypeAudio};
use objc2_core_audio_types::{AudioBufferList, kAudioFormatFlagIsFloat, kAudioFormatFlagIsNonInterleaved, kAudioFormatLinearPCM};
use objc2_core_foundation::CFRetained;
use objc2_core_graphics::{CGDisplayPixelsHigh, CGDisplayPixelsWide, CGMainDisplayID, CGPreflightScreenCaptureAccess, CGRequestScreenCaptureAccess};
use objc2_core_media::{CMAudioFormatDescriptionGetStreamBasicDescription, CMBlockBuffer, CMSampleBuffer, CMTime};
use objc2_core_video::kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange;
use objc2_foundation::{NSArray, NSError};
use objc2_screen_capture_kit::{SCContentFilter, SCDisplay, SCShareableContent, SCStream, SCStreamConfiguration, SCStreamDelegate, SCStreamOutput, SCStreamOutputType};

use greffier_coeur::adaptateurs::{Autorisation, Autorisations, Capture, DemandeDeCapture, EtatDAutorisation, EtatDeCapture, Niveaux};

use crate::ecrivain::{Ecrivain, IMAGES_PAR_SECONDE};
use crate::mixage::{self, Mixeur, Source, FREQUENCE};

/// Taille maximale des images : 1080p (spec #1).
pub const LARGEUR_MAX: usize = 1920;
pub const HAUTEUR_MAX: usize = 1080;

/// La taille des images : celle de l'écran en pixels, réduite pour tenir dans
/// [`LARGEUR_MAX`] × [`HAUTEUR_MAX`] en gardant ses proportions, en dimensions paires.
pub fn taille_des_images(largeur: usize, hauteur: usize) -> (usize, usize) {
    let echelle = (LARGEUR_MAX as f64 / largeur as f64).min(HAUTEUR_MAX as f64 / hauteur as f64).min(1.0);
    let (l, h) = ((largeur as f64 * echelle).round() as usize, (hauteur as f64 * echelle).round() as usize);
    (l.min(LARGEUR_MAX) & !1, h.min(HAUTEUR_MAX) & !1)
}

/// Ce que la file de capture partage avec le fil de l'app.
struct Partage {
    etat: Mutex<Etat>,
    /// Secondes écrites (f64 en bits), niveaux (f32 en bits).
    duree: AtomicU64,
    micro: AtomicU32,
    son_du_mac: AtomicU32,
    ecran: bool,
    /// Au moins une source audio.
    audio: bool,
    arrete: AtomicBool,
    /// Le flux s'est arrêté de lui-même : l'erreur est dans `Etat::erreur`.
    en_panne: AtomicBool,
}

struct Etat {
    ecrivain: Option<Ecrivain>,
    mixeur: Mixeur,
    /// La dernière erreur du flux (arrêt imprévu).
    erreur: Option<String>,
}

impl Partage {
    fn recevoir(&self, tampon: &CMSampleBuffer, genre: SCStreamOutputType) {
        if self.arrete.load(Ordering::Relaxed) {
            return;
        }
        unsafe {
            if !tampon.is_valid() || !tampon.data_is_ready() {
                return;
            }
        }
        let t = unsafe { tampon.presentation_time_stamp() };
        if trace() {
            let n = unsafe { tampon.num_samples() };
            let image = genre == SCStreamOutputType::Screen && unsafe { tampon.image_buffer() }.is_some();
            eprintln!("capture {:?} t={:.4} echelle={} n={n} image={image}", genre.0, unsafe { t.seconds() }, t.timescale);
        }
        let Ok(mut etat) = self.etat.lock() else { return };
        let etat = &mut *etat;
        let Some(ecrivain) = etat.ecrivain.as_mut() else { return };
        if genre == SCStreamOutputType::Screen {
            if !self.ecran {
                return;
            }
            // Les images « au repos » (l'écran n'a pas changé) n'ont pas de pixels.
            if let Some(image) = unsafe { tampon.image_buffer() } {
                ecrivain.image(&image, t);
            }
            // Sans source audio, les pistes audio sont du silence, au rythme des images.
            if !self.audio {
                let origine = ecrivain.origine(t);
                let fin = unsafe { t.subtract(origine).seconds() } * f64::from(FREQUENCE);
                if let Some(bloc) = etat.mixeur.silence_jusqu_a(fin.max(0.0) as u64) {
                    ecrivain.audio(&bloc);
                }
            }
        } else {
            let source = if genre == SCStreamOutputType::Microphone { Source::Micro } else { Source::SonDuMac };
            let Some(trames) = pcm_stereo_48k(tampon) else { return };
            let niveau = mixage::niveau(&trames);
            let jauge = if source == Source::Micro { &self.micro } else { &self.son_du_mac };
            // Montée immédiate, descente douce.
            let avant = f32::from_bits(jauge.load(Ordering::Relaxed));
            jauge.store(niveau.max(avant * 0.8).to_bits(), Ordering::Relaxed);
            let origine = ecrivain.origine(t);
            let debut = unsafe { t.subtract(origine).seconds() } * f64::from(FREQUENCE);
            etat.mixeur.pousser(source, debut.round() as i64, &trames);
            while let Some(bloc) = etat.mixeur.tirer() {
                ecrivain.audio(&bloc);
            }
        }
        self.duree.store(ecrivain.duree().to_bits(), Ordering::Relaxed);
    }
}

/// `GREFFIER_CAPTURE_TRACE=1` : chaque tampon reçu et chaque bloc écrit, sur stderr.
pub(crate) fn trace() -> bool {
    static TRACE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *TRACE.get_or_init(|| std::env::var_os("GREFFIER_CAPTURE_TRACE").is_some())
}

/// Les échantillons d'un tampon audio, en stéréo entrelacée à 48 kHz.
fn pcm_stereo_48k(tampon: &CMSampleBuffer) -> Option<Vec<f32>> {
    unsafe {
        let format = tampon.format_description()?;
        let asbd = CMAudioFormatDescriptionGetStreamBasicDescription(&format).as_ref()?;
        if asbd.mFormatID != kAudioFormatLinearPCM {
            return None;
        }
        let flottant = asbd.mFormatFlags & kAudioFormatFlagIsFloat != 0;
        let separes = asbd.mFormatFlags & kAudioFormatFlagIsNonInterleaved != 0;
        let canaux = asbd.mChannelsPerFrame.max(1) as usize;
        let bits = asbd.mBitsPerChannel;
        if !(flottant && bits == 32 || !flottant && bits == 16) {
            return None;
        }
        let mut taille = 0usize;
        tampon.audio_buffer_list_with_retained_block_buffer(&mut taille, ptr::null_mut(), 0, None, None, 0, ptr::null_mut());
        if taille == 0 {
            return None;
        }
        // Un AudioBufferList de longueur variable, aligné.
        let mut place = vec![0u64; taille.div_ceil(8)];
        let liste = place.as_mut_ptr() as *mut AudioBufferList;
        let mut bloc: *mut CMBlockBuffer = ptr::null_mut();
        // kCMSampleBufferFlag_AudioBufferList_Assure16ByteAlignment
        let s = tampon.audio_buffer_list_with_retained_block_buffer(ptr::null_mut(), liste, taille, None, None, 1, &mut bloc);
        if s != 0 || bloc.is_null() {
            return None;
        }
        let _garde = CFRetained::from_raw(NonNull::new_unchecked(bloc));
        let n = (*liste).mNumberBuffers as usize;
        let tampons = std::slice::from_raw_parts((*liste).mBuffers.as_ptr(), n);
        let lire = |b: &objc2_core_audio_types::AudioBuffer| -> Vec<f32> {
            if b.mData.is_null() {
                return Vec::new();
            }
            if flottant {
                std::slice::from_raw_parts(b.mData as *const f32, b.mDataByteSize as usize / 4).to_vec()
            } else {
                std::slice::from_raw_parts(b.mData as *const i16, b.mDataByteSize as usize / 2).iter().map(|x| f32::from(*x) / 32768.0).collect()
            }
        };
        let entrelace: Vec<f32>;
        let canaux_lus;
        if separes && n > 1 {
            let plans: Vec<Vec<f32>> = tampons.iter().map(lire).collect();
            let trames = plans.iter().map(Vec::len).min().unwrap_or(0);
            canaux_lus = plans.len();
            entrelace = (0..trames).flat_map(|i| plans.iter().map(move |p| p[i])).collect();
        } else {
            entrelace = tampons.first().map(lire).unwrap_or_default();
            canaux_lus = if separes { 1 } else { canaux };
        }
        Some(mixage::en_stereo_48k(&entrelace, canaux_lus, asbd.mSampleRate))
    }
}

define_class!(
    // SAFETY : NSObject n'a pas d'exigence de sous-classe ; la classe ne surcharge pas dealloc.
    #[unsafe(super(NSObject))]
    #[name = "GreffierSortieDeCapture"]
    #[ivars = Arc<Partage>]
    struct Sortie;

    unsafe impl NSObjectProtocol for Sortie {}

    unsafe impl SCStreamOutput for Sortie {
        #[unsafe(method(stream:didOutputSampleBuffer:ofType:))]
        fn recu(&self, _flux: &SCStream, tampon: &CMSampleBuffer, genre: SCStreamOutputType) {
            self.ivars().recevoir(tampon, genre);
        }
    }

    unsafe impl SCStreamDelegate for Sortie {
        #[unsafe(method(stream:didStopWithError:))]
        fn arrete(&self, _flux: &SCStream, erreur: &NSError) {
            if let Ok(mut e) = self.ivars().etat.lock() {
                e.erreur = Some(erreur.localizedDescription().to_string());
            }
            self.ivars().en_panne.store(true, Ordering::Relaxed);
        }
    }
);

impl Sortie {
    fn new(partage: Arc<Partage>) -> Retained<Sortie> {
        let this = Sortie::alloc().set_ivars(partage);
        unsafe { msg_send![super(this), init] }
    }
}

struct EnCours {
    flux: Retained<SCStream>,
    _sortie: Retained<Sortie>,
    partage: Arc<Partage>,
    file: DispatchRetained<DispatchQueue>,
}

/// La vraie capture.
#[derive(Default)]
pub struct CaptureDuMac {
    en_cours: Option<EnCours>,
    /// L'autorisation de l'écran a été demandée pendant ce lancement : si elle manque encore,
    /// c'est un refus (macOS ne pose la question qu'une fois).
    ecran_demande: bool,
}

impl CaptureDuMac {
    pub fn new() -> CaptureDuMac {
        CaptureDuMac::default()
    }
}

fn attendre<T>(rx: mpsc::Receiver<T>, secondes: u64, quoi: &str) -> Result<T, String> {
    rx.recv_timeout(Duration::from_secs(secondes)).map_err(|_| format!("ScreenCaptureKit ne répond pas ({quoi})"))
}

/// Le contenu partageable (les écrans) ; échoue si l'enregistrement de l'écran n'est pas autorisé.
fn contenu_partageable() -> Result<Retained<SCShareableContent>, String> {
    let (tx, rx) = mpsc::channel();
    let fini = RcBlock::new(move |contenu: *mut SCShareableContent, erreur: *mut NSError| {
        let r = match unsafe { (Retained::retain(contenu), Retained::retain(erreur)) } {
            (Some(c), _) => Ok(SendRetained(c)),
            (None, Some(e)) => Err(e.localizedDescription().to_string()),
            (None, None) => Err("aucun contenu partageable".into()),
        };
        let _ = tx.send(r);
    });
    unsafe { SCShareableContent::getShareableContentWithCompletionHandler(&fini) };
    attendre(rx, 10, "écrans")?.map(|c| c.0)
}

/// Fait passer un objet d'un fil à l'autre (le bloc de complétion → le fil de l'app).
struct SendRetained<T>(Retained<T>);
unsafe impl<T> Send for SendRetained<T> {}

fn completion(appel: impl FnOnce(&block2::DynBlock<dyn Fn(*mut NSError)>), quoi: &str) -> Result<(), String> {
    let (tx, rx) = mpsc::channel();
    let fini = RcBlock::new(move |erreur: *mut NSError| {
        let _ = tx.send(unsafe { Retained::retain(erreur) }.map(|e| e.localizedDescription().to_string()));
    });
    appel(&fini);
    match attendre(rx, 10, quoi)? {
        None => Ok(()),
        Some(e) => Err(e),
    }
}

fn ouvrir_les_reglages(rubrique: &str) {
    let _ = std::process::Command::new("open").arg(format!("x-apple.systempreferences:com.apple.preference.security?{rubrique}")).spawn();
}

impl Capture for CaptureDuMac {
    fn autorisations(&self) -> Autorisations {
        let ecran = if CGPreflightScreenCaptureAccess() {
            EtatDAutorisation::Accordee
        } else if self.ecran_demande {
            EtatDAutorisation::Refusee
        } else {
            EtatDAutorisation::ADemander
        };
        let micro = match unsafe { AVCaptureDevice::authorizationStatusForMediaType(AVMediaTypeAudio.expect("audio")) } {
            AVAuthorizationStatus::Authorized => EtatDAutorisation::Accordee,
            AVAuthorizationStatus::NotDetermined => EtatDAutorisation::ADemander,
            _ => EtatDAutorisation::Refusee,
        };
        Autorisations { ecran, micro, son_du_mac: ecran }
    }

    fn demander_autorisation(&mut self, autorisation: Autorisation) {
        match (autorisation, self.autorisations().etat(autorisation)) {
            (_, EtatDAutorisation::Accordee) => {}
            (Autorisation::Ecran | Autorisation::SonDuMac, EtatDAutorisation::ADemander) => {
                CGRequestScreenCaptureAccess();
                self.ecran_demande = true;
            }
            (Autorisation::Ecran | Autorisation::SonDuMac, EtatDAutorisation::Refusee) => ouvrir_les_reglages("Privacy_ScreenCapture"),
            (Autorisation::Micro, EtatDAutorisation::ADemander) => {
                let rien = RcBlock::new(|_: Bool| {});
                unsafe { AVCaptureDevice::requestAccessForMediaType_completionHandler(AVMediaTypeAudio.expect("audio"), &rien) };
            }
            (Autorisation::Micro, EtatDAutorisation::Refusee) => ouvrir_les_reglages("Privacy_Microphone"),
        }
    }

    fn demarrer(&mut self, demande: DemandeDeCapture) -> Result<(), String> {
        if self.en_cours.is_some() {
            return Err("une capture est déjà en cours".into());
        }
        let sources = demande.sources;
        let contenu = contenu_partageable().map_err(|e| format!("Accès à l'écran impossible : {e}. Autorise Greffier dans Réglages Système › Confidentialité et sécurité › Enregistrement de l'écran et de l'audio du système, puis relance Greffier."))?;
        let ecrans = unsafe { contenu.displays() };
        let principal = CGMainDisplayID();
        let ecran: Retained<SCDisplay> = ecrans.iter().find(|d| unsafe { d.displayID() } == principal).or_else(|| ecrans.firstObject()).ok_or("aucun écran à enregistrer")?;
        let id = unsafe { ecran.displayID() };
        let (largeur, hauteur) = taille_des_images(CGDisplayPixelsWide(id), CGDisplayPixelsHigh(id));

        let ecrivain = Ecrivain::new(&demande.fichier, sources.ecran.then_some((largeur, hauteur)))?;
        let partage = Arc::new(Partage {
            etat: Mutex::new(Etat { ecrivain: Some(ecrivain), mixeur: Mixeur::new(sources.micro, sources.son_du_mac), erreur: None }),
            duree: AtomicU64::new(0),
            micro: AtomicU32::new(0),
            son_du_mac: AtomicU32::new(0),
            ecran: sources.ecran,
            audio: sources.micro || sources.son_du_mac,
            arrete: AtomicBool::new(false),
            en_panne: AtomicBool::new(false),
        });
        let sortie = Sortie::new(partage.clone());
        unsafe {
            let config = SCStreamConfiguration::new();
            if sources.ecran {
                config.setWidth(largeur);
                config.setHeight(hauteur);
                config.setMinimumFrameInterval(CMTime::new(1, IMAGES_PAR_SECONDE));
            } else {
                // ScreenCaptureKit veut un écran : une image minuscule par seconde, ignorée.
                config.setWidth(2);
                config.setHeight(2);
                config.setMinimumFrameInterval(CMTime::new(1, 1));
            }
            config.setPixelFormat(kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange);
            config.setShowsCursor(true);
            config.setQueueDepth(8);
            config.setCapturesAudio(sources.son_du_mac);
            config.setSampleRate(FREQUENCE as isize);
            config.setChannelCount(2);
            config.setExcludesCurrentProcessAudio(true);
            config.setCaptureMicrophone(sources.micro);

            let filtre = SCContentFilter::initWithDisplay_excludingWindows(SCContentFilter::alloc(), &ecran, &NSArray::new());
            let delegue = ProtocolObject::from_ref(&*sortie);
            let flux = SCStream::initWithFilter_configuration_delegate(SCStream::alloc(), &filtre, &config, Some(delegue));
            let file = DispatchQueue::new("io.github.fredealexandre.greffier.capture", None);
            let sortie_proto: &ProtocolObject<dyn SCStreamOutput> = ProtocolObject::from_ref(&*sortie);
            let mut genres = vec![SCStreamOutputType::Screen];
            if sources.son_du_mac {
                genres.push(SCStreamOutputType::Audio);
            }
            if sources.micro {
                genres.push(SCStreamOutputType::Microphone);
            }
            for genre in genres {
                flux.addStreamOutput_type_sampleHandlerQueue_error(sortie_proto, genre, Some(&file)).map_err(|e| format!("sortie de capture : {}", e.localizedDescription()))?;
            }
            completion(|b| flux.startCaptureWithCompletionHandler(Some(b)), "démarrage").map_err(|e| format!("La capture ne démarre pas : {e}"))?;
            self.en_cours = Some(EnCours { flux, _sortie: sortie, partage, file });
        }
        Ok(())
    }

    fn sonder(&mut self, _ecoule: f64) -> EtatDeCapture {
        let Some(c) = &self.en_cours else { return EtatDeCapture::default() };
        let p = &c.partage;
        // macOS a coupé le flux (écran débranché, autorisation retirée…) : le cœur finit le fichier.
        let erreur = if p.en_panne.load(Ordering::Relaxed) {
            let detail = p.etat.lock().ok().and_then(|e| e.erreur.clone()).unwrap_or_default();
            Some(format!("La capture s'est arrêtée : {detail}"))
        } else {
            None
        };
        EtatDeCapture {
            duree: f64::from_bits(p.duree.load(Ordering::Relaxed)),
            niveaux: Niveaux { micro: f32::from_bits(p.micro.load(Ordering::Relaxed)), son_du_mac: f32::from_bits(p.son_du_mac.load(Ordering::Relaxed)) },
            erreur,
        }
    }

    fn arreter(&mut self) -> Result<f64, String> {
        let c = self.en_cours.take().ok_or("aucune capture en cours")?;
        let arret = completion(|b| unsafe { c.flux.stopCaptureWithCompletionHandler(Some(b)) }, "arrêt");
        // Les derniers échantillons déjà dans la file passent, puis plus rien.
        let partage = c.partage.clone();
        let (tx, rx) = mpsc::channel();
        c.file.exec_sync(move || {
            partage.arrete.store(true, Ordering::Relaxed);
            let mut etat = partage.etat.lock().expect("état de la capture");
            let etat = &mut *etat;
            let mut ecrivain = etat.ecrivain.take();
            if let (Some(e), Some(bloc)) = (ecrivain.as_mut(), etat.mixeur.vider()) {
                e.audio(&bloc);
            }
            let _ = tx.send((ecrivain, etat.erreur.take()));
        });
        let (ecrivain, erreur) = rx.recv().map_err(|_| "capture perdue")?;
        let ecrivain = ecrivain.ok_or("capture déjà arrêtée")?;
        let duree = ecrivain.finir();
        if let Err(e) = &arret {
            eprintln!("greffier-capture : arrêt du flux : {e}");
        }
        if let Some(e) = erreur {
            eprintln!("greffier-capture : le flux s'était arrêté : {e}");
        }
        duree
    }
}

#[cfg(test)]
mod tests {
    use super::taille_des_images;

    #[test]
    fn les_images_tiennent_dans_1920_x_1080_en_gardant_les_proportions_et_en_dimensions_paires() {
        // 1080p : l'écran tient dans 1920 × 1080, proportions gardées.
        assert_eq!(taille_des_images(2880, 1800), (1728, 1080), "16:10");
        assert_eq!(taille_des_images(3024, 1964), (1662, 1080));
        assert_eq!(taille_des_images(5120, 2880), (1920, 1080), "16:9");
        assert_eq!(taille_des_images(3440, 1440), (1920, 804), "très large : la largeur limite");
        // Un écran plus petit garde sa taille, rendue paire.
        assert_eq!(taille_des_images(1920, 1080), (1920, 1080));
        assert_eq!(taille_des_images(1512, 983), (1512, 982));
    }
}
