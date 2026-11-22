//! The real capture, without the UI:
//! `cargo run -p greffier-capture --example try_capture` shows the permission states (without
//! requesting anything); `-- --record output.mp4 [seconds]` records the screen, the Mac audio and
//! the microphone (`--no-screen`, `--no-micro`, `--no-mac-audio` leave them out). Run from a terminal, the terminal's permissions are the ones that count.

use greffier_capture::MacCapture;
use greffier_core::adapters::{Capture, CaptureRequest, Sources};

fn main() {
    let mut capture = MacCapture::new();
    println!("permissions: {:?}", capture.permissions());
    let args: Vec<String> = std::env::args().collect();
    let Some(i) = args.iter().position(|a| a == "--record") else { return };
    let file = std::path::PathBuf::from(args.get(i + 1).expect("output.mp4"));
    let seconds: f64 = args.get(i + 2).and_then(|s| s.parse().ok()).unwrap_or(5.0);
    capture.start(CaptureRequest { file: file.clone(), sources: Sources { screen: !args.iter().any(|a| a == "--no-screen"), micro: !args.iter().any(|a| a == "--no-micro"), mac_audio: !args.iter().any(|a| a == "--no-mac-audio") } }).unwrap_or_else(|e| panic!("{e}"));
    let start = std::time::Instant::now();
    while start.elapsed().as_secs_f64() < seconds {
        std::thread::sleep(std::time::Duration::from_millis(500));
        let e = capture.poll(0.5);
        println!("{:5.1} s  micro {:.2}  Mac audio {:.2}", e.duration, e.levels.micro, e.levels.mac_audio);
    }
    let duration = capture.stop().unwrap_or_else(|e| panic!("{e}"));
    println!("{}: {duration:.2} s", file.display());
}
