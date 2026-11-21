//! Essai à la main du vrai bilan par la CLI `claude` (modèle opus, quelques minutes, payant) sur
//! une réunion inventée : enregistrement factice, transcription écrite à la main, relecture qui ne
//! change rien, puis le vrai bilan. Le dossier de réunions donné est créé s'il manque.
//!
//! `cargo run -p greffier-coeur --example bilan -- <dossier de réunions> [<dossier de contexte>…]`

use std::path::PathBuf;
use std::time::{Duration, Instant};

use greffier_coeur::adaptateurs::*;
use greffier_coeur::claude::{AgentClaude, ClaudeDuMac, relecture::Reecoute};
use greffier_coeur::faux::{FausseCapture, FausseHorloge, FausseTache, FaussesRessources, FauxSeparateur, FauxTranscripteur};
use greffier_coeur::{Commande, DateHeure, Evenement, Greffier, Mot, Reglages, Statut, TourDeParole};

/// Le vrai bilan, avec une relecture qui ne change rien.
struct BilanSeul(AgentClaude);

/// Pas de réécoute : la relecture n'est pas jouée ici.
struct SansReecoute;

impl Reecoute for SansReecoute {
    fn reecouter(&self, _: &std::path::Path, extraits: &[(f64, f64)], _: &str, _: &std::sync::atomic::AtomicBool) -> Result<Vec<String>, String> {
        Ok(vec![String::new(); extraits.len()])
    }
}

impl Agent for BilanSeul {
    fn relire(&mut self, _: DemandeDeRelecture) -> TacheDe<ResultatDeRelecture> {
        FausseTache::boite(0.0, Ok(ResultatDeRelecture::default()))
    }
    fn faire_le_bilan(&mut self, d: DemandeDeBilan) -> TacheDe<ResultatDuBilan> {
        self.0.faire_le_bilan(d)
    }
}

const REPLIQUES: [(&str, &str); 8] = [
    ("SPEAKER_00", "Bonjour Bastien, on fait le point sur le salon de mars ?"),
    ("SPEAKER_01", "Oui Odile. Le stand est réservé, il reste à choisir le traiteur."),
    ("SPEAKER_00", "On avait deux devis, Les Halles Gourmandes à 2 400 euros et Saveurs du Port à 3 100."),
    ("SPEAKER_01", "Je propose Les Halles Gourmandes, la qualité était bonne l'an dernier."),
    ("SPEAKER_00", "D'accord, on prend Les Halles Gourmandes. Tu leur confirmes avant vendredi ?"),
    ("SPEAKER_01", "Oui, je les appelle jeudi. Par contre, pour les badges, on ne sait pas encore combien de visiteurs."),
    ("SPEAKER_00", "Je demande les chiffres à l'organisateur lundi et je te les envoie."),
    ("SPEAKER_01", "Parfait. Et on garde la date du 14 mars pour la répétition."),
];

fn main() {
    let mut args = std::env::args_os().skip(1).map(PathBuf::from);
    let Some(dossier) = args.next() else {
        eprintln!("usage : bilan <dossier de réunions> [<dossier de contexte>…]");
        std::process::exit(2);
    };
    let contexte: Vec<PathBuf> = args.collect();
    std::fs::create_dir_all(&dossier).expect("dossier de réunions");

    let (mut mots, mut tours) = (Vec::<Mot>::new(), Vec::<TourDeParole>::new());
    let mut t = 0.0;
    for (locuteur, texte) in REPLIQUES {
        let debut = t;
        for m in texte.split(' ') {
            mots.push(Mot { debut: t, fin: t + 0.35, texte: m.into(), confiance: 0.9, locuteur: None });
            t += 0.4;
        }
        tours.push(TourDeParole { debut, fin: t, locuteur: locuteur.into() });
        t += 1.0;
    }
    let duree = t;
    let adaptateurs = Adaptateurs {
        capture: Box::new(FausseCapture::new(20.0, 0.0)),
        transcripteur: Box::new(FauxTranscripteur::fixe(0.1, mots)),
        separateur: Box::new(FauxSeparateur::fixe(0.1, tours)),
        agent: Box::new(BilanSeul(AgentClaude::new(std::sync::Arc::new(ClaudeDuMac::default()), std::sync::Arc::new(SansReecoute)))),
        ressources: Box::new(FaussesRessources::pretes()),
        horloge: Box::new(FausseHorloge(DateHeure::new(2026, 11, 21, 16, 30))),
        corbeille: Box::new(greffier_coeur::faux::FausseCorbeille::new()),
    };
    let mut g = Greffier::ouvrir(&dossier, adaptateurs);
    g.executer(Commande::ChangerReglages(Reglages { dossiers_de_contexte: contexte, ..Reglages::default() })).expect("réglages");
    g.executer(Commande::DemarrerEnregistrement).expect("enregistrement");
    while g.enregistrement().is_some_and(|e| e.duree < duree) {
        g.avancer(0.1);
    }
    let id = g.executer(Commande::ArreterEnregistrement).expect("arrêt").iter().find_map(|e| if let Evenement::ReunionEnregistree(id) = e { Some(*id) } else { None }).expect("réunion");

    let debut = Instant::now();
    loop {
        for e in g.avancer(0.1) {
            println!("{e:?}");
        }
        let r = g.reunion(id).expect("réunion");
        match &r.statut {
            Statut::Terminee => break,
            Statut::Echec { message, .. } => {
                eprintln!("échec : {message}");
                std::process::exit(1);
            }
            _ => std::thread::sleep(Duration::from_millis(100)),
        }
    }
    let r = g.reunion(id).expect("réunion");
    println!("\n{} s · dossier : {}", debut.elapsed().as_secs(), g.dossier().dossier(&r.nom_du_dossier).display());
    println!("titre : {}\nrésumé : {}\nactions : {:?}\nsession : {:?}", r.titre, r.resume, r.actions_principales, r.sessions.bilan);
}
