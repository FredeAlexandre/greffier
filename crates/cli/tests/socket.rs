//! The process's socket, over a real Unix socket in this process: requests in, responses out,
//! one JSON object per line; a socket left behind; the lock; quitting when idle.

mod common;

use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

use common::Temp;
use greffier_cli::protocol::{Request, Response, encode};
use greffier_cli::server::{self, IdlePolicy, LockError};

/// A listener that answers every request on its own thread, as the test host's loop does: the
/// requests go through a channel, the responses come back through their reply.
fn echo_server(socket: &Path) -> server::Listener {
    let (tx, rx) = mpsc::channel::<(Request, server::Reply)>();
    std::thread::spawn(move || {
        for (request, reply) in rx {
            let response = match request {
                Request::Hello => Response::Ok(Default::default()),
                Request::Status { meeting } => Response::error(format!("status {meeting:?}")),
                other => Response::error(format!("{other:?}")),
            };
            reply(response);
        }
    });
    let tx = std::sync::Mutex::new(tx);
    server::listen(socket, move |request, reply| {
        let _ = tx.lock().unwrap().send((request, reply));
    })
    .expect("listening")
}

struct Client {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
}

impl Client {
    fn new(socket: &Path) -> Client {
        let s = UnixStream::connect(socket).expect("connected");
        s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        Client { reader: BufReader::new(s.try_clone().unwrap()), writer: s }
    }
    fn line(&mut self, line: &str) -> String {
        self.writer.write_all(line.as_bytes()).unwrap();
        let mut answer = String::new();
        self.reader.read_line(&mut answer).expect("an answer");
        answer
    }
    fn ask(&mut self, r: &Request) -> Response {
        serde_json::from_str(&self.line(&encode(r))).expect("a response")
    }
}

#[test]
fn several_requests_on_one_connection() {
    let t = Temp::new();
    let socket = server::socket_path(&t.0);
    let _l = echo_server(&socket);
    let mut c = Client::new(&socket);
    assert_eq!(c.ask(&Request::Hello), Response::Ok(Default::default()));
    assert_eq!(c.ask(&Request::Status { meeting: Some("x".into()) }), Response::error("status Some(\"x\")"));
    assert_eq!(c.ask(&Request::OpenWindow), Response::error("OpenWindow"));
}

#[test]
fn the_wire_format_is_one_json_object_per_line() {
    assert_eq!(encode(&Request::Hello), "\"hello\"\n");
    assert_eq!(encode(&Request::Status { meeting: None }), "{\"status\":{\"meeting\":null}}\n");
    let rename = Request::Execute(greffier_cli::protocol::Action::Rename { meeting: "2026-11-20".into(), title: "Revue".into() });
    assert_eq!(encode(&rename), "{\"execute\":{\"rename\":{\"meeting\":\"2026-11-20\",\"title\":\"Revue\"}}}\n");
    let redo = Request::Execute(greffier_cli::protocol::Action::Redo { meeting: "x".into(), stage: greffier_cli::protocol::Stage::Review });
    assert_eq!(encode(&redo), "{\"execute\":{\"redo\":{\"meeting\":\"x\",\"stage\":\"review\"}}}\n");
    assert_eq!(encode(&Response::error("non")), "{\"error\":{\"message\":\"non\"}}\n");
}

#[test]
fn a_malformed_line_gets_an_error_and_the_connection_goes_on() {
    let t = Temp::new();
    let socket = server::socket_path(&t.0);
    let _l = echo_server(&socket);
    let mut c = Client::new(&socket);
    let answer: Response = serde_json::from_str(&c.line("{\"frobnicate\":1}\n")).unwrap();
    assert!(matches!(&answer, Response::Error { message } if message.starts_with("unreadable request")), "{answer:?}");
    assert_eq!(c.ask(&Request::Hello), Response::Ok(Default::default()));
}

#[test]
fn a_dropped_connection_does_not_stop_the_server() {
    let t = Temp::new();
    let socket = server::socket_path(&t.0);
    let _l = echo_server(&socket);
    {
        let mut s = UnixStream::connect(&socket).unwrap();
        s.write_all(b"\"hel").unwrap();
    }
    {
        // Asks, then leaves before the answer.
        let mut s = UnixStream::connect(&socket).unwrap();
        s.write_all(encode(&Request::Hello).as_bytes()).unwrap();
    }
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(Client::new(&socket).ask(&Request::Hello), Response::Ok(Default::default()));
}

