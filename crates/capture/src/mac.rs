//! [`MacCapture`]: capture through ScreenCaptureKit, a single stream for the main screen, the Mac
//! audio (all system audio except Greffier's own) and the default microphone (macOS 15 and later).
//! Samples arrive on a serial dispatch queue; the writer and the mixer live behind a Mutex that
//! only this queue touches during the recording.
//!
//! Permissions: the screen and the Mac audio depend on the same macOS permission ("Screen & System
//! Audio Recording"); the microphone on its own. They can only be granted to an installed, signed
//! `Greffier.app` (`cargo xtask install-app`), not through `cargo run` (the terminal asks for them
//! then).

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

use greffier_core::adapters::{Permission, Permissions, Capture, CaptureRequest, PermissionState, CaptureState, Levels};

use crate::writer::{Writer, FRAMES_PER_SECOND};
use crate::mixing::{self, Mixer, Source, SAMPLE_RATE};

/// Maximum frame size: 1080p (spec #1).
pub const MAX_WIDTH: usize = 1920;
pub const MAX_HEIGHT: usize = 1080;

/// The frame size: the screen's size in pixels, scaled down to fit in [`MAX_WIDTH`] ×
/// [`MAX_HEIGHT`] keeping its aspect ratio, with even dimensions.
pub fn image_size(width: usize, height: usize) -> (usize, usize) {
    let timescale = (MAX_WIDTH as f64 / width as f64).min(MAX_HEIGHT as f64 / height as f64).min(1.0);
    let (l, h) = ((width as f64 * timescale).round() as usize, (height as f64 * timescale).round() as usize);
    (l.min(MAX_WIDTH) & !1, h.min(MAX_HEIGHT) & !1)
}

/// What the capture queue shares with the app thread.
struct Shared {
    state: Mutex<State>,
    /// Seconds written (f64 as bits), levels (f32 as bits).
    duration: AtomicU64,
    micro: AtomicU32,
    mac_audio: AtomicU32,
    screen: bool,
    /// At least one audio source.
    audio: bool,
    stopped: AtomicBool,
    /// The stream stopped on its own: the error is in `State::error`.
    broken: AtomicBool,
}

struct State {
    writer: Option<Writer>,
    mixer: Mixer,
    /// The stream's last error (unexpected stop).
    error: Option<String>,
}

impl Shared {
    fn receive(&self, buffer: &CMSampleBuffer, kind: SCStreamOutputType) {
        if self.stopped.load(Ordering::Relaxed) {
            return;
        }
        unsafe {
            if !buffer.is_valid() || !buffer.data_is_ready() {
                return;
            }
        }
        let t = unsafe { buffer.presentation_time_stamp() };
        let Ok(mut state) = self.state.lock() else { return };
        let state = &mut *state;
        let Some(writer) = state.writer.as_mut() else { return };
        if kind == SCStreamOutputType::Screen {
            if !self.screen {
                return;
            }
            // "Idle" frames (the screen hasn't changed) have no pixels.
            if let Some(image) = unsafe { buffer.image_buffer() } {
                writer.image(&image, t);
            }
            // With no audio source, the audio tracks are silence, at the pace of the frames.
            if !self.audio {
                let origin = writer.origin(t);
                let end = unsafe { t.subtract(origin).seconds() } * f64::from(SAMPLE_RATE);
                if let Some(block) = state.mixer.silence_until(end.max(0.0) as u64) {
                    writer.audio(&block);
                }
            }
        } else {
            let source = if kind == SCStreamOutputType::Microphone { Source::Micro } else { Source::MacAudio };
            let Some(frames) = pcm_stereo_48k(buffer) else { return };
            let level = mixing::level(&frames);
            let gauge = if source == Source::Micro { &self.micro } else { &self.mac_audio };
            // Rise immediately, fall gently.
            let before = f32::from_bits(gauge.load(Ordering::Relaxed));
            gauge.store(level.max(before * 0.8).to_bits(), Ordering::Relaxed);
            let origin = writer.origin(t);
            let start = unsafe { t.subtract(origin).seconds() } * f64::from(SAMPLE_RATE);
            state.mixer.push(source, start.round() as i64, &frames);
            while let Some(block) = state.mixer.pull() {
                writer.audio(&block);
            }
        }
        self.duration.store(writer.duration().to_bits(), Ordering::Relaxed);
        // The file can no longer be written: stop now, so the user knows during the meeting
        // instead of at the end.
        if let Some(f) = writer.failure()
            && !self.broken.load(Ordering::Relaxed)
        {
            state.error = Some(format!("le fichier ne peut plus être écrit ({f})"));
            self.broken.store(true, Ordering::Relaxed);
        }
    }
}

