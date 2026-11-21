//! La séparation des locuteurs : le nombre de locuteurs indiqué avant le traitement, la
//! transcription et la séparation en parallèle, puis l'attribution de chaque mot à un locuteur
//! selon les règles du vault (décalage de +0,2 s, interruptions rendues, recalage sur les fins de
//! phrase) et les coupures de réplique. Mots et tours de parole écrits à la main ; noms inventés.

mod banc;

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use banc::{Banc, mot, tour};
use greffier_coeur::dossier::Import;
use greffier_coeur::faux::{FauxSeparateur, FauxTranscripteur};
use greffier_coeur::{Commande, DateHeure, Etape, Evenement, IdReunion, Mot, Reunion, Role, Statut, TourDeParole};

fn video() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/dossier-de-reunions/2026-11-19 15h30 - Réunion/2026-11-19 15h30 - Réunion - enregistrement.mp4")
}

fn importer_un_media(banc: &mut Banc) -> IdReunion {
    let import = Import { enregistrement: video(), transcription: None, bilan: None, debut: DateHeure::new(2026, 10, 2, 9, 30), titre: "Point Ormeau".into() };
    let ev = banc.executer(Commande::Importer(import));
    ev.iter().find_map(|e| if let Evenement::ReunionImportee(id) = e { Some(*id) } else { None }).expect("réunion importée")
}

fn traiter_la_transcription(banc: &mut Banc, id: IdReunion) {
    banc.executer(Commande::Traiter(id));
    banc.avancer_jusqu_a(id, |s| matches!(s, Statut::EnCours { etape: Etape::Relecture, .. } | Statut::Echec { .. } | Statut::Terminee));
}

/// Transcrit ces mots avec ces tours de parole et rend la réunion.
fn transcrire(mots: Vec<Mot>, tours: Vec<TourDeParole>) -> (Banc, IdReunion) {
    let mut a = banc::adaptateurs();
    a.transcripteur = Box::new(FauxTranscripteur::fixe(1.0, mots));
    a.separateur = Box::new(FauxSeparateur::fixe(1.0, tours));
    let mut banc = Banc::avec(a);
    let id = importer_un_media(&mut banc);
    traiter_la_transcription(&mut banc, id);
    (banc, id)
}

/// L'étiquette du locuteur de chaque mot.
fn locuteurs_des_mots(r: &Reunion) -> Vec<String> {
    r.mots.iter().map(|m| m.locuteur.map_or("?".into(), |i| r.locuteurs[i].etiquette.clone())).collect()
}

/// « SPEAKER_00 : texte » pour chaque réplique.
fn repliques(r: &Reunion) -> Vec<String> {
    r.repliques.iter().map(|x| format!("{} : {}", r.locuteurs[x.locuteur].etiquette, x.texte)).collect()
}

/// Des mots qui se suivent, 0,4 s chacun, à partir de `debut`.
fn phrase(debut: f64, texte: &str) -> Vec<Mot> {
    texte.split(' ').enumerate().map(|(i, t)| mot(debut + 0.4 * i as f64, debut + 0.4 * (i + 1) as f64, t)).collect()
}

fn fichier(banc: &Banc, id: IdReunion, role: Role) -> String {
    let r = banc.reunion(id);
    banc.lire(&banc.greffier.dossier().fichier(&r.nom_du_dossier, role))
}

// --- Nombre de locuteurs -------------------------------------------------------------------

#[test]
fn le_nombre_de_locuteurs_indique_avant_le_traitement_est_garde_dans_la_fiche_et_donne_au_separateur() {
    let vus = Rc::new(RefCell::new(Vec::new()));
    let v = vus.clone();
    let mut a = banc::adaptateurs();
    a.separateur = Box::new(FauxSeparateur::new(1.0, move |d| {
        v.borrow_mut().push(d.nombre_de_locuteurs);
        Ok(banc::tours())
    }));
    let mut banc = Banc::avec(a);
    let id = importer_un_media(&mut banc);

    banc.executer(Commande::NombreDeLocuteurs { reunion: id, nombre: Some(3) });
    assert!(fichier(&banc, id, Role::Fiche).contains("nombre_de_locuteurs: 3\n"), "{}", fichier(&banc, id, Role::Fiche));
    traiter_la_transcription(&mut banc, id);
    assert_eq!(*vus.borrow(), [Some(3)]);
}

