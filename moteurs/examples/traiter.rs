//! Importe un média dans un dossier de réunions et le fait transcrire par le cœur avec les vrais
//! moteurs (whisper.cpp et pyannote, en parallèle), sans fenêtre : la vérification à la main de
//! l'étape Transcription de bout en bout. S'arrête quand la transcription est écrite.
//!
//!     cargo run --release -p greffier-moteurs --example traiter -- <dossier de réunions> <média> [--locuteurs N]
//!
//! Puis `GREFFIER_DOSSIER=<dossier de réunions> cargo run` pour voir la réunion dans l'app.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use greffier_coeur::adaptateurs::Adaptateurs;
use greffier_coeur::dossier::Import;
use greffier_coeur::faux::FaussesRessources;
use greffier_coeur::{Commande, DateHeure, Etape, Evenement, Greffier, Statut};
use greffier_moteurs::{SeparateurPyannote, TranscripteurWhisper};

fn main() {
    let mut args = std::env::args().skip(1);
    let mut positionnels = Vec::new();
    let mut nombre = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--locuteurs" => nombre = Some(args.next().and_then(|n| n.parse().ok()).expect("--locuteurs <nombre>")),
            _ => positionnels.push(PathBuf::from(a)),
        }
    }
    let [dossier, media] = <[PathBuf; 2]>::try_from(positionnels).expect("usage : traiter <dossier de réunions> <média> [--locuteurs N]");
    std::fs::create_dir_all(&dossier).expect("dossier de réunions");
    let app = greffier_coeur::ressources::dossier_de_l_app();
    let adaptateurs = Adaptateurs {
        transcripteur: Box::new(TranscripteurWhisper::de_l_app(&app)),
        separateur: Box::new(SeparateurPyannote::de_l_app(&app)),
        ressources: Box::new(FaussesRessources::pretes()),
        ..greffier_coeur::fictif::adaptateurs()
    };
    let mut greffier = Greffier::ouvrir(&dossier, adaptateurs);
    let titre = media.file_stem().map_or("Réunion".into(), |s| s.to_string_lossy().into_owned());
    let import = Import { enregistrement: media, transcription: None, bilan: None, debut: DateHeure::new(2026, 11, 21, 10, 0), titre };
    let ev = greffier.executer(Commande::Importer(import)).expect("import");
    let id = ev.iter().find_map(|e| if let Evenement::ReunionImportee(id) = e { Some(*id) } else { None }).expect("réunion importée");
    greffier.executer(Commande::NombreDeLocuteurs { reunion: id, nombre }).expect("nombre de locuteurs");
    greffier.executer(Commande::Traiter(id)).expect("traiter");

    let t = Instant::now();
    let mut dernier = -1;
    loop {
        greffier.avancer(0.1);
        let r = greffier.reunions().iter().find(|r| r.id == id).expect("réunion");
        match &r.statut {
            Statut::EnCours { etape: Etape::Transcription, progres } => {
                let pc = (progres * 100.0) as i32;
                if pc / 10 != dernier / 10 {
                    eprintln!("  {pc} %");
                    dernier = pc;
                }
            }
            Statut::Echec { message, .. } if r.etape_faite.is_none() => panic!("transcription : {message}"),
            _ if r.etape_faite.is_some() => break,
            _ => {}
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let r = greffier.reunions().iter().find(|r| r.id == id).expect("réunion");
    eprintln!("transcription : {} mots, {} locuteurs, {} tours, {} répliques en {:.1} s → {}", r.mots.len(), r.locuteurs.len(), r.tours.len(), r.repliques.len(), t.elapsed().as_secs_f64(), r.nom_du_dossier);
    for x in &r.repliques {
        println!("[{:7.2}] {} : {}", x.debut, r.locuteurs[x.locuteur].etiquette, x.texte);
    }
    // La relecture qui suit n'est pas l'objet de cet essai : arrêtée, la réunion reste « à traiter »
    // après sa transcription.
    let _ = greffier.executer(Commande::ArreterTraitement(id));
}