/// The samples of an audio buffer, as interleaved stereo at 48 kHz.
fn pcm_stereo_48k(buffer: &CMSampleBuffer) -> Option<Vec<f32>> {
    unsafe {
        let format = buffer.format_description()?;
        let asbd = CMAudioFormatDescriptionGetStreamBasicDescription(&format).as_ref()?;
        if asbd.mFormatID != kAudioFormatLinearPCM {
            return None;
        }
        let is_float = asbd.mFormatFlags & kAudioFormatFlagIsFloat != 0;
        let non_interleaved = asbd.mFormatFlags & kAudioFormatFlagIsNonInterleaved != 0;
        let channels = asbd.mChannelsPerFrame.max(1) as usize;
        let bits = asbd.mBitsPerChannel;
        if !(is_float && bits == 32 || !is_float && bits == 16) {
            return None;
        }
        let mut size = 0usize;
        buffer.audio_buffer_list_with_retained_block_buffer(&mut size, ptr::null_mut(), 0, None, None, 0, ptr::null_mut());
        if size == 0 {
            return None;
        }
        // A variable-length, aligned AudioBufferList.
        let mut storage = vec![0u64; size.div_ceil(8)];
        let list = storage.as_mut_ptr() as *mut AudioBufferList;
        let mut block: *mut CMBlockBuffer = ptr::null_mut();
        // kCMSampleBufferFlag_AudioBufferList_Assure16ByteAlignment
        let s = buffer.audio_buffer_list_with_retained_block_buffer(ptr::null_mut(), list, size, None, None, 1, &mut block);
        if s != 0 || block.is_null() {
            return None;
        }
        let _guard = CFRetained::from_raw(NonNull::new_unchecked(block));
        let n = (*list).mNumberBuffers as usize;
        let buffers = std::slice::from_raw_parts((*list).mBuffers.as_ptr(), n);
        let read = |b: &objc2_core_audio_types::AudioBuffer| -> Vec<f32> {
            if b.mData.is_null() {
                return Vec::new();
            }
            if is_float {
                std::slice::from_raw_parts(b.mData as *const f32, b.mDataByteSize as usize / 4).to_vec()
            } else {
                std::slice::from_raw_parts(b.mData as *const i16, b.mDataByteSize as usize / 2).iter().map(|x| f32::from(*x) / 32768.0).collect()
            }
        };
        let interleaved: Vec<f32>;
        let channels_read;
        if non_interleaved && n > 1 {
            let planes: Vec<Vec<f32>> = buffers.iter().map(read).collect();
            let frames = planes.iter().map(Vec::len).min().unwrap_or(0);
            channels_read = planes.len();
            interleaved = (0..frames).flat_map(|i| planes.iter().map(move |p| p[i])).collect();
        } else {
            interleaved = buffers.first().map(read).unwrap_or_default();
            channels_read = if non_interleaved { 1 } else { channels };
        }
        Some(mixing::to_stereo_48k(&interleaved, channels_read, asbd.mSampleRate))
    }
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements; the class doesn't override dealloc.
    #[unsafe(super(NSObject))]
    #[name = "GreffierCaptureOutput"]
    #[ivars = Arc<Shared>]
    struct Output;

    unsafe impl NSObjectProtocol for Output {}

    unsafe impl SCStreamOutput for Output {
        #[unsafe(method(stream:didOutputSampleBuffer:ofType:))]
        fn received(&self, _stream: &SCStream, buffer: &CMSampleBuffer, kind: SCStreamOutputType) {
            self.ivars().receive(buffer, kind);
        }
    }

    unsafe impl SCStreamDelegate for Output {
        #[unsafe(method(stream:didStopWithError:))]
        fn stopped(&self, _stream: &SCStream, error: &NSError) {
            if let Ok(mut e) = self.ivars().state.lock() {
                e.error = Some(error.localizedDescription().to_string());
            }
            self.ivars().broken.store(true, Ordering::Relaxed);
        }
    }
);

impl Output {
    fn new(shared: Arc<Shared>) -> Retained<Output> {
        let this = Output::alloc().set_ivars(shared);
        unsafe { msg_send![super(this), init] }
    }
}

struct Running {
    stream: Retained<SCStream>,
    _output: Retained<Output>,
    shared: Arc<Shared>,
    queue: DispatchRetained<DispatchQueue>,
}

/// The real capture.
#[derive(Default)]
pub struct MacCapture {
    running: Option<Running>,
    /// The screen permission was requested during this launch: if it is still missing, it was
    /// denied (macOS asks only once).
    screen_requested: bool,
}

impl MacCapture {
    pub fn new() -> MacCapture {
        MacCapture::default()
    }
}

fn wait<T>(rx: mpsc::Receiver<T>, seconds: u64, what: &str) -> Result<T, String> {
    rx.recv_timeout(Duration::from_secs(seconds)).map_err(|_| format!("ScreenCaptureKit ne répond pas ({what})"))
}

