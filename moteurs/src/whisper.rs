//! whisper.cpp, compilé dans l'app (build.rs), appelé par `src/transcrire.cpp` avec les
//! paramètres du vault.

use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use greffier_coeur::Mot;

/// Paramètres fixés (ceux du vault) : français, 6 threads.
pub const LANGUE: &str = "fr";
pub const THREADS: i32 = 6;

#[repr(C)]
struct Reglages {
    modele: *const c_char,
    vad: *const c_char,
    langue: *const c_char,
    threads: c_int,
    vocabulaire: *const c_char,
}

type RappelMot = extern "C" fn(*mut c_void, *const c_char, i64, i64, f32);
type RappelProgres = extern "C" fn(*mut c_void, c_int);
type RappelAbandon = extern "C" fn(*mut c_void) -> bool;

unsafe extern "C" {
    fn greffier_transcrire(
        r: *const Reglages,
        pcm: *const f32,
        n: c_int,
        user: *mut c_void,
        mot: RappelMot,
        progres: RappelProgres,
        abandon: RappelAbandon,
        erreur: *mut c_char,
        taille_erreur: c_int,
    ) -> c_int;
}

struct Contexte<'a> {
    mots: Vec<Mot>,
    progres: &'a mut dyn FnMut(f64),
    arret: &'a AtomicBool,
}

extern "C" fn sur_mot(user: *mut c_void, texte: *const c_char, t0: i64, t1: i64, p: f32) {
    // SAFETY : `user` est le `Contexte` passé à `greffier_transcrire`, vivant pendant l'appel ;
    // `texte` est une chaîne C de whisper.cpp.
    let c = unsafe { &mut *(user as *mut Contexte) };
    let texte = unsafe { CStr::from_ptr(texte) }.to_string_lossy();
    c.mots.push(Mot { debut: t0 as f64 / 1000.0, fin: t1 as f64 / 1000.0, texte: texte.trim().to_string(), confiance: (p * 1000.0).round() / 1000.0, locuteur: None });
}

extern "C" fn sur_progres(user: *mut c_void, progres: c_int) {
    // SAFETY : voir `sur_mot`.
    let c = unsafe { &mut *(user as *mut Contexte) };
    (c.progres)(progres as f64 / 100.0);
}

extern "C" fn abandon(user: *mut c_void) -> bool {
    // SAFETY : voir `sur_mot`.
    let c = unsafe { &*(user as *const Contexte) };
    c.arret.load(Ordering::Relaxed)
}

fn chaine(s: &str) -> Result<CString, String> {
    CString::new(s).map_err(|_| format!("caractère nul dans « {s} »"))
}

/// Transcrit de l'audio mono 16 kHz : un mot par segment, avec sa confiance (arrondie au
/// millième, comme le vault), horodaté en secondes. `progres` reçoit l'avancement de whisper (0
/// à 1). `arret` l'interrompt (rend `Err("arrêté")`).
pub fn transcrire(modele: &Path, vad: &Path, echantillons: &[i16], vocabulaire: &str, arret: &AtomicBool, progres: &mut dyn FnMut(f64)) -> Result<Vec<Mot>, String> {
    // Comme whisper-cli lisant un wav 16 bits.
    let pcm: Vec<f32> = echantillons.iter().map(|&e| e as f32 / 32768.0).collect();
    let modele_c = chaine(&modele.to_string_lossy())?;
    let vad_c = chaine(&vad.to_string_lossy())?;
    let langue = chaine(LANGUE)?;
    let vocabulaire = chaine(vocabulaire)?;
    let reglages = Reglages { modele: modele_c.as_ptr(), vad: vad_c.as_ptr(), langue: langue.as_ptr(), threads: THREADS, vocabulaire: vocabulaire.as_ptr() };
    let mut contexte = Contexte { mots: Vec::new(), progres, arret };
    let mut erreur = vec![0 as c_char; 512];
    let n = c_int::try_from(pcm.len()).map_err(|_| "réunion trop longue pour whisper.cpp".to_string())?;
    // SAFETY : tous les pointeurs restent valides pendant l'appel, qui est synchrone.
    let code = unsafe {
        greffier_transcrire(&reglages, pcm.as_ptr(), n, &mut contexte as *mut Contexte as *mut c_void, sur_mot, sur_progres, abandon, erreur.as_mut_ptr(), erreur.len() as c_int)
    };
    let message = || unsafe { CStr::from_ptr(erreur.as_ptr()) }.to_string_lossy().trim().to_string();
    match code {
        0 => Ok(contexte.mots),
        1 => Err(format!("Le modèle de Whisper est illisible ({}) : {}", modele.display(), message())),
        3 => Err("arrêté".into()),
        _ => Err(format!("whisper.cpp a échoué : {}", message())),
    }
}
