//! L'import d'une réunion passée : un média seul, avec sa transcription (fichier de Greffier,
//! WebVTT ou SRT), ou avec sa transcription et son bilan. Les fichiers sont copiés dans un
//! nouveau dossier de réunion ; les originaux ne bougent pas.

mod banc;

use std::path::{Path, PathBuf};

use banc::Banc;
use greffier_coeur::dossier::Import;
use greffier_coeur::{Commande, DateHeure, Etape, Evenement, IdReunion, Role, Statut};

/// La vidéo des fixtures (12 s).
fn video() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/dossier-de-reunions/2026-11-19 15h30 - Réunion/2026-11-19 15h30 - Réunion - enregistrement.mp4")
}

/// Des fichiers hors du dossier de réunions, comme sur le bureau.
struct Dehors(PathBuf);

impl Dehors {
    fn new(banc: &Banc) -> Dehors {
        let d = banc.dossier.with_extension("dehors");
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("dossier des originaux");
        Dehors(d)
    }
    fn ecrire(&self, nom: &str, contenu: &str) -> PathBuf {
        let p = self.0.join(nom);
        std::fs::write(&p, contenu).expect("original");
        p
    }
    fn copier(&self, de: &Path, nom: &str) -> PathBuf {
        let p = self.0.join(nom);
        std::fs::copy(de, &p).expect("original");
        p
    }
}

