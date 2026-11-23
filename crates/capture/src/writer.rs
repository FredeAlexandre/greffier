//! Writing the recording with AVAssetWriter, as it goes, as fragmented MP4 (one fragment every
//! [`FRAGMENT_INTERVAL`] seconds: a crash loses only the last one).
//!
//! Format taken from the vault: H.264 video at 30 fps, ~1200 kbps; three AAC audio tracks at
//! 160 kbps, 48 kHz, in this order: mix (the one transcription reads, first), microphone only, Mac
//! audio only. The three audio tracks form a group whose default track is the mix: a player plays
//! only that one instead of adding up all three.

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
    AVVideoAllowFrameReorderingKey, AVVideoAverageBitRateKey, AVVideoCodecKey, AVVideoCodecTypeH264, AVVideoCompressionPropertiesKey, AVVideoExpectedSourceFrameRateKey, AVVideoHeightKey, AVVideoMaxKeyFrameIntervalKey,
    AVVideoProfileLevelH264HighAutoLevel, AVVideoProfileLevelKey, AVVideoWidthKey,
};
use objc2_core_audio_types::{AudioStreamBasicDescription, kAudioFormatFlagIsFloat, kAudioFormatFlagIsPacked, kAudioFormatLinearPCM};
use objc2_core_foundation::CFRetained;
use objc2_core_media::{CMAudioFormatDescriptionCreate, CMAudioSampleBufferCreateReadyWithPacketDescriptions, CMBlockBuffer, CMFormatDescription, CMSampleBuffer, CMTime, CMTimeRoundingMethod, kCMBlockBufferAssureMemoryNowFlag};
use objc2_core_video::CVPixelBuffer;
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString, NSURL};

use crate::mixing::{AudioBlock, CHANNELS, SAMPLE_RATE};

pub const FRAMES_PER_SECOND: i32 = 30;
pub const VIDEO_BITRATE: i32 = 1_200_000;
pub const AUDIO_BITRATE: i32 = 160_000;
pub const FRAGMENT_INTERVAL: f64 = 2.0;
/// At most 4 frames (133 ms) wait for the audio; ScreenCaptureKit keeps 8 in reserve.
pub const PENDING_FRAMES: usize = 4;
/// 'aac '
const AAC_FORMAT: u32 = 0x6161_6320;

pub struct Writer {
    writer: Retained<AVAssetWriter>,
    video: Option<(Retained<AVAssetWriterInput>, Retained<AVAssetWriterInputPixelBufferAdaptor>)>,
    /// Mix, microphone, Mac audio.
    audio: [Retained<AVAssetWriterInput>; 3],
    audio_format: CFRetained<CMFormatDescription>,
    /// The timestamp of the first sample: the start of the recording.
    origin: Option<CMTime>,
    /// Seconds written since the origin.
    duration: f64,
    /// Samples rejected for lack of room (the encoder can't keep up).
    pub lost: u64,
    /// The writer's first failure.
    failure: Option<String>,
    /// The frames waiting for the audio, and the audio frames already written.
    /// (times in 1/90,000 s)
    pending: VecDeque<(CFRetained<CVPixelBuffer>, i64)>,
    audio_written: u64,
    /// The last frame, to repeat it when the screen doesn't change.
    last_frame: Option<(CFRetained<CVPixelBuffer>, i64)>,
}

// AVAssetWriter can be used from any thread, as long as it is one at a time: the writer lives
// behind a Mutex.
unsafe impl Send for Writer {}

fn key(s: &str) -> Retained<NSString> {
    NSString::from_str(s)
}

fn dictionary(pairs: &[(&NSString, Retained<AnyObject>)]) -> Retained<NSDictionary<NSString, AnyObject>> {
    let keys: Vec<&NSString> = pairs.iter().map(|(c, _)| *c).collect();
    let values: Vec<&AnyObject> = pairs.iter().map(|(_, v)| &**v).collect();
    NSDictionary::from_slices(&keys, &values)
}