#[test]
fn an_unanswered_request_closes_its_connection() {
    let t = Temp::new();
    let socket = server::socket_path(&t.0);
    // Drops every reply.
    let _l = server::listen(&socket, |_, _| {}).unwrap();
    let mut c = Client::new(&socket);
    c.writer.write_all(encode(&Request::Hello).as_bytes()).unwrap();
    let mut answer = String::new();
    assert_eq!(c.reader.read_line(&mut answer).unwrap(), 0, "closed");
}

#[test]
fn the_socket_is_the_users_only_and_removed_at_the_end() {
    let t = Temp::new();
    let socket = server::socket_path(&t.0);
    let l = echo_server(&socket);
    let mode = std::fs::metadata(&socket).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    drop(l);
    assert!(!socket.exists());
    assert!(UnixStream::connect(&socket).is_err());
}

#[test]
fn a_socket_left_behind_is_replaced() {
    let t = Temp::new();
    let socket = server::socket_path(&t.0);
    // A process that died without removing its socket.
    drop(UnixListener::bind(&socket).unwrap());
    assert!(socket.exists());
    let _l = echo_server(&socket);
    assert_eq!(Client::new(&socket).ask(&Request::Hello), Response::Ok(Default::default()));
}

#[test]
fn a_socket_that_answers_is_not_taken_over() {
    let t = Temp::new();
    let socket = server::socket_path(&t.0);
    let _first = echo_server(&socket);
    let Err(e) = server::listen(&socket, |_, _| {}) else { panic!("two listeners") };
    assert!(e.starts_with("another Greffier process already answers on "), "{e}");
    assert_eq!(Client::new(&socket).ask(&Request::Hello), Response::Ok(Default::default()));
}

#[test]
fn a_socket_path_too_long_is_refused_clearly() {
    let t = Temp::new();
    let deep = t.0.join("x".repeat(120));
    let Err(e) = server::listen(&server::socket_path(&deep), |_, _| {}) else { panic!("bound") };
    assert!(e.starts_with("socket path too long ("), "{e}");
    assert!(!deep.exists(), "nothing created");
}

#[test]
fn one_instance_per_app_folder() {
    let t = Temp::new();
    let lock = server::lock_instance(&t.0).expect("first");
    assert_eq!(server::lock_instance(&t.0).err(), Some(LockError::Held));
    assert_eq!(LockError::Held.to_string(), "Greffier is already running for this app folder");
    let other = Temp::new();
    let _other = server::lock_instance(&other.0).expect("another app folder");
    drop(lock);
    let _again = server::lock_instance(&t.0).expect("free again");
    let pid = std::fs::read_to_string(server::lock_path(&t.0)).unwrap();
    assert_eq!(pid.trim(), std::process::id().to_string());
}

#[test]
fn a_background_process_quits_after_its_idle_time() {
    let mut p = IdlePolicy::new(true, 300.0, 1000.0);
    assert!(!p.should_quit(1299.0, false));
    assert!(p.should_quit(1300.0, false));
    let mut p = IdlePolicy::new(true, 300.0, 0.0);
    // Busy until 200: the wait starts there.
    assert!(!p.should_quit(200.0, true));
    assert!(!p.should_quit(450.0, false));
    // A request at 480 starts it again.
    p.request(480.0);
    assert!(!p.should_quit(700.0, false));
    assert!(p.should_quit(780.0, false));
}

#[test]
fn a_shown_window_or_a_normal_launch_never_quits_on_its_own() {
    let mut shown = IdlePolicy::new(true, 300.0, 0.0);
    assert!(shown.may_quit());
    shown.window_shown();
    assert!(!shown.may_quit());
    assert!(!shown.should_quit(10_000.0, false));
    let mut normal = IdlePolicy::new(false, 300.0, 0.0);
    assert!(!normal.may_quit());
    assert!(!normal.should_quit(10_000.0, false));
    assert_eq!(server::IDLE_QUIT, 300.0);
}
