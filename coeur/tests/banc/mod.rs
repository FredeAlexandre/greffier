//! Le banc de test du cœur : un dossier de réunions dans un répertoire temporaire, des faux
//! adaptateurs nourris de données écrites à la main, des commandes, puis on regarde l'état et les
//! fichiers. Aucun test ne dépend de la structure interne du cœur.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use greffier_coeur::adaptateurs::{Adaptateurs, Participant, ResultatDeRelecture, ResultatDuBilan};
use greffier_coeur::faux::{FausseCapture, FausseCorbeille, FausseHorloge, FaussesRessources, FauxAgent, FauxSeparateur, FauxTranscripteur};
use greffier_coeur::{Commande, DateHeure, Etape, Evenement, Greffier, IdReunion, Mot, Reunion, Statut, TourDeParole};

/// Durée simulée de chaque étape des faux adaptateurs, en secondes.
pub const DUREE_D_ETAPE: f64 = 1.0;

pub const BILAN: &str = "## En bref\n\nAlexandre salue l'équipe.\n\n## Participants\n\n- **SPEAKER_00 — Alexandre**\n- **SPEAKER_01 — Khalid**\n\n## Sujets abordés\n\n### Salutations (0:00:00)\n\n- Bonjour.\n\n## Décisions\n\n- (0:00:03) On se revoit demain.\n\n## Actions\n\n- [ ] **Alexandre** — préparer la démo — lundi\n- [ ] **Khalid** — relire la PR\n\n## Questions ouvertes\n\n- Aucune.\n";

pub fn mot(debut: f64, fin: f64, texte: &str) -> Mot {
    Mot { debut, fin, texte: texte.into(), confiance: 0.9, locuteur: None }
}

pub fn tour(debut: f64, fin: f64, locuteur: &str) -> TourDeParole {
    TourDeParole { debut, fin, locuteur: locuteur.into() }
}

/// Deux répliques : SPEAKER_00 dit bonjour, SPEAKER_01 répond.
pub fn mots() -> Vec<Mot> {
    vec![mot(0.0, 0.5, "Bonjour"), mot(0.5, 0.8, "à"), mot(0.8, 1.5, "tous."), mot(3.0, 3.4, "Salut"), mot(3.4, 4.0, "Alexandre.")]
}

pub fn tours() -> Vec<TourDeParole> {
    vec![tour(0.0, 1.6, "SPEAKER_00"), tour(2.9, 4.1, "SPEAKER_01")]
}

pub fn resultat_du_bilan() -> ResultatDuBilan {
    ResultatDuBilan {
        session: Some("session-bilan".into()),
        titre: "Point d'équipe : démo".into(),
        resume: "Alexandre salue l'équipe.".into(),
        actions: vec!["Alexandre — préparer la démo — lundi".into(), "Khalid — relire la PR".into()],
        participants: vec![
            Participant { locuteur: "SPEAKER_00".into(), nom_propose: "Alexandre".into(), justification: "Khalid l'appelle Alexandre à 0:00:03.".into(), meme_voix_que: None },
            Participant { locuteur: "SPEAKER_01".into(), nom_propose: "Khalid".into(), justification: "Répond à Alexandre.".into(), meme_voix_que: None },
        ],
    }
}

/// Un agent qui ne corrige rien et écrit le bilan donné.
pub fn agent(bilan: &'static str) -> FauxAgent {
    FauxAgent::new(
        DUREE_D_ETAPE,
        |_| Ok(ResultatDeRelecture::default()),
        DUREE_D_ETAPE,
        move |_| Ok((bilan.to_string(), resultat_du_bilan())),
    )
}

pub fn adaptateurs() -> Adaptateurs {
    Adaptateurs {
        capture: Box::new(FausseCapture::new(1.0, 0.0)),
        transcripteur: Box::new(FauxTranscripteur::fixe(DUREE_D_ETAPE, mots())),
        separateur: Box::new(FauxSeparateur::fixe(DUREE_D_ETAPE, tours())),
        agent: Box::new(agent(BILAN)),
        ressources: Box::new(FaussesRessources::pretes()),
        horloge: Box::new(FausseHorloge(DateHeure::new(2026, 11, 21, 14, 2))),
        corbeille: Box::new(FausseCorbeille::new()),
    }
}

pub struct Banc {
    pub greffier: Greffier,
    pub dossier: PathBuf,
}

impl Banc {
    pub fn new() -> Banc {
        Banc::avec(adaptateurs())
    }

    pub fn avec(adaptateurs: Adaptateurs) -> Banc {
        let dossier = dossier_temporaire();
        let greffier = Greffier::ouvrir(&dossier, adaptateurs);
        Banc { greffier, dossier }
    }