fn number(n: f64) -> Retained<AnyObject> {
    object(&*NSNumber::new_f64(n))
}

fn integer(n: i64) -> Retained<AnyObject> {
    object(&*NSNumber::new_i64(n))
}

fn object<T: objc2::Message>(o: &T) -> Retained<AnyObject> {
    // SAFETY: every Objective-C object is an AnyObject.
    unsafe { Retained::retain(o as *const T as *mut AnyObject) }.expect("object")
}

/// An error's message, with its domain, its code and the underlying error.
pub fn error(e: &objc2_foundation::NSError) -> String {
    let mut m = format!("{} ({} {})", e.localizedDescription(), e.domain(), e.code());
    let info = e.userInfo();
    if let Some(underlying) = info.objectForKey(&key("NSUnderlyingError"))
        && let Ok(underlying) = underlying.downcast::<objc2_foundation::NSError>()
    {
        m.push_str(&format!(" ; {}", error(&underlying)));
    }
    m
}

impl Writer {
    /// Prepares the file. `video`: the frame size, if there is a video track. Frames wait for the
    /// audio: with no audio source, silence must be written ([`Mixer::silence_until`]).
    ///
    /// [`Mixer::silence_until`]: crate::mixing::Mixer::silence_until
    pub fn new(file: &Path, video: Option<(usize, usize)>) -> Result<Writer, String> {
        let url = NSURL::from_file_path(file).ok_or("chemin de l'enregistrement invalide")?;
        let _ = std::fs::remove_file(file);
        unsafe {
            let writer = AVAssetWriter::initWithURL_fileType_error(AVAssetWriter::alloc(), &url, AVFileTypeMPEG4.expect("AVFileTypeMPEG4")).map_err(|e| error(&e))?;
            writer.setShouldOptimizeForNetworkUse(false);
            writer.setMovieFragmentInterval(CMTime::with_seconds(FRAGMENT_INTERVAL, 600));

            let video = match video {
                Some((width, height)) => {
                    let compression = dictionary(&[
                        (AVVideoAverageBitRateKey.expect("key"), integer(i64::from(VIDEO_BITRATE))),
                        (AVVideoExpectedSourceFrameRateKey.expect("key"), integer(i64::from(FRAMES_PER_SECOND))),
                        (AVVideoMaxKeyFrameIntervalKey.expect("key"), integer(i64::from(2 * FRAMES_PER_SECOND))),
                        (AVVideoProfileLevelKey.expect("key"), object(AVVideoProfileLevelH264HighAutoLevel.expect("key"))),
                        // No B-frames: with them, writing a fragment fails at random (-11800 /
                        // -16341), about one 20 s recording in two.
                        (AVVideoAllowFrameReorderingKey.expect("key"), object(&*NSNumber::new_bool(false))),
                    ]);
                    let settings = dictionary(&[
                        (AVVideoCodecKey.expect("key"), object(AVVideoCodecTypeH264.expect("key"))),
                        (AVVideoWidthKey.expect("key"), integer(width as i64)),
                        (AVVideoHeightKey.expect("key"), integer(height as i64)),
                        (AVVideoCompressionPropertiesKey.expect("key"), object(&*compression)),
                    ]);
                    let input = AVAssetWriterInput::initWithMediaType_outputSettings(AVAssetWriterInput::alloc(), AVMediaTypeVideo.expect("video"), Some(&settings));
                    input.setExpectsMediaDataInRealTime(true);
                    if !writer.canAddInput(&input) {
                        return Err("piste vidéo refusée par AVAssetWriter".into());
                    }
                    writer.addInput(&input);
                    let adapter = AVAssetWriterInputPixelBufferAdaptor::assetWriterInputPixelBufferAdaptorWithAssetWriterInput_sourcePixelBufferAttributes(&input, None);
                    Some((input, adapter))
                }
                None => None,
            };

            let audio_settings = dictionary(&[
                (&key("AVFormatIDKey"), integer(i64::from(AAC_FORMAT))),
                (&key("AVSampleRateKey"), number(f64::from(SAMPLE_RATE))),
                (&key("AVNumberOfChannelsKey"), integer(CHANNELS as i64)),
                (&key("AVEncoderBitRateKey"), integer(i64::from(AUDIO_BITRATE))),
                // Constant bitrate, like OBS's AAC in the vault.
                (&key("AVEncoderBitRateStrategyKey"), object(&*key("AVAudioBitRateStrategy_Constant"))),
            ]);
            let track = || -> Result<Retained<AVAssetWriterInput>, String> {
                let input = AVAssetWriterInput::initWithMediaType_outputSettings(AVAssetWriterInput::alloc(), AVMediaTypeAudio.expect("audio"), Some(&audio_settings));
                input.setExpectsMediaDataInRealTime(true);
                if !writer.canAddInput(&input) {
                    return Err("piste audio refusée par AVAssetWriter".into());
                }
                writer.addInput(&input);
                Ok(input)
            };
            let audio = [track()?, track()?, track()?];
            let group = AVAssetWriterInputGroup::initWithInputs_defaultInput(AVAssetWriterInputGroup::alloc(), &NSArray::from_retained_slice(&audio), Some(&audio[0]));
            if writer.canAddInputGroup(&group) {
                writer.addInputGroup(&group);
            }

            let asbd = AudioStreamBasicDescription {
                mSampleRate: f64::from(SAMPLE_RATE),
                mFormatID: kAudioFormatLinearPCM,
                mFormatFlags: kAudioFormatFlagIsFloat | kAudioFormatFlagIsPacked,
                mBytesPerPacket: (4 * CHANNELS) as u32,
                mFramesPerPacket: 1,
                mBytesPerFrame: (4 * CHANNELS) as u32,
                mChannelsPerFrame: CHANNELS as u32,
                mBitsPerChannel: 32,
                mReserved: 0,
            };
            let mut format: *const CMFormatDescription = ptr::null();
            let status = CMAudioFormatDescriptionCreate(None, NonNull::from(&asbd), 0, ptr::null(), 0, ptr::null(), None, NonNull::from(&mut format));
            if status != 0 || format.is_null() {
                return Err(format!("format audio ({status})"));
            }
            let audio_format = CFRetained::from_raw(NonNull::new_unchecked(format as *mut CMFormatDescription));

            if !writer.startWriting() {
                return Err(writer.error().map_or("AVAssetWriter ne démarre pas".into(), |e| error(&e)));
            }
            Ok(Writer { writer, video, audio, audio_format, origin: None, duration: 0.0, lost: 0, failure: None, pending: VecDeque::new(), audio_written: 0, last_frame: None })
        }
    }

