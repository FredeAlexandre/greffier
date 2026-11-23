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
