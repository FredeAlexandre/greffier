//! La vraie capture, sans l'interface :
//! `cargo run -p greffier-capture --example essai_capture` affiche l'état des autorisations (sans
//! rien demander) ; `-- --enregistrer sortie.mp4 [secondes]` enregistre l'écran, le son du Mac et
//! le micro (`--sans-ecran`, `--sans-micro`, `--sans-son` en retirent). Lancée d'un terminal, ce sont les autorisations du terminal qui comptent.

use greffier_capture::MacCapture;
use greffier_core::adapters::{Capture, CaptureRequest, Sources};

fn main() {
    let mut capture = MacCapture::new();
    println!("autorisations : {:?}", capture.permissions());
    let args: Vec<String> = std::env::args().collect();
    let Some(i) = args.iter().position(|a| a == "--enregistrer") else { return };
    let file = std::path::PathBuf::from(args.get(i + 1).expect("sortie.mp4"));
    let seconds: f64 = args.get(i + 2).and_then(|s| s.parse().ok()).unwrap_or(5.0);
    capture.start(CaptureRequest { file: file.clone(), sources: Sources { screen: !args.iter().any(|a| a == "--sans-ecran"), micro: !args.iter().any(|a| a == "--sans-micro"), mac_audio: !args.iter().any(|a| a == "--sans-son") } }).unwrap_or_else(|e| panic!("{e}"));
    let start = std::time::Instant::now();
    while start.elapsed().as_secs_f64() < seconds {
        std::thread::sleep(std::time::Duration::from_millis(500));
        let e = capture.poll(0.5);
        println!("{:5.1} s  micro {:.2}  son du Mac {:.2}", e.duration, e.levels.micro, e.levels.mac_audio);
    }
    let duration = capture.stop().unwrap_or_else(|e| panic!("{e}"));
    println!("{} : {duration:.2} s", file.display());
}