    /// The origin of the recording: set by the first sample received.
    pub fn origin(&mut self, t: CMTime) -> CMTime {
        let t = video_timescale(t);
        *self.origin.get_or_insert_with(|| {
            unsafe { self.writer.startSessionAtSourceTime(t) };
            t
        })
    }

    pub fn duration(&self) -> f64 {
        self.duration
    }

    /// The writer's first failure: nothing more will be written.
    pub fn failure(&self) -> Option<&str> {
        self.failure.as_deref()
    }

    fn note_end(&mut self, end: CMTime) {
        if let Some(o) = self.origin {
            let d = unsafe { end.subtract(o).seconds() };
            if d.is_finite() && d > self.duration {
                self.duration = d;
            }
        }
    }

    /// A screen frame, at its timestamp. Two rules, without which AVAssetWriter fails
    /// (-11800 / -16341) while writing a fragment:
    /// - the video has a fixed frame rate: ScreenCaptureKit sends nothing when the screen doesn't
    ///   change, so the last frame is repeated (like OBS);
    /// - a frame waits until the audio has caught up with it (at most [`PENDING_FRAMES`] frames).
    pub fn image(&mut self, image: &CVPixelBuffer, t: CMTime) {
        let t = video_timescale(t);
        let origin = self.origin(t);
        if unsafe { t.compare(origin) } < 0 || self.video.is_none() {
            return;
        }
        // SAFETY: one more reference on the buffer, for the wait and the repeats.
        let image = unsafe { CFRetained::retain(NonNull::from(image)) };
        // On the 30 fps grid: ScreenCaptureKit's timestamps are irregular, and two frames too
        // close together would share a decode timestamp in the file.
        let t = origin.value + (t.value - origin.value + PERIOD / 2) / PERIOD * PERIOD;
        match &mut self.last_frame {
            // This slot already has a frame (or the repeats have gone past it): the newest image
            // takes its place, if it is not written yet, and is used for the next repeats.
            Some((d, td)) if *td >= t => {
                if let Some((p, tp)) = self.pending.back_mut()
                    && *tp == *td
                {
                    *p = image.clone();
                }
                *d = image;
            }
            _ => {
                self.fill_gap(t - PERIOD);
                self.pending.push_back((image.clone(), t));
                self.last_frame = Some((image, t));
            }
        }
        self.write_frames(false);
    }