/// The shareable content (the screens); fails if screen recording is not allowed.
fn shareable_content() -> Result<Retained<SCShareableContent>, String> {
    let (tx, rx) = mpsc::channel();
    let on_done = RcBlock::new(move |content: *mut SCShareableContent, error: *mut NSError| {
        let r = match unsafe { (Retained::retain(content), Retained::retain(error)) } {
            (Some(c), _) => Ok(SendRetained(c)),
            (None, Some(e)) => Err(e.localizedDescription().to_string()),
            (None, None) => Err("aucun contenu partageable".into()),
        };
        let _ = tx.send(r);
    });
    unsafe { SCShareableContent::getShareableContentWithCompletionHandler(&on_done) };
    wait(rx, 10, "écrans")?.map(|c| c.0)
}

/// Moves an object from one thread to another (the completion block → the app thread).
struct SendRetained<T>(Retained<T>);
unsafe impl<T> Send for SendRetained<T> {}

fn completion(call: impl FnOnce(&block2::DynBlock<dyn Fn(*mut NSError)>), what: &str) -> Result<(), String> {
    let (tx, rx) = mpsc::channel();
    let on_done = RcBlock::new(move |error: *mut NSError| {
        let _ = tx.send(unsafe { Retained::retain(error) }.map(|e| e.localizedDescription().to_string()));
    });
    call(&on_done);
    match wait(rx, 10, what)? {
        None => Ok(()),
        Some(e) => Err(e),
    }
}

fn open_settings(section: &str) {
    let _ = std::process::Command::new("open").arg(format!("x-apple.systempreferences:com.apple.preference.security?{section}")).spawn();
}

impl Capture for MacCapture {
    fn permissions(&self) -> Permissions {
        let screen = if CGPreflightScreenCaptureAccess() {
            PermissionState::Granted
        } else if self.screen_requested {
            PermissionState::Denied
        } else {
            PermissionState::ToRequest
        };
        let micro = match unsafe { AVCaptureDevice::authorizationStatusForMediaType(AVMediaTypeAudio.expect("audio")) } {
            AVAuthorizationStatus::Authorized => PermissionState::Granted,
            AVAuthorizationStatus::NotDetermined => PermissionState::ToRequest,
            _ => PermissionState::Denied,
        };
        Permissions { screen, micro, mac_audio: screen }
    }

    fn request_permission(&mut self, permission: Permission) {
        match (permission, self.permissions().state(permission)) {
            (_, PermissionState::Granted) => {}
            (Permission::Screen | Permission::MacAudio, PermissionState::ToRequest) => {
                CGRequestScreenCaptureAccess();
                self.screen_requested = true;
            }
            (Permission::Screen | Permission::MacAudio, PermissionState::Denied) => open_settings("Privacy_ScreenCapture"),
            (Permission::Micro, PermissionState::ToRequest) => {
                let nothing = RcBlock::new(|_: Bool| {});
                unsafe { AVCaptureDevice::requestAccessForMediaType_completionHandler(AVMediaTypeAudio.expect("audio"), &nothing) };
            }
            (Permission::Micro, PermissionState::Denied) => open_settings("Privacy_Microphone"),
        }
    }