impl Drop for Dehors {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn importer(banc: &mut Banc, import: Import) -> IdReunion {
    let ev = banc.executer(Commande::Importer(import));
    ev.iter()
        .find_map(|e| match e {
            Evenement::ReunionImportee(id) => Some(*id),
            _ => None,
        })
        .expect("la réunion importée")
}

fn import(enregistrement: PathBuf) -> Import {
    Import { enregistrement, transcription: None, bilan: None, debut: DateHeure::new(2026, 10, 2, 9, 30), titre: "Point client".into() }
}

#[test]
fn un_media_seul_devient_une_reunion_a_traiter_des_la_transcription() {
    let mut banc = Banc::new();
    let dehors = Dehors::new(&banc);
    let original = dehors.copier(&video(), "point client.mp4");
    let id = importer(&mut banc, import(original.clone()));

    let r = banc.reunion(id);
    assert_eq!(r.nom_du_dossier, "2026-10-02 09h30 - Point client");
    assert_eq!(r.titre, "Point client");
    assert_eq!(r.debut, DateHeure::new(2026, 10, 2, 9, 30));
    assert_eq!(r.duree, 12.0);
    assert_eq!(r.statut, Statut::ATraiter);
    assert_eq!(r.etape_faite, None);
    assert_eq!(r.fichiers, [Role::Fiche, Role::Enregistrement]);
    let copie = banc.greffier.dossier().fichier(&r.nom_du_dossier, Role::Enregistrement);
    assert_eq!(std::fs::read(copie).unwrap(), std::fs::read(video()).unwrap());
    assert!(original.is_file(), "l'original reste où il était");

    // Relue au lancement suivant, à l'identique.
    let nom = r.nom_du_dossier.clone();
    banc.rouvrir();
    let r = banc.reunion_nommee(&nom);
    assert_eq!((r.statut.clone(), r.etape_faite, r.duree, r.titre.as_str()), (Statut::ATraiter, None, 12.0, "Point client"));

    let id = r.id;
    banc.executer(Commande::Traiter(id));
    assert_eq!(banc.avancer_jusqu_a(id, |s| *s == Statut::Terminee), [Etape::Transcription, Etape::Relecture, Etape::Bilan]);
}

/// (locuteur affiché, début, fin, texte) de chaque réplique.
fn repliques(banc: &Banc, id: IdReunion) -> Vec<(String, f64, f64, String)> {
    let r = banc.reunion(id);
    r.repliques.iter().map(|x| (r.locuteurs[r.locuteur_affiche(x.locuteur)].affiche(), x.debut, x.fin, x.texte.clone())).collect()
}

fn replique(qui: &str, debut: f64, fin: f64, texte: &str) -> (String, f64, f64, String) {
    (qui.into(), debut, fin, texte.into())
}

#[test]
fn un_media_et_sa_transcription_webvtt_reprend_a_la_relecture() {
    let mut banc = Banc::new();
    let dehors = Dehors::new(&banc);
    let vtt = dehors.ecrire(
        "point.vtt",
        "WEBVTT\n\nNOTE exporté de la visio\n\n1\n00:00:01.000 --> 00:00:03.500\n<v Alexandre>Bonjour Khalid,\non commence ?</v>\n\n2\n00:00:04.000 --> 00:00:05.250 align:start\n<v.chef Khalid>Oui, <i>vas-y</i>.\n\n00:06.000 --> 00:07.000\n<v Alexandre>Parfait.\n",
    );
    let id = importer(&mut banc, Import { transcription: Some(vtt.clone()), ..import(dehors.copier(&video(), "point.mp4")) });

    assert_eq!(
        repliques(&banc, id),
        [replique("Alexandre", 1.0, 3.5, "Bonjour Khalid, on commence ?"), replique("Khalid", 4.0, 5.25, "Oui, vas-y."), replique("Alexandre", 6.0, 7.0, "Parfait.")]
    );
    let r = banc.reunion(id);
    let etiquettes: Vec<&str> = r.locuteurs.iter().map(|l| l.etiquette.as_str()).collect();
    assert_eq!(etiquettes, ["SPEAKER_00", "SPEAKER_01"], "les noms sont donnés aux locuteurs, dans l'ordre où ils parlent");
    assert_eq!(r.statut, Statut::ATraiter);
    assert_eq!(r.etape_faite, Some(Etape::Transcription));
    assert!(r.relecture.is_none(), "une transcription importée n'est pas relue");
    assert_eq!(r.fichiers, [Role::Fiche, Role::TranscriptionLisible, Role::TranscriptionStructuree, Role::Enregistrement]);
    assert!(vtt.is_file());

    // La fiche garde les noms, la transcription structurée les répliques : tout revient au lancement suivant.
    let avant = r.clone();
    banc.rouvrir();
    let apres = banc.reunion_nommee(&avant.nom_du_dossier).clone();
    assert_eq!(greffier_coeur::Reunion { id: avant.id, ..apres }, avant);

    let id = banc.reunion_nommee(&avant.nom_du_dossier).id;
    banc.executer(Commande::Traiter(id));
    assert_eq!(banc.avancer_jusqu_a(id, |s| *s == Statut::Terminee), [Etape::Relecture, Etape::Bilan]);
}

/// Importe le média des fixtures avec cette transcription ; rend ses répliques.
fn importer_la_transcription(nom: &str, contenu: &str) -> Vec<(String, f64, f64, String)> {
    let mut banc = Banc::new();
    let dehors = Dehors::new(&banc);
    let t = dehors.ecrire(nom, contenu);
    let id = importer(&mut banc, Import { transcription: Some(t), ..import(dehors.copier(&video(), "point.mp4")) });
    repliques(&banc, id)
}

#[test]
fn le_locuteur_d_un_srt_est_lu_entre_crochets() {
    let srt = "1\r\n00:00:00,051 --> 00:00:02,853\r\n[SPEAKER_02]: Oui, vas-y.\r\n\r\n2\r\n00:00:02,873 --> 00:00:08,057\r\n[SPEAKER_00]: On regarde le banc.\r\n\r\n3\r\n00:00:09,000 --> 00:00:10,000\r\n[Khalid] : D'accord.\r\n";
    assert_eq!(
        importer_la_transcription("point.srt", srt),
        [replique("SPEAKER_02", 0.051, 2.853, "Oui, vas-y."), replique("SPEAKER_00", 2.873, 8.057, "On regarde le banc."), replique("Khalid", 9.0, 10.0, "D'accord.")]
    );
}

#[test]
fn le_locuteur_est_lu_devant_deux_points() {
    let vtt = "WEBVTT\n\n00:00:01.000 --> 00:00:02.000\nAlexandre : Bonjour.\n\n00:00:02.000 --> 00:00:03.000\nMarie-Claire Dupont: Salut, il est 10:30.\n\n00:00:03.000 --> 00:00:04.000\net on continue sans nom\n";
    assert_eq!(
        importer_la_transcription("point.vtt", vtt),
        [replique("Alexandre", 1.0, 2.0, "Bonjour."), replique("Marie-Claire Dupont", 2.0, 3.0, "Salut, il est 10:30."), replique("Marie-Claire Dupont", 3.0, 4.0, "et on continue sans nom")]
    );
}

#[test]
fn sans_locuteur_tout_va_a_un_seul_locuteur() {
    let srt = "1\n00:00:01,000 --> 00:00:02,000\nBonjour à tous.\n\n2\n00:00:02,500 --> 00:00:04,000\nBref : on commence.\n\n3\n00:00:04,000 --> 00:00:05,000\nC'est parti.\n";
    assert_eq!(
        importer_la_transcription("point.srt", srt),
        [replique("SPEAKER_00", 1.0, 2.0, "Bonjour à tous."), replique("SPEAKER_00", 2.5, 4.0, "Bref : on commence."), replique("SPEAKER_00", 4.0, 5.0, "C'est parti.")]
    );
}

fn revue_de_release(role: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fixtures/dossier-de-reunions/2026-11-20 10h00 - Revue de release/2026-11-20 10h00 - Revue de release - {role}"))
}

#[test]
fn un_media_et_un_bilan_sans_transcription_repartent_de_la_transcription() {
    let mut banc = Banc::new();
    let dehors = Dehors::new(&banc);
    let bilan = dehors.ecrire("point.md", "Un bilan écrit ailleurs.\n");
    let id = importer(&mut banc, Import { bilan: Some(bilan), ..import(dehors.copier(&video(), "point.mp4")) });
    assert_eq!((banc.reunion(id).statut.clone(), banc.reunion(id).etape_faite), (Statut::ATraiter, None));

    banc.executer(Commande::Traiter(id));
    assert_eq!(banc.avancer_jusqu_a(id, |s| *s == Statut::Terminee), [Etape::Transcription, Etape::Relecture, Etape::Bilan]);
}

#[test]
fn un_media_sa_transcription_et_son_bilan_ne_passent_par_aucune_etape() {
    let mut banc = Banc::new();
    let dehors = Dehors::new(&banc);
    let bilan = "Un bilan écrit ailleurs, sans les sections de Greffier.\n";
    let id = importer(
        &mut banc,
        Import {
            transcription: Some(dehors.copier(&revue_de_release("transcription.json"), "revue.json")),
            bilan: Some(dehors.ecrire("revue.md", bilan)),
            titre: "Revue : release".into(),
            ..import(dehors.copier(&video(), "revue.mp4"))
        },
    );

    let r = banc.reunion(id);
    assert_eq!(r.nom_du_dossier, "2026-10-02 09h30 - Revue - release");
    assert_eq!(r.titre, "Revue : release");
    assert_eq!(r.statut, Statut::Terminee);
    assert_eq!(r.etape_faite, Some(Etape::Bilan));
    assert_eq!(r.fichiers, Role::TOUS);
    assert_eq!(r.bilan, bilan);
    assert!(!r.nouvelle);
    assert_eq!(banc.lire(&banc.greffier.dossier().fichier(&r.nom_du_dossier, Role::Bilan)), bilan, "le bilan est copié tel quel");
    // Le fichier de Greffier est repris, mais réputé non relu.
    assert_eq!(r.repliques.len(), 3);
    assert_eq!(r.repliques[1].texte, "Oui, si la recette passe vendredi.");
    assert!(r.relecture.is_none() && r.brute.is_empty());
    let json = banc.lire(&banc.greffier.dossier().fichier(&r.nom_du_dossier, Role::TranscriptionStructuree));
    assert!(!json.contains("\"relecture\""));

    assert!(banc.greffier.executer(Commande::Traiter(id)).is_err(), "rien ne reste à faire");
    let avant = banc.reunion(id).clone();
    banc.rouvrir();
    let apres = banc.reunion_nommee(&avant.nom_du_dossier).clone();
    assert_eq!(greffier_coeur::Reunion { id: avant.id, ..apres }, avant);
}

/// Une vraie transcription d'une ancienne réunion du vault (lue sur place : le dépôt est public,
/// elle n'y est pas copiée). Sans le vault sur ce Mac, le test ne vérifie rien.
#[test]
fn un_vtt_reel_du_vault_s_importe_comme_n_importe_quel_vtt() {
    let Some(maison) = std::env::var_os("HOME") else { return };
    let vtt = Path::new(&maison).join("Vault/Falcon HQ/Réunions/2026-11-05 11h53 - Réunion/transcription/2026-11-05 11h53.vtt");
    if !vtt.is_file() {
        eprintln!("vault absent, test sauté : {}", vtt.display());
        return;
    }
    let mut banc = Banc::new();
    let dehors = Dehors::new(&banc);
    let id = importer(&mut banc, Import { transcription: Some(vtt.clone()), ..import(dehors.copier(&video(), "2026-11-05 11h53.mp4")) });

    let r = banc.reunion(id);
    assert_eq!(r.repliques.len(), 111, "une réplique par entrée du VTT");
    let etiquettes: Vec<&str> = r.locuteurs.iter().map(|l| l.etiquette.as_str()).collect();
    assert_eq!(etiquettes, ["SPEAKER_00", "SPEAKER_01", "SPEAKER_02"]);
    assert!(r.locuteurs.iter().all(|l| l.nom.is_none()), "des étiquettes, pas des noms");
    let parle = |i: usize| r.repliques.iter().filter(|x| x.locuteur == i).count();
    assert_eq!((parle(0), parle(1), parle(2)), (30, 26, 55));
    let premiere = &r.repliques[0];
    assert_eq!((premiere.locuteur, premiere.debut, premiere.fin), (2, 0.051, 2.853));
    assert!(premiere.texte.starts_with("Oui, vas-y"), "le « [SPEAKER_02]: » est retiré du texte : {}", premiere.texte);
    let derniere = r.repliques.last().unwrap();
    assert_eq!((derniere.locuteur, derniere.debut, derniere.fin, derniere.texte.as_str()), (1, 423.594, 423.694, "Hop."));
    assert_eq!(r.etape_faite, Some(Etape::Transcription));
}

/// Un MP4 minimal dont la boîte `mvhd` dit sa date de création (secondes depuis 1904, UTC).
fn mp4_cree_le(secondes_depuis_1904: u32) -> Vec<u8> {
    let boite = |nom: &[u8; 4], contenu: &[u8]| [&(8 + contenu.len() as u32).to_be_bytes()[..], nom, contenu].concat();
    let mut mvhd = vec![0u8; 100];
    mvhd[4..8].copy_from_slice(&secondes_depuis_1904.to_be_bytes());
    mvhd[12..16].copy_from_slice(&1000u32.to_be_bytes());
    mvhd[16..20].copy_from_slice(&90_000u32.to_be_bytes());
    [boite(b"ftyp", b"isom\0\0\x02\0isom"), boite(b"moov", &boite(b"mvhd", &mvhd))].concat()
}

#[test]
fn les_fichiers_deposes_donnent_le_media_la_transcription_le_bilan_la_date_et_le_titre() {
    let banc = Banc::new();
    let dehors = Dehors::new(&banc);
    let media = dehors.0.join("visio.mp4");
    // 2026-10-02 07:30:00 UTC = 3 873 771 000 s depuis 1904.
    std::fs::write(&media, mp4_cree_le(3_873_771_000)).unwrap();
    let vtt = dehors.ecrire("visio.vtt", "WEBVTT\n\n00:01.000 --> 00:02.000\nBonjour.\n");
    let bilan = dehors.ecrire("notes.md", "---\ntype: bilan\n---\n\n# Bilan — Point client Axe\n\n## En bref\n\nRien.\n");

    let import = banc.greffier.proposer_import(&[vtt.clone(), bilan.clone(), media.clone()]).unwrap();
    assert_eq!(import, Import { enregistrement: media, transcription: Some(vtt), bilan: Some(bilan), debut: DateHeure::new(2026, 10, 2, 7, 30), titre: "Point client Axe".into() });
    assert_eq!(banc.greffier.dossier().noms(), Vec::<String>::new(), "proposer n'écrit rien");
}

#[test]
fn sans_date_dans_le_media_la_date_est_celle_de_modification_et_le_titre_le_nom_du_fichier() {
    let mut a = banc::adaptateurs();
    let audio_demande = std::rc::Rc::new(std::cell::RefCell::new(None));
    let note = audio_demande.clone();
    a.transcripteur = Box::new(greffier_coeur::faux::FauxTranscripteur::new(banc::DUREE_D_ETAPE, move |d| {
        *note.borrow_mut() = Some(d.audio.clone());
        Ok(banc::mots())
    }));
    let mut banc = Banc::avec(a);
    let dehors = Dehors::new(&banc);
    let audio = dehors.ecrire("Daily du lundi.m4a", "pas vraiment de l'audio");
    // 2026-11-16 08:45:00 UTC.
    let lundi = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_794_818_700);
    std::fs::File::options().write(true).open(&audio).unwrap().set_modified(lundi).unwrap();

