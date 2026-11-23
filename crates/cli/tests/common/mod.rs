//! Temporary folders for the tests: a copy of the core's sample meetings, a media file to
//! import. Under the system's temporary folder, short enough for a socket path (104 bytes).

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

/// A temporary folder, deleted at the end.
pub struct Temp(pub PathBuf);

impl Temp {
    pub fn new() -> Temp {
        static N: AtomicU32 = AtomicU32::new(0);
        let d = std::env::temp_dir().join(format!("gr-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("temporary folder");
        Temp(d)
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub fn copy_to(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("copy");
    for e in std::fs::read_dir(from).expect("fixtures").flatten() {
        let target = to.join(e.file_name());
        if e.file_type().expect("type").is_dir() { copy_to(&e.path(), &target) } else { std::fs::copy(e.path(), target).map(|_| ()).expect("copy") }
    }
}

/// A copy of the core's sample meetings folder.
pub fn fixtures() -> Temp {
    let t = Temp::new();
    copy_to(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../core/tests/fixtures/meetings-folder"), &t.0);
    t
}

/// A media file to import (1 s of sound, MP4), outside any meetings folder.
pub fn media() -> (Temp, PathBuf) {
    let t = Temp::new();
    let file = t.0.join("Appel client.mp4");
    std::fs::copy(Path::new(env!("CARGO_MANIFEST_DIR")).join("../core/tests/fixtures/mp4/audio-only.mp4"), &file).expect("media");
    (t, file)
}

/// A Greffier process in a thread of this one: a core on `folder` with these adapters, the
/// socket in `app`, time moving `speed` times faster than the clock.
pub struct Served {
    pub folder: Temp,
    pub app: Temp,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Served {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

struct LoopHost(greffier_core::Greffier);

impl greffier_cli::server::Host for LoopHost {
    fn core(&mut self) -> &mut greffier_core::Greffier {
        &mut self.0
    }
    fn background(&self) -> bool {
        true
    }
}

pub fn serve(folder: Temp, adapters: fn() -> greffier_core::adapters::Adapters, speed: f64) -> Served {
    use std::sync::{Arc, Mutex, atomic::AtomicBool, mpsc};
    use std::time::{Duration, Instant};
    let app = Temp::new();
    let socket = greffier_cli::server::socket_path(&app.0);
    let root = folder.0.clone();
    let stop = Arc::new(AtomicBool::new(false));
    let stopping = stop.clone();
    let (ready_tx, ready) = mpsc::channel();
    let thread = std::thread::spawn(move || {
        let mut host = LoopHost(greffier_core::Greffier::open(&root, adapters()));
        let (tx, rx) = mpsc::channel::<(greffier_cli::protocol::Request, greffier_cli::server::Reply)>();
        let tx = Mutex::new(tx);
        let _listener = greffier_cli::server::listen(&socket, move |r, reply| {
            let _ = tx.lock().unwrap().send((r, reply));
        })
        .expect("listening");
        ready_tx.send(()).unwrap();
        let mut last = Instant::now();
        while !stopping.load(Ordering::SeqCst) {
            if let Ok((request, reply)) = rx.recv_timeout(Duration::from_millis(10)) {
                reply(greffier_cli::server::answer(&mut host, request));
            }
            let now = Instant::now();
            host.0.advance((now - last).as_secs_f64() * speed);
            last = now;
        }
    });
    ready.recv().expect("served");
    Served { folder, app, stop, thread: Some(thread) }
}

/// The demo's fake adapters: a processing of 15 s of the core's time.
pub fn adapters() -> greffier_core::adapters::Adapters {
    greffier_core::demo::adapters()
}

/// The demo's fake adapters, with a transcription that fails.
pub fn failing_adapters() -> greffier_core::adapters::Adapters {
    let mut a = greffier_core::demo::adapters();
    a.transcriber = Box::new(greffier_core::fake::FakeTranscriber::new(1.0, |_| Err("whisper a planté".into())));
    a
}

/// What a command printed, and its exit code.
pub struct Output {
    pub code: i32,
    pub out: String,
    pub err: String,
}

pub fn run_in(cx: &greffier_cli::Context, args: &[&str]) -> Output {
    let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = greffier_cli::run_with(cx, &args, &mut out, &mut err);
    Output { code, out: String::from_utf8(out).expect("UTF-8"), err: String::from_utf8(err).expect("UTF-8") }
}