    fn start(&mut self, request: CaptureRequest) -> Result<(), String> {
        if self.running.is_some() {
            return Err("une capture est déjà en cours".into());
        }
        let sources = request.sources;
        let content = shareable_content().map_err(|e| format!("Accès à l'écran impossible : {e}. Autorise Greffier dans Réglages Système › Confidentialité et sécurité › Enregistrement de l'écran et de l'audio du système, puis relance Greffier."))?;
        let screens = unsafe { content.displays() };
        let main_display = CGMainDisplayID();
        let screen: Retained<SCDisplay> = screens.iter().find(|d| unsafe { d.displayID() } == main_display).or_else(|| screens.firstObject()).ok_or("aucun écran à enregistrer")?;
        let id = unsafe { screen.displayID() };
        let (width, height) = image_size(CGDisplayPixelsWide(id), CGDisplayPixelsHigh(id));

        let writer = Writer::new(&request.file, sources.screen.then_some((width, height)))?;
        let shared = Arc::new(Shared {
            state: Mutex::new(State { writer: Some(writer), mixer: Mixer::new(sources.micro, sources.mac_audio), error: None }),
            duration: AtomicU64::new(0),
            micro: AtomicU32::new(0),
            mac_audio: AtomicU32::new(0),
            screen: sources.screen,
            audio: sources.micro || sources.mac_audio,
            stopped: AtomicBool::new(false),
            broken: AtomicBool::new(false),
        });
        let output = Output::new(shared.clone());
        unsafe {
            let config = SCStreamConfiguration::new();
            if sources.screen {
                config.setWidth(width);
                config.setHeight(height);
                config.setMinimumFrameInterval(CMTime::new(1, FRAMES_PER_SECOND));
            } else {
                // ScreenCaptureKit wants a screen: one tiny frame per second, ignored.
                config.setWidth(2);
                config.setHeight(2);
                config.setMinimumFrameInterval(CMTime::new(1, 1));
            }
            config.setPixelFormat(kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange);
            config.setShowsCursor(true);
            config.setQueueDepth(8);
            config.setCapturesAudio(sources.mac_audio);
            config.setSampleRate(SAMPLE_RATE as isize);
            config.setChannelCount(2);
            config.setExcludesCurrentProcessAudio(true);
            config.setCaptureMicrophone(sources.micro);

            let filter = SCContentFilter::initWithDisplay_excludingWindows(SCContentFilter::alloc(), &screen, &NSArray::new());
            let delegate = ProtocolObject::from_ref(&*output);
            let stream = SCStream::initWithFilter_configuration_delegate(SCStream::alloc(), &filter, &config, Some(delegate));
            let queue = DispatchQueue::new("io.github.fredealexandre.greffier.capture", None);
            let output_proto: &ProtocolObject<dyn SCStreamOutput> = ProtocolObject::from_ref(&*output);
            let mut kinds = vec![SCStreamOutputType::Screen];
            if sources.mac_audio {
                kinds.push(SCStreamOutputType::Audio);
            }
            if sources.micro {
                kinds.push(SCStreamOutputType::Microphone);
            }
            for kind in kinds {
                stream.addStreamOutput_type_sampleHandlerQueue_error(output_proto, kind, Some(&queue)).map_err(|e| format!("sortie de capture : {}", e.localizedDescription()))?;
            }
            completion(|b| stream.startCaptureWithCompletionHandler(Some(b)), "démarrage").map_err(|e| format!("La capture ne démarre pas : {e}"))?;
            self.running = Some(Running { stream, _output: output, shared, queue });
        }
        Ok(())
    }

    fn poll(&mut self, _elapsed: f64) -> CaptureState {
        let Some(c) = &self.running else { return CaptureState::default() };
        let p = &c.shared;
        // macOS cut the stream (screen unplugged, permission revoked…): the core finishes the file.
        let error = if p.broken.load(Ordering::Relaxed) {
            let detail = p.state.lock().ok().and_then(|e| e.error.clone()).unwrap_or_default();
            Some(format!("La capture s'est arrêtée : {detail}"))
        } else {
            None
        };
        CaptureState {
            duration: f64::from_bits(p.duration.load(Ordering::Relaxed)),
            levels: Levels { micro: f32::from_bits(p.micro.load(Ordering::Relaxed)), mac_audio: f32::from_bits(p.mac_audio.load(Ordering::Relaxed)) },
            error,
        }
    }

    fn stop(&mut self) -> Result<f64, String> {
        let c = self.running.take().ok_or("aucune capture en cours")?;
        let stop = completion(|b| unsafe { c.stream.stopCaptureWithCompletionHandler(Some(b)) }, "arrêt");
        // The last samples already in the queue go through, then nothing more.
        let shared = c.shared.clone();
        let (tx, rx) = mpsc::channel();
        c.queue.exec_sync(move || {
            shared.stopped.store(true, Ordering::Relaxed);
            let mut state = shared.state.lock().expect("capture state");
            let state = &mut *state;
            let mut writer = state.writer.take();
            if let (Some(e), Some(block)) = (writer.as_mut(), state.mixer.drain()) {
                e.audio(&block);
            }
            let _ = tx.send((writer, state.error.take()));
        });
        let (writer, error) = rx.recv().map_err(|_| "capture perdue")?;
        let writer = writer.ok_or("capture déjà arrêtée")?;
        let duration = writer.finish();
        if let Err(e) = &stop {
            eprintln!("greffier-capture: stopping the stream: {e}");
        }
        if let Some(e) = error {
            eprintln!("greffier-capture: the stream had stopped: {e}");
        }
        duration
    }
}

#[cfg(test)]
mod tests {
    use super::image_size;

    #[test]
    fn frames_fit_in_1920_x_1080_keeping_aspect_ratio_and_even_dimensions() {
        // 1080p: the screen fits in 1920 × 1080, aspect ratio kept.
        assert_eq!(image_size(2880, 1800), (1728, 1080), "16:10");
        assert_eq!(image_size(3024, 1964), (1662, 1080));
        assert_eq!(image_size(5120, 2880), (1920, 1080), "16:9");
        assert_eq!(image_size(3440, 1440), (1920, 804), "very wide: the width is the limit");
        // A smaller screen keeps its size, made even.
        assert_eq!(image_size(1920, 1080), (1920, 1080));
        assert_eq!(image_size(1512, 983), (1512, 982));
    }
}