#[test]
fn le_nombre_de_locuteurs_est_relu_au_lancement_suivant() {
    let mut banc = Banc::new();
    let id = importer_un_media(&mut banc);
    banc.executer(Commande::NombreDeLocuteurs { reunion: id, nombre: Some(4) });
    let nom = banc.reunion(id).nom_du_dossier.clone();
    banc.rouvrir();
    assert_eq!(banc.reunion_nommee(&nom).nombre_de_locuteurs, Some(4));
}

#[test]
fn laisse_detecte_le_separateur_trouve_seul_le_nombre_de_locuteurs() {
    let vus = Rc::new(RefCell::new(Vec::new()));
    let v = vus.clone();
    let mut a = banc::adaptateurs();
    a.separateur = Box::new(FauxSeparateur::new(1.0, move |d| {
        v.borrow_mut().push(d.nombre_de_locuteurs);
        Ok(banc::tours())
    }));
    let mut banc = Banc::avec(a);
    let id = importer_un_media(&mut banc);
    banc.executer(Commande::NombreDeLocuteurs { reunion: id, nombre: Some(3) });
    banc.executer(Commande::NombreDeLocuteurs { reunion: id, nombre: None });
    assert!(!fichier(&banc, id, Role::Fiche).contains("nombre_de_locuteurs"), "{}", fichier(&banc, id, Role::Fiche));
    traiter_la_transcription(&mut banc, id);
    assert_eq!(*vus.borrow(), [None]);
}

// --- En parallèle --------------------------------------------------------------------------

#[test]
fn la_transcription_et_la_separation_des_locuteurs_tournent_en_parallele() {
    let mut a = banc::adaptateurs();
    a.transcripteur = Box::new(FauxTranscripteur::fixe(3.0, banc::mots()));
    a.separateur = Box::new(FauxSeparateur::fixe(3.0, banc::tours()));
    let mut banc = Banc::avec(a);
    let id = importer_un_media(&mut banc);
    banc.executer(Commande::Traiter(id));
    // Les deux durent 3 s : en parallèle, l'étape dure 3 s, pas 6.
    banc.avancer(1.5);
    let p = banc.reunion(id).statut.progres();
    assert!((p - 0.5).abs() < 0.05, "{p}");
    banc.avancer(1.7);
    assert!(matches!(banc.reunion(id).statut, Statut::EnCours { etape: Etape::Relecture, .. }), "{:?}", banc.reunion(id).statut);
    assert_eq!(banc.reunion(id).repliques.len(), 2);
}

// --- Attribution ---------------------------------------------------------------------------

#[test]
fn les_mots_sont_decales_de_0_2_s_avant_de_chercher_le_locuteur() {
    // « Non. » (1,0 → 1,4 s) chevauche surtout SPEAKER_A sans décalage (0,25 s contre 0,15 s),
    // surtout SPEAKER_B avec (1,2 → 1,6 s : 0,05 s contre 0,35 s). Des phrases d'un mot : ni
    // interruption, ni recalage.
    let mots = vec![mot(0.0, 0.5, "Oui."), mot(1.0, 1.4, "Non."), mot(1.4, 1.8, "Si."), mot(1.8, 2.2, "Voilà.")];
    let tours = vec![tour(0.0, 1.25, "SPEAKER_A"), tour(1.25, 3.0, "SPEAKER_B")];
    let (banc, id) = transcrire(mots, tours);
    assert_eq!(locuteurs_des_mots(banc.reunion(id)), ["SPEAKER_00", "SPEAKER_01", "SPEAKER_01", "SPEAKER_01"]);
}

