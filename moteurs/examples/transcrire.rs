//! Transcrit un média avec les vrais moteurs et les modèles du dossier de l'app, sans fenêtre :
//! la vérification à la main de l'adaptateur whisper.cpp.
//!
//!     cargo run --release -p greffier-moteurs --example transcrire -- <média> [--vocabulaire "…"] [--wav sortie.wav]
//!
//! Affiche les mots (début, fin, confiance) puis la durée de chaque partie. `--wav` écrit l'audio
//! converti par AVFoundation, pour le comparer à celui de ffmpeg.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use greffier_coeur::adaptateurs::DemandeDeTranscription;
use greffier_moteurs::{TranscripteurWhisper, audio};

fn main() {
    let mut args = std::env::args().skip(1);
    let mut media = None;
    let mut vocabulaire = String::new();
    let mut wav = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--vocabulaire" => vocabulaire = args.next().expect("--vocabulaire <phrase>"),
            "--wav" => wav = Some(PathBuf::from(args.next().expect("--wav <fichier>"))),
            _ => media = Some(PathBuf::from(a)),
        }
    }
    let media = media.expect("usage : transcrire <média> [--vocabulaire \"…\"] [--wav sortie.wav]");
    let arret = AtomicBool::new(false);

    let t = Instant::now();
    let echantillons = audio::mixage(&media, &arret, |_| {}).unwrap_or_else(|e| panic!("audio : {e}"));
    eprintln!("audio : {:.2} s de son en {:.2} s", echantillons.len() as f64 / audio::FREQUENCE as f64, t.elapsed().as_secs_f64());
    if let Some(w) = wav {
        audio::ecrire_wav(&w, &echantillons).expect("wav");
    }

    let moteur = TranscripteurWhisper::de_l_app(&greffier_coeur::ressources::dossier_de_l_app());
    let demande = DemandeDeTranscription { audio: media, duree: 0.0, vocabulaire };
    let t = Instant::now();
    let mut dernier = -1;
    let mots = moteur
        .transcrire_ici(&demande, &arret, &mut |p| {
            let pc = (p * 100.0) as i32;
            if pc / 10 != dernier / 10 {
                eprintln!("  {pc} %");
                dernier = pc;
            }
        })
        .unwrap_or_else(|e| panic!("transcription : {e}"));
    eprintln!("transcription : {} mots en {:.1} s", mots.len(), t.elapsed().as_secs_f64());
    for m in &mots {
        println!("{:8.2} {:8.2} {:.3} {}", m.debut, m.fin, m.confiance, m.texte);
    }
}
