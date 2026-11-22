//! Writes a synthetic recording (a moving square, an A on the microphone, an E on the Mac audio
//! with silences) through the real writer, without ScreenCaptureKit or permissions:
//! `cargo run -p greffier-capture --example try_writer -- output.mp4 [seconds] [--cut]`.
//! `--cut` quits without finishing the file, like a crash, to check that it stays readable.

use std::ptr::{self, NonNull};

use greffier_capture::writer::Writer;
use greffier_capture::mixing::{Mixer, Source, CHANNELS, SAMPLE_RATE};
use objc2_core_foundation::CFRetained;
use objc2_core_media::CMTime;
use objc2_core_video::{kCVPixelFormatType_32BGRA, CVPixelBuffer, CVPixelBufferCreate, CVPixelBufferGetBaseAddress, CVPixelBufferGetBytesPerRow, CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let output = std::path::PathBuf::from(args.get(1).expect("output.mp4"));
    let seconds: f64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(6.0);
    let cut = args.iter().any(|a| a == "--cut");
    let (l, h) = (1280usize, 720usize);
    let mut e = Writer::new(&output, Some((l, h))).expect("writer");
    let mut m = Mixer::new(true, true);
    let base = 1_000_000_000_000i64; // an arbitrary clock timestamp, in ns
    let packet = 1024usize;
    let mut audio_frame = 0usize;
    let images = (seconds * 30.0) as usize;
    for i in 0..images {
        let t = i as f64 / 30.0;
        let pts = unsafe { CMTime::new(base + (t * 1e9) as i64, 1_000_000_000) };
        e.image(&image(l, h, i), pts);
        // The audio up to this frame, in packets of 1024 frames.
        while (audio_frame as f64) / f64::from(SAMPLE_RATE) < t + 1.0 / 30.0 {
            let tone = |f: f64, k: usize| ((2.0 * std::f64::consts::PI * f * (audio_frame + k) as f64 / f64::from(SAMPLE_RATE)).sin() * 0.3) as f32;
            let micro: Vec<f32> = (0..packet).flat_map(|k| [tone(440.0, k); CHANNELS]).collect();
            m.push(Source::Micro, audio_frame as i64, &micro);
            // The Mac audio goes quiet every other second.
            if (audio_frame / SAMPLE_RATE as usize).is_multiple_of(2) {
                let mac: Vec<f32> = (0..packet).flat_map(|k| [tone(660.0, k); CHANNELS]).collect();
                m.push(Source::MacAudio, audio_frame as i64, &mac);
            }
            audio_frame += packet;
            while let Some(b) = m.pull() {
                e.audio(&b);
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(33));
        if cut && t >= seconds - 1.0 {
            eprintln!("cut at {t:.1} s without finishing");
            std::process::exit(0);
        }
    }
    if let Some(b) = m.drain() {
        e.audio(&b);
    }
    let lost = e.lost;
    let duration = e.finish().expect("finish");
    println!("duration written {duration:.2} s, samples lost {lost}");
}

fn image(l: usize, h: usize, i: usize) -> CFRetained<CVPixelBuffer> {
    unsafe {
        let mut p: *mut CVPixelBuffer = ptr::null_mut();
        assert_eq!(CVPixelBufferCreate(None, l, h, kCVPixelFormatType_32BGRA, None, NonNull::from(&mut p)), 0);
        let p = CFRetained::from_raw(NonNull::new_unchecked(p));
        CVPixelBufferLockBaseAddress(&p, CVPixelBufferLockFlags(0));
        let base = CVPixelBufferGetBaseAddress(&p) as *mut u8;
        let line = CVPixelBufferGetBytesPerRow(&p);
        let x0 = (i * 8) % (l - 100);
        for y in 0..h {
            for x in 0..l {
                let o = base.add(y * line + x * 4);
                let inside = x >= x0 && x < x0 + 100 && (300..400).contains(&y);
                let (b, g, r) = if inside { (0u8, 140u8, 255u8) } else { (40, 40, 40) };
                *o = b;
                *o.add(1) = g;
                *o.add(2) = r;
                *o.add(3) = 255;
            }
        }
        CVPixelBufferUnlockBaseAddress(&p, CVPixelBufferLockFlags(0));
        p
    }
}
