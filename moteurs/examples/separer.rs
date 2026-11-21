//! Sépare les locuteurs d'un média avec pyannote dans le Python privé du dossier de l'app, sans
//! fenêtre : la vérification à la main de l'adaptateur pyannote.
//!
//!     cargo run --release -p greffier-moteurs --example separer -- <média> [--locuteurs N]
//!
//! Affiche les tours de parole (début, fin, locuteur) puis la durée.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use greffier_coeur::adaptateurs::DemandeDeSeparation;
use greffier_moteurs::SeparateurPyannote;

fn main() {
    let mut args = std::env::args().skip(1);
    let mut media = None;
    let mut nombre_de_locuteurs = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--locuteurs" => nombre_de_locuteurs = Some(args.next().and_then(|n| n.parse().ok()).expect("--locuteurs <nombre>")),
            _ => media = Some(PathBuf::from(a)),
        }
    }
    let media = media.expect("usage : separer <média> [--locuteurs N]");
    let moteur = SeparateurPyannote::de_l_app(&greffier_coeur::ressources::dossier_de_l_app());
    let demande = DemandeDeSeparation { audio: media, duree: 0.0, nombre_de_locuteurs };
    let t = Instant::now();
    let mut dernier = -1;
    let tours = moteur
        .separer_ici(&demande, &AtomicBool::new(false), &mut |p| {
            let pc = (p * 100.0) as i32;
            if pc / 10 != dernier / 10 {
                eprintln!("  {pc} %");
                dernier = pc;
            }
        })
        .unwrap_or_else(|e| panic!("séparation : {e}"));
    let mut locuteurs: Vec<&str> = tours.iter().map(|t| t.locuteur.as_str()).collect();
    locuteurs.sort_unstable();
    locuteurs.dedup();
    eprintln!("séparation : {} locuteurs, {} tours de parole en {:.1} s", locuteurs.len(), tours.len(), t.elapsed().as_secs_f64());
    for t in &tours {
        println!("{:8.2} {:8.2} {}", t.debut, t.fin, t.locuteur);
    }
}
