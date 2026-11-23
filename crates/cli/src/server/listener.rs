//! The socket the process listens on, and the lock that keeps one process per app folder.

use std::fs::{File, OpenOptions, TryLockError};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;

use crate::protocol::{Request, Response, encode};

/// The socket, in the app folder.
pub fn socket_path(app_folder: &Path) -> PathBuf {
    app_folder.join("greffier.sock")
}

/// The lock file, in the app folder.
pub fn lock_path(app_folder: &Path) -> PathBuf {
    app_folder.join("greffier.lock")
}

/// The longest socket path macOS takes, in bytes (`sun_path` holds 104, its final zero included).
pub const MAX_SOCKET_PATH: usize = 103;

/// The longest request line read, in bytes: beyond, the connection is closed.
const MAX_LINE: u64 = 1 << 20;

/// Hands the response to a request back to its connection. Dropped without being called, the
/// connection closes.
pub type Reply = Box<dyn FnOnce(Response) + Send>;

/// The socket being listened on; dropping it stops accepting and removes the socket file.
pub struct Listener {
    socket: PathBuf,
    stopped: Arc<AtomicBool>,
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::SeqCst);
        // Wakes the accepting thread up, which then stops.
        let _ = UnixStream::connect(&self.socket);
        let _ = std::fs::remove_file(&self.socket);
    }
}

/// Listens on `socket`: each connection gets a thread that reads one request per line and
/// hands it to `deliver` with the means to answer it; the response goes back on the same
/// connection before the next request is read. A malformed line gets an error response; a
/// dropped connection only ends its own thread.
///
/// A socket file left behind by a dead process is removed; one that answers is not (another
/// process listens there). The socket is only open to the user (0600).
pub fn listen(socket: &Path, deliver: impl Fn(Request, Reply) + Send + Sync + 'static) -> Result<Listener, String> {
    if socket.as_os_str().len() > MAX_SOCKET_PATH {
        return Err(format!("chemin du socket trop long ({} octets, {MAX_SOCKET_PATH} au plus) : {}", socket.as_os_str().len(), socket.display()));
    }
    if let Some(parent) = socket.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{} : {e}", parent.display()))?;
    }
    if std::fs::symlink_metadata(socket).is_ok() {
        if UnixStream::connect(socket).is_ok() {
            return Err(format!("un autre processus Greffier répond déjà sur {}", socket.display()));
        }
        std::fs::remove_file(socket).map_err(|e| format!("{} : {e}", socket.display()))?;
    }
    let listener = UnixListener::bind(socket).map_err(|e| format!("{} : {e}", socket.display()))?;
    std::fs::set_permissions(socket, std::fs::Permissions::from_mode(0o600)).map_err(|e| format!("{} : {e}", socket.display()))?;
    let stopped = Arc::new(AtomicBool::new(false));
    let deliver = Arc::new(deliver);
    let stop = stopped.clone();
    std::thread::Builder::new()
        .name("greffier-socket".into())
        .spawn(move || {
            for stream in listener.incoming() {
                if stop.load(Ordering::SeqCst) {
                    break;
                }
                let Ok(stream) = stream else { continue };
                let deliver = deliver.clone();
                let _ = std::thread::Builder::new().name("greffier-connection".into()).spawn(move || serve(stream, &*deliver));
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(Listener { socket: socket.to_path_buf(), stopped })
}

/// Answers the requests of one connection, one per line, until it closes.
fn serve(stream: UnixStream, deliver: &(impl Fn(Request, Reply) + ?Sized)) {
    let Ok(reading) = stream.try_clone() else { return };
    let mut reader = BufReader::new(reading);
    let mut writer = stream;
    loop {
        let mut line = String::new();
        match (&mut reader).take(MAX_LINE).read_line(&mut line) {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        if !line.ends_with('\n') && line.len() as u64 >= MAX_LINE {
            let _ = writer.write_all(encode(&Response::error("requête trop longue")).as_bytes());
            return;
        }
        if line.trim().is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<Request>(&line) {
            Err(e) => Response::error(format!("requête illisible : {e}")),
            Ok(request) => {
                let (tx, rx) = mpsc::channel();
                deliver(
                    request,
                    Box::new(move |r| {
                        let _ = tx.send(r);
                    }),
                );
                match rx.recv() {
                    Ok(r) => r,
                    Err(_) => return,
                }
            }
        };
        if writer.write_all(encode(&response).as_bytes()).is_err() {
            return;
        }
    }
}

/// Held for the process's life: no other process takes the same app folder.
pub struct InstanceLock {
    _file: File,
}

/// Why the lock was not taken.
#[derive(Debug, PartialEq)]
pub enum LockError {
    /// Another instance holds it.
    Held,
    Failed(String),
}

impl std::fmt::Display for LockError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LockError::Held => f.write_str("Greffier tourne déjà pour ce dossier de l'app"),
            LockError::Failed(m) => f.write_str(m),
        }
    }
}

/// Takes the lock of this app folder (`greffier.lock`, created if missing), with the process's
/// pid written in it; [`LockError::Held`] if another instance holds it. Released when the
/// [`InstanceLock`] is dropped, or the process ends.
pub fn lock_instance(app_folder: &Path) -> Result<InstanceLock, LockError> {
    std::fs::create_dir_all(app_folder).map_err(|e| LockError::Failed(format!("{} : {e}", app_folder.display())))?;
    let path = lock_path(app_folder);
    let failed = |e: std::io::Error| LockError::Failed(format!("{} : {e}", path.display()));
    let mut file = OpenOptions::new().create(true).truncate(false).read(true).write(true).open(&path).map_err(failed)?;
    match file.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => return Err(LockError::Held),
        Err(TryLockError::Error(e)) => return Err(failed(e)),
    }
    let _ = file.set_len(0).and_then(|()| writeln!(file, "{}", std::process::id()));
    Ok(InstanceLock { _file: file })
}