#[test]
fn un_mot_etire_sur_un_silence_ne_compte_que_pour_ses_premiers_instants() {
    // « Bon. » s'étire de 1,0 à 5,0 s : il ne compte que jusqu'à 1,0 + 0,3 + 0,08 × 4 = 1,62 s
    // (+0,2 s de décalage), où SPEAKER_A parle le plus.
    let mots = vec![mot(0.0, 0.5, "Alors."), mot(1.0, 5.0, "Bon."), mot(5.0, 5.4, "Ensuite."), mot(5.4, 5.8, "Fini.")];
    let tours = vec![tour(0.0, 1.6, "SPEAKER_A"), tour(1.6, 6.0, "SPEAKER_B")];
    let (banc, id) = transcrire(mots, tours);
    assert_eq!(locuteurs_des_mots(banc.reunion(id)), ["SPEAKER_00", "SPEAKER_00", "SPEAKER_01", "SPEAKER_01"]);
}

#[test]
fn sans_tour_qui_le_chevauche_un_mot_va_au_tour_le_plus_proche_a_moins_d_une_seconde_sinon_au_mot_precedent() {
    let mots = vec![
        mot(0.0, 0.4, "Oui."),  // SPEAKER_A
        mot(2.0, 2.4, "Bien."), // 2,2 → 2,6 s : SPEAKER_B commence à 3,0 s (0,4 s), SPEAKER_A finit à 1,0 s
        mot(6.0, 6.4, "Hum."),  // 6,2 → 6,6 s : rien à moins d'une seconde, comme le mot précédent
    ];
    let tours = vec![tour(0.0, 1.0, "SPEAKER_A"), tour(3.0, 4.0, "SPEAKER_B"), tour(9.0, 10.0, "SPEAKER_A")];
    let (banc, id) = transcrire(mots, tours);
    assert_eq!(locuteurs_des_mots(banc.reunion(id)), ["SPEAKER_00", "SPEAKER_01", "SPEAKER_01"]);
}

#[test]
fn une_interruption_de_deux_mots_au_plus_sans_fin_de_phrase_est_rendue_au_locuteur_en_cours() {
    // SPEAKER_B ne prend que « que oui » au milieu de la phrase de SPEAKER_A.
    let mots = phrase(0.0, "Je pense que oui sans aucun doute.");
    let tours = vec![tour(0.0, 1.0, "SPEAKER_A"), tour(1.0, 1.8, "SPEAKER_B"), tour(1.8, 5.0, "SPEAKER_A")];
    let (banc, id) = transcrire(mots, tours);
    let r = banc.reunion(id);
    assert_eq!(repliques(r), ["SPEAKER_00 : Je pense que oui sans aucun doute."]);
    assert_eq!(r.locuteurs.len(), 2, "le locuteur reste détecté");
}

#[test]
fn une_interruption_qui_finit_une_phrase_ou_de_trois_mots_n_est_pas_rendue() {
    // « D'accord. » finit une phrase : c'est une vraie réponse.
    let mut mots = phrase(0.0, "Tu viens.");
    mots.push(mot(0.8, 1.2, "D'accord."));
    mots.extend(phrase(1.2, "Alors on y va."));
    let tours = vec![tour(0.0, 1.0, "SPEAKER_A"), tour(1.0, 1.4, "SPEAKER_B"), tour(1.4, 5.0, "SPEAKER_A")];
    let (banc, id) = transcrire(mots, tours);
    assert_eq!(repliques(banc.reunion(id)), ["SPEAKER_00 : Tu viens.", "SPEAKER_01 : D'accord.", "SPEAKER_00 : Alors on y va."]);

    // Trois mots : pas une interruption (ni rendue, ni recalée : aucune fin de phrase proche).
    let mots = phrase(0.0, "Alors nous allons voir tout cela ensemble cette semaine");
    let tours = vec![tour(0.0, 1.0, "SPEAKER_A"), tour(1.0, 2.2, "SPEAKER_B"), tour(2.2, 6.0, "SPEAKER_A")];
    let (banc, id) = transcrire(mots, tours);
    let attendu = ["SPEAKER_00", "SPEAKER_00", "SPEAKER_01", "SPEAKER_01", "SPEAKER_01", "SPEAKER_00", "SPEAKER_00", "SPEAKER_00", "SPEAKER_00"];
    assert_eq!(locuteurs_des_mots(banc.reunion(id)), attendu);
}

