//! Rejoue une trace de capture (`GREFFIER_CAPTURE_TRACE=1` : ordre d'arrivée, horodatages,
//! tailles des tampons) à travers le vrai mixeur et le vrai écrivain, avec des images et un son
//! synthétiques. Sert à reproduire sans écran ni autorisation un échec d'écriture vu en vrai :
//! `cargo run -p greffier-capture --example rejouer -- trace.txt sortie.mp4`.

use std::ptr::{self, NonNull};
use std::time::Duration;

use greffier_capture::ecrivain::Ecrivain;
use greffier_capture::mixage::{CANAUX, FREQUENCE, Mixeur, Source};
use objc2_core_foundation::CFRetained;
use objc2_core_media::CMTime;
use objc2_core_video::{CVPixelBuffer, CVPixelBufferCreate, kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let trace = std::fs::read_to_string(args.get(1).expect("trace.txt")).expect("trace");
    let sortie = std::path::PathBuf::from(args.get(2).expect("sortie.mp4"));
    let (l, h) = (1710, 1106);
    let mut e = Ecrivain::new(&sortie, Some((l, h))).expect("écrivain");
    let mut m = Mixeur::new(true, true);
    let mut p: *mut CVPixelBuffer = ptr::null_mut();
    unsafe { assert_eq!(CVPixelBufferCreate(None, l, h, kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange, None, NonNull::from(&mut p)), 0) };
    let image = unsafe { CFRetained::from_raw(NonNull::new_unchecked(p)) };
    let mut precedent: Option<f64> = None;
    for ligne in trace.lines() {
        // capture <genre> t=<secondes> echelle=<échelle> n=<échantillons> image=<bool>
        let Some(reste) = ligne.strip_prefix("capture ") else { continue };
        let champ = |nom: &str| reste.split_whitespace().find_map(|c| c.strip_prefix(nom)).unwrap_or_default().to_string();
        let genre: u8 = reste.split_whitespace().next().and_then(|g| g.parse().ok()).unwrap_or(0);
        let t: f64 = champ("t=").parse().expect("t");
        let echelle: i32 = champ("echelle=").parse().unwrap_or(1_000_000_000);
        let n: usize = champ("n=").parse().expect("n");
        // Au rythme de la capture, pour que l'encodeur ait le temps qu'il aurait eu.
        if let Some(pr) = precedent
            && t > pr
        {
            std::thread::sleep(Duration::from_secs_f64((t - pr).min(0.1)));
        }
        precedent = Some(precedent.map_or(t, |x| x.max(t)));
        let cm = unsafe { CMTime::new((t * f64::from(echelle)).round() as i64, echelle) };
        if genre == 0 {
            if champ("image=") == "true" {
                e.image(&image, cm);
            }
            continue;
        }
        let source = if genre == 2 { Source::Micro } else { Source::SonDuMac };
        let origine = e.origine(cm);
        let debut = unsafe { cm.subtract(origine).seconds() } * f64::from(FREQUENCE);
        m.pousser(source, debut.round() as i64, &vec![0.01; n * CANAUX]);
        while let Some(b) = m.tirer() {
            e.audio(&b);
        }
    }
    if let Some(b) = m.vider() {
        e.audio(&b);
    }
    println!("{:?}", e.finir());
}