    let propose = banc.greffier.proposer_import(std::slice::from_ref(&audio)).unwrap();
    assert_eq!((propose.debut, propose.titre.as_str(), propose.transcription.clone(), propose.bilan.clone()), (DateHeure::new(2026, 11, 16, 8, 45), "Daily du lundi", None, None));

    // La date et le titre se changent avant de valider ; l'audio garde son extension.
    let id = importer(&mut banc, Import { debut: DateHeure::new(2026, 11, 16, 9, 0), titre: "Daily".into(), ..propose });
    let nom = banc.reunion(id).nom_du_dossier.clone();
    assert_eq!(nom, "2026-11-16 09h00 - Daily");
    let copie = banc.greffier.dossier().dossier(&nom).join("2026-11-16 09h00 - Daily - enregistrement.m4a");
    assert!(copie.is_file());
    assert_eq!(banc.reunion(id).fichiers, [Role::Fiche, Role::Enregistrement]);
    banc.executer(Commande::Traiter(id));
    banc.avancer_jusqu_a(id, |s| *s == Statut::Terminee);
    assert_eq!(audio_demande.borrow().as_ref(), Some(&copie), "le traitement lit l'audio importé");
}

#[test]
fn un_import_sans_media_ou_avec_deux_medias_est_refuse() {
    let banc = Banc::new();
    let dehors = Dehors::new(&banc);
    let vtt = dehors.ecrire("a.vtt", "WEBVTT\n");
    let pdf = dehors.ecrire("a.pdf", "");
    let mp4 = dehors.copier(&video(), "a.mp4");
    let mov = dehors.copier(&video(), "b.mov");
    for fichiers in [vec![vtt.clone()], vec![mp4.clone(), mov], vec![mp4, pdf], vec![]] {
        assert!(banc.greffier.proposer_import(&fichiers).is_err(), "{fichiers:?} accepté");
    }
}