#[test]
fn les_signes_de_ponctuation_ne_comptent_pas_dans_les_deux_mots_d_une_interruption() {
    let mots = vec![mot(0.0, 0.4, "On"), mot(0.4, 0.8, "voit"), mot(0.8, 1.2, "bien"), mot(1.2, 1.3, ","), mot(1.3, 1.7, "là"), mot(1.7, 2.1, "le"), mot(2.1, 2.5, "souci.")];
    let tours = vec![tour(0.0, 1.0, "SPEAKER_A"), tour(1.0, 1.9, "SPEAKER_B"), tour(1.9, 5.0, "SPEAKER_A")];
    let (banc, id) = transcrire(mots, tours);
    assert_eq!(repliques(banc.reunion(id)), ["SPEAKER_00 : On voit bien , là le souci."]);
}

#[test]
fn un_changement_de_locuteur_juste_apres_une_fin_de_phrase_y_est_ramene() {
    // SPEAKER_B commence trop tard d'après la séparation : « Alors on » suit une fin de phrase,
    // ils passent à SPEAKER_B.
    let mut mots = phrase(0.0, "Bon, voilà le plan.");
    mots.extend(phrase(1.6, "Alors on regarde demain."));
    let tours = vec![tour(0.0, 2.6, "SPEAKER_A"), tour(2.6, 5.0, "SPEAKER_B")];
    let (banc, id) = transcrire(mots, tours);
    assert_eq!(repliques(banc.reunion(id)), ["SPEAKER_00 : Bon, voilà le plan.", "SPEAKER_01 : Alors on regarde demain."]);
}

#[test]
fn un_changement_de_locuteur_juste_avant_une_fin_de_phrase_y_est_repousse() {
    // Pas de fin de phrase dans les trois mots d'avant, mais « réunion. » finit la phrase de
    // SPEAKER_A juste après le changement : il lui revient.
    let mut mots = phrase(0.0, "Bonjour tout le monde on commence la");
    mots.push(mot(2.8, 3.2, "réunion."));
    mots.extend(phrase(3.2, "Merci Bérénice."));
    let tours = vec![tour(0.0, 2.9, "SPEAKER_A"), tour(2.9, 5.0, "SPEAKER_B")];
    let (banc, id) = transcrire(mots, tours);
    assert_eq!(repliques(banc.reunion(id)), ["SPEAKER_00 : Bonjour tout le monde on commence la réunion.", "SPEAKER_01 : Merci Bérénice."]);
}

#[test]
fn le_recalage_ne_va_pas_au_dela_de_trois_mots() {
    // Quatre mots entre la fin de phrase et le changement : il reste où il est.
    let mut mots = phrase(0.0, "Voilà.");
    mots.extend(phrase(0.4, "Alors nous allons voir"));
    mots.extend(phrase(2.0, "ça plus tard."));
    let tours = vec![tour(0.0, 2.2, "SPEAKER_A"), tour(2.2, 5.0, "SPEAKER_B")];
    let (banc, id) = transcrire(mots, tours);
    // Recalage en avant à 3 mots près : « ça plus tard. » finit la phrase, il revient à SPEAKER_A.
    assert_eq!(repliques(banc.reunion(id)), ["SPEAKER_00 : Voilà. Alors nous allons voir ça plus tard."]);

    let mut mots = phrase(0.0, "Voilà.");
    mots.extend(phrase(0.4, "Alors nous allons voir"));
    mots.extend(phrase(2.0, "ça bien plus tard."));
    let tours = vec![tour(0.0, 2.2, "SPEAKER_A"), tour(2.2, 5.0, "SPEAKER_B")];
    let (banc, id) = transcrire(mots, tours);
    assert_eq!(repliques(banc.reunion(id)), ["SPEAKER_00 : Voilà. Alors nous allons voir", "SPEAKER_01 : ça bien plus tard."]);
}

// --- Locuteurs, répliques, fichiers --------------------------------------------------------

