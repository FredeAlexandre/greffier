//! whisper.cpp, compilé dans l'app (build.rs), appelé par `src/transcribe.cpp` avec les
//! paramètres du vault.

use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use greffier_core::Word;

/// Paramètres fixés (ceux du vault) : français, 6 threads.
pub const LANGUAGE: &str = "fr";
pub const THREADS: i32 = 6;

#[repr(C)]
struct Settings {
    model: *const c_char,
    vad: *const c_char,
    language: *const c_char,
    threads: c_int,
    vocabulary: *const c_char,
}

type WordCallback = extern "C" fn(*mut c_void, *const c_char, i64, i64, f32);
type ProgressCallback = extern "C" fn(*mut c_void, c_int);
type AbortCallback = extern "C" fn(*mut c_void) -> bool;

unsafe extern "C" {
    fn greffier_transcribe(
        r: *const Settings,
        pcm: *const f32,
        n: c_int,
        user: *mut c_void,
        word: WordCallback,
        progress: ProgressCallback,
        abandon: AbortCallback,
        error: *mut c_char,
        error_size: c_int,
    ) -> c_int;
}

struct Context<'a> {
    words: Vec<Word>,
    progress: &'a mut dyn FnMut(f64),
    stop: &'a AtomicBool,
}

extern "C" fn on_word(user: *mut c_void, text: *const c_char, t0: i64, t1: i64, p: f32) {
    // SAFETY : `user` est le `Contexte` passé à `greffier_transcribe`, vivant pendant l'appel ;
    // `texte` est une chaîne C de whisper.cpp.
    let c = unsafe { &mut *(user as *mut Context) };
    let text = unsafe { CStr::from_ptr(text) }.to_string_lossy();
    c.words.push(Word { start: t0 as f64 / 1000.0, end: t1 as f64 / 1000.0, text: text.trim().to_string(), confidence: (p * 1000.0).round() / 1000.0, speaker: None });
}

extern "C" fn on_progress(user: *mut c_void, progress: c_int) {
    // SAFETY : voir `sur_mot`.
    let c = unsafe { &mut *(user as *mut Context) };
    (c.progress)(progress as f64 / 100.0);
}

extern "C" fn abandon(user: *mut c_void) -> bool {
    // SAFETY : voir `sur_mot`.
    let c = unsafe { &*(user as *const Context) };
    c.stop.load(Ordering::Relaxed)
}

fn c_string(s: &str) -> Result<CString, String> {
    CString::new(s).map_err(|_| format!("caractère nul dans « {s} »"))
}

/// Transcrit de l'audio mono 16 kHz : un mot par segment, avec sa confiance (arrondie au
/// millième, comme le vault), horodaté en secondes. `progres` reçoit l'avancement de whisper (0
/// à 1). `arret` l'interrompt (rend `Err("arrêté")`).
pub fn transcribe(model: &Path, vad: &Path, samples: &[i16], vocabulary: &str, stop: &AtomicBool, progress: &mut dyn FnMut(f64)) -> Result<Vec<Word>, String> {
    // Comme whisper-cli lisant un wav 16 bits.
    let pcm: Vec<f32> = samples.iter().map(|&e| e as f32 / 32768.0).collect();
    let model_c = c_string(&model.to_string_lossy())?;
    let vad_c = c_string(&vad.to_string_lossy())?;
    let language = c_string(LANGUAGE)?;
    let vocabulary = c_string(vocabulary)?;
    let settings = Settings { model: model_c.as_ptr(), vad: vad_c.as_ptr(), language: language.as_ptr(), threads: THREADS, vocabulary: vocabulary.as_ptr() };
    let mut context = Context { words: Vec::new(), progress, stop };
    let mut error = vec![0 as c_char; 512];
    let n = c_int::try_from(pcm.len()).map_err(|_| "réunion trop longue pour whisper.cpp".to_string())?;
    // SAFETY : tous les pointeurs restent valides pendant l'appel, qui est synchrone.
    let code = unsafe {
        greffier_transcribe(&settings, pcm.as_ptr(), n, &mut context as *mut Context as *mut c_void, on_word, on_progress, abandon, error.as_mut_ptr(), error.len() as c_int)
    };
    let message = || unsafe { CStr::from_ptr(error.as_ptr()) }.to_string_lossy().trim().to_string();
    match code {
        0 => Ok(context.words),
        1 => Err(format!("Le modèle de Whisper est illisible ({}) : {}", model.display(), message())),
        3 => Err("arrêté".into()),
        _ => Err(format!("whisper.cpp a échoué : {}", message())),
    }
}