#[test]
fn un_import_rate_ne_laisse_rien_dans_le_dossier_de_reunions() {
    let mut banc = Banc::new();
    let dehors = Dehors::new(&banc);
    let vide = dehors.ecrire("vide.vtt", "WEBVTT\n\nNOTE rien ici\n");
    let refus = banc.greffier.executer(Commande::Importer(Import { transcription: Some(vide), ..import(dehors.copier(&video(), "a.mp4")) }));
    assert!(refus.is_err());
    assert!(banc.greffier.reunions().is_empty());
    assert_eq!(std::fs::read_dir(&banc.dossier).unwrap().count(), 0);
}

#[test]
fn une_reunion_importee_n_est_pas_vue_comme_modifiee_hors_de_l_app_et_son_audio_est_surveille() {
    let mut banc = Banc::new();
    let dehors = Dehors::new(&banc);
    let audio = dehors.ecrire("Point.m4a", "pas vraiment de l'audio");
    let id = importer(&mut banc, import(audio));

    // Les fichiers copiés et écrits par Greffier ne passent pas pour des modifications.
    assert_eq!(banc.greffier.relire_le_dossier(), []);

    // L'enregistrement importé garde son extension, et sa modification se voit quand même.
    let nom = banc.reunion(id).nom_du_dossier.clone();
    let copie = banc.greffier.dossier().enregistrement(&nom).expect("l'audio importé");
    assert_eq!(copie.extension().and_then(|e| e.to_str()), Some("m4a"));
    std::fs::write(&copie, "un autre audio, plus long que le premier").unwrap();
    assert_eq!(banc.greffier.relire_le_dossier(), [Evenement::ReunionRelue(id)]);
}