#[test]
fn les_locuteurs_sont_numerotes_dans_l_ordre_de_leur_premier_mot_et_les_tours_suivent() {
    // pyannote numérote à sa façon : celui qui parle en premier devient SPEAKER_00.
    let mots = vec![mot(0.0, 0.4, "Salut."), mot(2.0, 2.4, "Bonjour."), mot(4.0, 4.4, "Coucou.")];
    let tours = vec![tour(0.0, 1.0, "SPEAKER_02"), tour(2.0, 3.0, "SPEAKER_00"), tour(4.0, 5.0, "SPEAKER_01"), tour(8.0, 9.0, "SPEAKER_03")];
    let (banc, id) = transcrire(mots, tours);
    let r = banc.reunion(id);
    let etiquettes: Vec<&str> = r.locuteurs.iter().map(|l| l.etiquette.as_str()).collect();
    // Un locuteur sans mot reste détecté, après les autres.
    assert_eq!(etiquettes, ["SPEAKER_00", "SPEAKER_01", "SPEAKER_02", "SPEAKER_03"]);
    let tours: Vec<(f64, &str)> = r.tours.iter().map(|t| (t.debut, t.locuteur.as_str())).collect();
    assert_eq!(tours, [(0.0, "SPEAKER_00"), (2.0, "SPEAKER_01"), (4.0, "SPEAKER_02"), (8.0, "SPEAKER_03")]);
    assert_eq!(repliques(r), ["SPEAKER_00 : Salut.", "SPEAKER_01 : Bonjour.", "SPEAKER_02 : Coucou."]);
}

#[test]
fn une_nouvelle_replique_commence_a_chaque_changement_de_locuteur() {
    let mut mots = phrase(0.0, "On démarre ?");
    mots.extend(phrase(1.2, "Oui, allons-y."));
    mots.extend(phrase(2.4, "Parfait."));
    let tours = vec![tour(0.0, 1.3, "SPEAKER_A"), tour(1.3, 2.5, "SPEAKER_B"), tour(2.5, 3.2, "SPEAKER_A")];
    let (banc, id) = transcrire(mots, tours);
    assert_eq!(repliques(banc.reunion(id)), ["SPEAKER_00 : On démarre ?", "SPEAKER_01 : Oui, allons-y.", "SPEAKER_00 : Parfait."]);
}

#[test]
fn les_transcriptions_lisible_et_structuree_contiennent_locuteurs_tours_et_locuteur_de_chaque_mot() {
    let mut mots = phrase(0.0, "On démarre ?");
    mots.extend(phrase(1.2, "Oui, allons-y."));
    let tours = vec![tour(0.0, 1.3, "SPEAKER_A"), tour(1.3, 2.4, "SPEAKER_B")];
    let (mut banc, id) = transcrire(mots, tours);

    let json: serde_json::Value = serde_json::from_str(&fichier(&banc, id, Role::TranscriptionStructuree)).expect("JSON");
    assert_eq!(json["locuteurs"], serde_json::json!(["SPEAKER_00", "SPEAKER_01"]));
    let des_mots: Vec<&str> = json["mots"].as_array().expect("mots").iter().map(|m| m["locuteur"].as_str().unwrap_or("?")).collect();
    assert_eq!(des_mots, ["SPEAKER_00", "SPEAKER_00", "SPEAKER_00", "SPEAKER_01", "SPEAKER_01"]);
    assert_eq!(json["tours"], serde_json::json!([{ "debut": 0.0, "fin": 1.3, "locuteur": "SPEAKER_00" }, { "debut": 1.3, "fin": 2.4, "locuteur": "SPEAKER_01" }]));

    let md = fichier(&banc, id, Role::TranscriptionLisible);
    assert!(md.contains("[0:00:00] SPEAKER_00 : On démarre ?"), "{md}");
    assert!(md.contains("[0:00:01] SPEAKER_01 : Oui, allons-y."), "{md}");

    // Relue telle quelle au lancement suivant.
    let nom = banc.reunion(id).nom_du_dossier.clone();
    banc.rouvrir();
    let r = banc.reunion_nommee(&nom);
    assert_eq!(locuteurs_des_mots(r), ["SPEAKER_00", "SPEAKER_00", "SPEAKER_00", "SPEAKER_01", "SPEAKER_01"]);
    assert_eq!(r.tours.len(), 2);
}