    /// Repeats the last frame, at a fixed frame rate, up to `until` (in 1/90,000 s).
    fn fill_gap(&mut self, until: i64) {
        let Some((image, t)) = &mut self.last_frame else { return };
        while *t + PERIOD <= until {
            *t += PERIOD;
            self.pending.push_back((image.clone(), *t));
        }
    }

    /// The time up to which the audio is written, in 1/90,000 s.
    fn audio_until(&self) -> Option<i64> {
        self.origin.map(|o| o.value + (self.audio_written as i64) * i64::from(VIDEO_TIMESCALE) / i64::from(SAMPLE_RATE))
    }

    /// Writes the frames the audio has caught up with (all of them if `all`).
    fn write_frames(&mut self, all: bool) {
        let audio = self.audio_until();
        while let Some((_, t)) = self.pending.front() {
            let caught_up = audio.is_some_and(|a| *t <= a);
            if !(all || caught_up || self.pending.len() > PENDING_FRAMES) {
                break;
            }
            let Some((image, t)) = self.pending.pop_front() else { break };
            let Some((input, adapter)) = &self.video else { return };
            let t = unsafe { CMTime::new(t, VIDEO_TIMESCALE) };
            let ok = unsafe { input.isReadyForMoreMediaData() && adapter.appendPixelBuffer_withPresentationTime(&image, t) };
            if ok {
                self.note_end(t);
            } else {
                self.lost("image", t);
            }
        }
    }

    /// The three tracks of a mixer block, placed at `origin + start / 48 kHz`.
    pub fn audio(&mut self, block: &AudioBlock) {
        let Some(origin) = self.origin else { return };
        let n = block.frames();
        if n == 0 {
            return;
        }
        let t = unsafe { origin.convert_scale(SAMPLE_RATE as i32, CMTimeRoundingMethod::RoundHalfAwayFromZero).add(CMTime::new(block.start as i64, SAMPLE_RATE as i32)) };
        for (i, data) in [&block.mixing, &block.micro, &block.mac_audio].into_iter().enumerate() {
            let Some(buffer) = samples(&self.audio_format, data, n, t) else {
                self.lost += 1;
                continue;
            };
            let input = &self.audio[i];
            let ok = unsafe { input.isReadyForMoreMediaData() && input.appendSampleBuffer(&buffer) };
            if !ok {
                self.lost("audio", t);
            }
        }
        self.note_end(unsafe { t.add(CMTime::new(n as i64, SAMPLE_RATE as i32)) });
        self.audio_written = block.start + n as u64;
        // Still screen: the video follows the audio, a little behind to let a real frame
        // arrive.
        if let Some(a) = self.audio_until() {
            self.fill_gap(a - REPEAT_DELAY);
        }
        self.write_frames(false);
    }