    /// Le cœur sur une copie du dossier de réunions d'exemple (`tests/fixtures/dossier-de-reunions`).
    pub fn sur_les_fixtures() -> Banc {
        let dossier = dossier_temporaire();
        copier(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/dossier-de-reunions"), &dossier);
        let greffier = Greffier::ouvrir(&dossier, adaptateurs());
        Banc { greffier, dossier }
    }

    /// Relance le cœur sur le même dossier de réunions, comme au lancement suivant de l'app.
    pub fn rouvrir(&mut self) {
        self.greffier = Greffier::ouvrir(&self.dossier, adaptateurs());
    }

    /// La réunion dont le dossier porte ce nom.
    pub fn reunion_nommee(&self, nom: &str) -> &Reunion {
        let noms: Vec<&str> = self.greffier.reunions().iter().map(|r| r.nom_du_dossier.as_str()).collect();
        self.greffier.reunions().iter().find(|r| r.nom_du_dossier == nom).unwrap_or_else(|| panic!("pas de réunion « {nom} » parmi {noms:?}"))
    }

    pub fn executer(&mut self, commande: Commande) -> Vec<Evenement> {
        let nom = format!("{commande:?}");
        self.greffier.executer(commande).unwrap_or_else(|e| panic!("{nom} refusée : {e}"))
    }

    /// Avance le temps par pas de 0,1 s, comme le ferait l'interface.
    pub fn avancer(&mut self, secondes: f64) -> Vec<Evenement> {
        let mut evenements = Vec::new();
        let pas = (secondes / 0.1).round() as usize;
        for _ in 0..pas {
            evenements.extend(self.greffier.avancer(0.1));
        }
        evenements
    }

    /// Avance jusqu'à ce que `fin` soit vraie (au plus 60 s), en notant les étapes traversées.
    pub fn avancer_jusqu_a(&mut self, id: IdReunion, fin: impl Fn(&Statut) -> bool) -> Vec<Etape> {
        let mut etapes = Vec::new();
        for _ in 0..600 {
            if let Statut::EnCours { etape, .. } = self.reunion(id).statut
                && etapes.last() != Some(&etape)
            {
                etapes.push(etape);
            }
            if fin(&self.reunion(id).statut) {
                return etapes;
            }
            self.greffier.avancer(0.1);
        }
        panic!("toujours {:?} après 60 s", self.reunion(id).statut);
    }

    /// Enregistre une réunion de `duree` secondes (fausse capture au temps réel), sans la traiter.
    pub fn enregistrer(&mut self, duree: f64) -> IdReunion {
        self.executer(Commande::DemarrerEnregistrement);
        self.avancer(duree);
        self.enregistrer_la_suite()
    }

    /// Arrête l'enregistrement en cours ; rend la réunion enregistrée.
    pub fn enregistrer_la_suite(&mut self) -> IdReunion {
        let evenements = self.executer(Commande::ArreterEnregistrement);
        evenements
            .iter()
            .find_map(|e| match e {
                Evenement::ReunionEnregistree(id) => Some(*id),
                _ => None,
            })
            .expect("la réunion enregistrée")
    }

    /// Une réunion enregistrée et pas encore traitée.
    pub fn reunion_a_traiter(&mut self) -> IdReunion {
        self.executer(Commande::ChangerReglages(greffier_coeur::Reglages { traiter_a_l_arret: false, ..self.greffier.reglages().clone() }));
        self.enregistrer(5.0)
    }

    pub fn reunion(&self, id: IdReunion) -> &Reunion {
        self.greffier.reunion(id).expect("réunion connue")
    }

    /// Le cache de recherche à côté du dossier de réunions (supprimé avec lui), donné au cœur
    /// comme l'app le fait au lancement avec celui de son dossier.
    pub fn utiliser_un_cache_de_recherche(&mut self) -> PathBuf {
        let cache = self.dossier.with_extension("recherche");
        self.executer(Commande::ChangerReglages(greffier_coeur::Reglages { cache_de_recherche: Some(cache.clone()), ..self.greffier.reglages().clone() }));
        cache
    }

    pub fn lire(&self, chemin: &Path) -> String {
        std::fs::read_to_string(chemin).unwrap_or_else(|e| panic!("{} : {e}", chemin.display()))
    }
}

fn dossier_temporaire() -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let dossier = std::env::temp_dir().join(format!("greffier-banc-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    let _ = std::fs::remove_dir_all(&dossier);
    std::fs::create_dir_all(&dossier).expect("dossier temporaire");
    dossier
}

fn copier(de: &Path, vers: &Path) {
    std::fs::create_dir_all(vers).expect("copie des fixtures");
    for entree in std::fs::read_dir(de).expect("fixtures") {
        let entree = entree.expect("fixture");
        let cible = vers.join(entree.file_name());
        if entree.file_type().expect("type").is_dir() {
            copier(&entree.path(), &cible);
        } else {
            std::fs::copy(entree.path(), cible).expect("copie d'une fixture");
        }
    }
}

impl Drop for Banc {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dossier);
        let _ = std::fs::remove_dir_all(self.dossier.with_extension("recherche"));
    }
}
