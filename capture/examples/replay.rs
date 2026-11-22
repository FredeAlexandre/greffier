//! Replays a capture trace (`GREFFIER_CAPTURE_TRACE=1`: arrival order, timestamps, buffer sizes)
//! through the real mixer and the real writer, with synthetic frames and sound. Used to reproduce,
//! without a screen or permissions, a write failure seen for real:
//! `cargo run -p greffier-capture --example replay -- trace.txt output.mp4`.

use std::ptr::{self, NonNull};
use std::time::Duration;

use greffier_capture::writer::Writer;
use greffier_capture::mixing::{CHANNELS, SAMPLE_RATE, Mixer, Source};
use objc2_core_foundation::CFRetained;
use objc2_core_media::CMTime;
use objc2_core_video::{CVPixelBuffer, CVPixelBufferCreate, kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let trace = std::fs::read_to_string(args.get(1).expect("trace.txt")).expect("trace");
    let output = std::path::PathBuf::from(args.get(2).expect("output.mp4"));
    let (l, h) = (1710, 1106);
    let mut e = Writer::new(&output, Some((l, h))).expect("writer");
    let mut m = Mixer::new(true, true);
    let mut p: *mut CVPixelBuffer = ptr::null_mut();
    unsafe { assert_eq!(CVPixelBufferCreate(None, l, h, kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange, None, NonNull::from(&mut p)), 0) };
    let image = unsafe { CFRetained::from_raw(NonNull::new_unchecked(p)) };
    let mut previous: Option<f64> = None;
    for line in trace.lines() {
        // capture <kind> t=<seconds> timescale=<timescale> n=<samples> image=<bool>
        let Some(rest) = line.strip_prefix("capture ") else { continue };
        let field = |name: &str| rest.split_whitespace().find_map(|c| c.strip_prefix(name)).unwrap_or_default().to_string();
        let kind: u8 = rest.split_whitespace().next().and_then(|g| g.parse().ok()).unwrap_or(0);
        let t: f64 = field("t=").parse().expect("t");
        let timescale: i32 = field("timescale=").parse().unwrap_or(1_000_000_000);
        let n: usize = field("n=").parse().expect("n");
        // At the capture's pace, so the encoder gets the time it would have had.
        if let Some(pr) = previous
            && t > pr
        {
            std::thread::sleep(Duration::from_secs_f64((t - pr).min(0.1)));
        }
        previous = Some(previous.map_or(t, |x| x.max(t)));
        let cm = unsafe { CMTime::new((t * f64::from(timescale)).round() as i64, timescale) };
        if kind == 0 {
            if field("image=") == "true" {
                e.image(&image, cm);
            }
            continue;
        }
        let source = if kind == 2 { Source::Micro } else { Source::MacAudio };
        let origin = e.origin(cm);
        let start = unsafe { cm.subtract(origin).seconds() } * f64::from(SAMPLE_RATE);
        m.push(source, start.round() as i64, &vec![0.01; n * CHANNELS]);
        while let Some(b) = m.pull() {
            e.audio(&b);
        }
    }
    if let Some(b) = m.drain() {
        e.audio(&b);
    }
    println!("{:?}", e.finish());
}