    /// Records a rejected sample; says why on the writer's first failure.
    fn lost(&mut self, what: &str, t: CMTime) {
        self.lost += 1;
        let status = unsafe { self.writer.status() };
        if status == AVAssetWriterStatus::Failed && self.failure.is_none() {
            let m = unsafe { self.writer.error() }.map_or("?".into(), |e| error(&e));
            eprintln!("greffier-capture: writing failed on {what} at {:.3} s: {m}", unsafe { t.seconds() });
            self.failure = Some(m);
        }
    }

    /// Closes the tracks and finishes the file; returns the duration written.
    pub fn finish(mut self) -> Result<f64, String> {
        if let Some(a) = self.audio_until() {
            self.fill_gap(a);
        }
        self.write_frames(true);
        if self.origin.is_none() {
            unsafe { self.writer.cancelWriting() };
            return Err("rien n'a été capturé".into());
        }
        unsafe {
            if let Some((input, _)) = &self.video {
                input.markAsFinished();
            }
            for input in &self.audio {
                input.markAsFinished();
            }
            let (tx, rx) = mpsc::channel();
            let on_done = RcBlock::new(move || {
                let _ = tx.send(());
            });
            self.writer.finishWritingWithCompletionHandler(&on_done);
            rx.recv_timeout(Duration::from_secs(60)).map_err(|_| "AVAssetWriter ne finit pas d'écrire")?;
            if self.writer.status() != AVAssetWriterStatus::Completed {
                return Err(self.writer.error().map_or("écriture de l'enregistrement en échec".into(), |e| error(&e)));
            }
        }
        Ok(self.duration)
    }
}

/// ScreenCaptureKit timestamps are in nanoseconds: at that timescale, a track duration overflows
/// 32 bits after 4.29 s and AVAssetWriter fails at the next fragment. Bring them down to 90 kHz.
fn video_timescale(t: CMTime) -> CMTime {
    if t.timescale == VIDEO_TIMESCALE {
        return t;
    }
    unsafe { t.convert_scale(VIDEO_TIMESCALE, CMTimeRoundingMethod::RoundHalfAwayFromZero) }
}

const VIDEO_TIMESCALE: i32 = 90_000;
/// One frame at 30 fps, in 1/90,000 s.
const PERIOD: i64 = VIDEO_TIMESCALE as i64 / FRAMES_PER_SECOND as i64;
/// The repeats follow the audio 100 ms behind.
const REPEAT_DELAY: i64 = VIDEO_TIMESCALE as i64 / 10;

/// A CoreMedia buffer of `n` interleaved floating-point stereo frames.
fn samples(format: &CMFormatDescription, data: &[f32], n: usize, t: CMTime) -> Option<CFRetained<CMSampleBuffer>> {
    let bytes = std::mem::size_of_val(data);
    unsafe {
        let mut block: *mut CMBlockBuffer = ptr::null_mut();
        let s = CMBlockBuffer::create_with_memory_block(None, ptr::null_mut(), bytes, None, ptr::null(), 0, bytes, kCMBlockBufferAssureMemoryNowFlag, NonNull::from(&mut block));
        if s != 0 || block.is_null() {
            return None;
        }
        let block = CFRetained::from_raw(NonNull::new_unchecked(block));
        if CMBlockBuffer::replace_data_bytes(NonNull::new_unchecked(data.as_ptr() as *mut _), &block, 0, bytes) != 0 {
            return None;
        }
        let mut buffer: *mut CMSampleBuffer = ptr::null_mut();
        let s = CMAudioSampleBufferCreateReadyWithPacketDescriptions(None, &block, format, n as isize, t, ptr::null(), NonNull::from(&mut buffer));
        if s != 0 || buffer.is_null() {
            return None;
        }
        Some(CFRetained::from_raw(NonNull::new_unchecked(buffer)))
    }
}
