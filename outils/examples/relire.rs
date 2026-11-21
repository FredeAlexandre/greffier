//! Essai à la main de la relecture par la vraie CLI `claude`, sur une petite transcription
//! inventée, sans réécoute (les extraits rendent « rien d'exploitable »).
//!
//! `cargo run -p greffier-outils --example relire [-- <répertoire de travail>]`

use std::path::Path;
use std::sync::atomic::AtomicBool;

use greffier_coeur::DateHeure;
use greffier_coeur::adaptateurs::{DemandeDeRelecture, RepliquePourLAgent};
use greffier_outils::claude::ClaudeDuMac;
use greffier_outils::claude::relecture::{self, Reecoute};

struct SansReecoute;

impl Reecoute for SansReecoute {
    fn reecouter(&self, _: &Path, extraits: &[(f64, f64)], _: &str, _: &AtomicBool) -> Result<Vec<String>, String> {
        eprintln!("réécoute demandée : {extraits:?}");
        Ok(vec![String::new(); extraits.len()])
    }
}

fn main() {
    let repertoire = std::env::args().nth(1).map(Into::into).unwrap_or_else(std::env::temp_dir);
    let r = |numero, locuteur: &str, debut, fin, texte: &str| RepliquePourLAgent { numero, locuteur: locuteur.into(), nom: None, debut, fin, texte: texte.into() };
    let demande = DemandeDeRelecture {
        enregistrement: "/nulle/part.mp4".into(),
        dossier_de_reunions: repertoire,
        nom_du_dossier: "2026-11-21 14h02 - Point d'équipe".into(),
        titre: "Point d'équipe".into(),
        debut: DateHeure::new(2026, 11, 21, 14, 2),
        nombre_de_locuteurs: Some(2),
        repliques: vec![
            r(0, "SPEAKER_00", 0.0, 4.0, "Bonjour Bob, on fait le point sur la mise en prod de Zorglube ?"),
            r(1, "SPEAKER_01", 4.5, 9.0, "Oui, la recète est terminée, il reste le ticket sur le cache redisse."),
            r(2, "SPEAKER_01", 9.5, 12.0, "Et le, euh, le flux de paie mangue chez le client ?"),
            r(3, "SPEAKER_01", 12.5, 15.0, "Il est bloqué, on attend leur retour jeudi."),
        ],
        lexique: greffier_coeur::lexique::LEXIQUE_INITIAL.into(),
        vocabulaire: String::new(),
    };
    let mut progres = |p: f64| eprintln!("palier {p}");
    match relecture::relire(&ClaudeDuMac::default(), &SansReecoute, &demande, &AtomicBool::new(false), &mut progres) {
        Ok(resultat) => println!("{resultat:#?}"),
        Err(e) => {
            eprintln!("échec : {e}");
            std::process::exit(1);
        }
    }
}
