//! Le moteur du banc de parité (`scripts/banc-parite`) : fait passer un média par les vrais
//! moteurs de Greffier, comme l'étape Transcription du cœur (whisper.cpp avec le vocabulaire du
//! lexique, pyannote, puis `attribution::attribuer(sans_les_bruits(mots), tours)`), et écrit le
//! résultat en JSON. Les deux moteurs tournent l'un après l'autre pour que leurs durées se
//! comparent à celles du vault.
//!
//!     cargo run --release -p greffier-moteurs --example banc -- <média> <sortie.json> \
//!         [--lexique lexique.md] [--locuteurs N] [--wav audio.wav]
//!
//! - `--lexique` : le lexique dont la section Vocabulaire est donnée à whisper (par défaut celui du
//!   dossier de l'app, comme Greffier).
//! - `--wav` : écrit aussi la piste de mixage décodée (PCM mono 16 kHz), pour la comparer à celle
//!   de ffmpeg.
//!
//! Autre usage, `banc --decouper <média> <début> <fin> <sortie.wav>` : écrit la piste de mixage
//! décodée par AVFoundation entre deux instants (en secondes), comme
//! `ffmpeg -ss <début> -to <fin> -i <média> -vn -ac 1 -ar 16000 -c:a pcm_s16le` (le WAV du banc
//! du vault).
//!
//! Sortie : `{ audio, vocabulaire, secondes_transcription, secondes_separation, brut: [mots de
//! whisper], words: [{start, end, text, locuteur}], tours: [{start, end, locuteur}] }`. Seuls le
//! fichier de sortie, le WAV demandé et le WAV temporaire de pyannote (dans le dossier de l'app)
//! sont écrits.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use greffier_coeur::adaptateurs::{DemandeDeSeparation, DemandeDeTranscription};
use greffier_coeur::{attribution, lexique, ressources};
use greffier_moteurs::{SeparateurPyannote, TranscripteurWhisper, audio};
use serde_json::{Value, json};

fn decouper(args: &[String]) {
    let [media, debut, fin, sortie] = args else { panic!("usage : banc --decouper <média> <début> <fin> <sortie.wav>") };
    let instant = |s: &String| (s.parse::<f64>().expect("instant en secondes") * audio::FREQUENCE as f64).round() as usize;
    let echantillons = audio::mixage(std::path::Path::new(media), &AtomicBool::new(false), |_| {}).unwrap_or_else(|e| panic!("audio : {e}"));
    let (de, a) = (instant(debut).min(echantillons.len()), instant(fin).min(echantillons.len()));
    audio::ecrire_wav(std::path::Path::new(sortie), &echantillons[de..a]).unwrap_or_else(|e| panic!("{sortie} : {e}"));
}

fn main() {
    let tous: Vec<String> = std::env::args().skip(1).collect();
    if tous.first().map(String::as_str) == Some("--decouper") {
        return decouper(&tous[1..]);
    }
    let mut args = tous.into_iter();
    let mut positionnels = Vec::new();
    let (mut lexique_md, mut nombre, mut wav) = (None, None, None);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--lexique" => lexique_md = Some(PathBuf::from(args.next().expect("--lexique <fichier>"))),
            "--locuteurs" => nombre = Some(args.next().and_then(|n| n.parse().ok()).expect("--locuteurs <nombre>")),
            "--wav" => wav = Some(PathBuf::from(args.next().expect("--wav <fichier>"))),
            _ => positionnels.push(PathBuf::from(a)),
        }
    }
    let [media, sortie] = <[PathBuf; 2]>::try_from(positionnels).expect("usage : banc <média> <sortie.json> [--lexique f] [--locuteurs N] [--wav f]");
    let app = ressources::dossier_de_l_app();
    let lexique_md = lexique_md.unwrap_or_else(|| app.join("lexique.md"));
    let vocabulaire = std::fs::read_to_string(&lexique_md).map(|md| lexique::vocabulaire(&md)).unwrap_or_default();
    let arret = AtomicBool::new(false);

    if let Some(wav) = &wav {
        let echantillons = audio::mixage(&media, &arret, |_| {}).unwrap_or_else(|e| panic!("audio : {e}"));
        audio::ecrire_wav(wav, &echantillons).unwrap_or_else(|e| panic!("{} : {e}", wav.display()));
    }

    let t = Instant::now();
    let demande = DemandeDeTranscription { audio: media.clone(), duree: 0.0, vocabulaire: vocabulaire.clone() };
    let mots = TranscripteurWhisper::de_l_app(&app).transcrire_ici(&demande, &arret, &mut |_| {}).unwrap_or_else(|e| panic!("transcription : {e}"));
    let secondes_transcription = t.elapsed().as_secs_f64();

    let t = Instant::now();
    let demande = DemandeDeSeparation { audio: media.clone(), duree: 0.0, nombre_de_locuteurs: nombre };
    let tours = SeparateurPyannote::de_l_app(&app).separer_ici(&demande, &arret, &mut |_| {}).unwrap_or_else(|e| panic!("séparation : {e}"));
    let secondes_separation = t.elapsed().as_secs_f64();

    let brut: Vec<Value> = mots.iter().map(|m| json!({ "start": m.debut, "end": m.fin, "text": m.texte })).collect();
    let a = attribution::attribuer(attribution::sans_les_bruits(mots), tours);
    let etiquette = |i: Option<usize>| i.and_then(|i| a.locuteurs.get(i)).map(|l| l.etiquette.clone());
    let words: Vec<Value> = a.mots.iter().map(|m| json!({ "start": m.debut, "end": m.fin, "text": m.texte, "locuteur": etiquette(m.locuteur) })).collect();
    let tours: Vec<Value> = a.tours.iter().map(|t| json!({ "start": t.debut, "end": t.fin, "locuteur": t.locuteur })).collect();
    let resultat = json!({
        "audio": media,
        "vocabulaire": lexique_md,
        "secondes_transcription": secondes_transcription,
        "secondes_separation": secondes_separation,
        "brut": brut,
        "words": words,
        "tours": tours,
    });
    std::fs::write(&sortie, serde_json::to_string(&resultat).expect("json")).unwrap_or_else(|e| panic!("{} : {e}", sortie.display()));
    eprintln!("{} : {} mots, {} locuteurs, transcription {:.1} s, séparation {:.1} s", media.display(), a.mots.len(), a.locuteurs.len(), secondes_transcription, secondes_separation);
}
