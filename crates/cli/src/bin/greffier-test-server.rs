//! A Greffier process for the tests, without a window: it exists so that the command line's
//! end-to-end tests can launch a real process (`GREFFIER_SERVER_PROGRAM`) and talk to it over
//! the socket, as they will to the app. It takes the app folder's lock, listens on its socket
//! and drives a core on the demo's fake adapters (a processing completes in 15 s of the core's
//! time), the way the app does with [`greffier_cli::server`].
//!
//! Its folders come from `GREFFIER_FOLDER` and `GREFFIER_APP_FOLDER`, both required: it never
//! touches the real ones. `GREFFIER_TEST_SPEED` makes the core's time run that many times
//! faster than the clock (10 by default); `GREFFIER_TEST_IDLE` is the idle time after which it
//! quits when started with `--background` (300 s by default).

#![forbid(unsafe_code)]

use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use greffier_cli::server::{self, Host, IdlePolicy};
use greffier_core::Greffier;

struct TestHost {
    core: Greffier,
    background: bool,
    idle: IdlePolicy,
}

impl Host for TestHost {
    fn core(&mut self) -> &mut Greffier {
        &mut self.core
    }
    fn background(&self) -> bool {
        self.background
    }
    fn open_window(&mut self) {
        self.idle.window_shown();
    }
}

fn number(variable: &str, default: f64) -> f64 {
    std::env::var(variable).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

fn fail(message: impl std::fmt::Display) -> ! {
    eprintln!("greffier-test-server : {message}");
    std::process::exit(1);
}

fn main() {
    let background = std::env::args().any(|a| a == "--background");
    let Some(folder) = greffier_core::settings::replaced_meetings_folder() else { fail("GREFFIER_FOLDER manque") };
    if std::env::var_os(greffier_core::settings::APP_FOLDER_VARIABLE).is_none_or(|v| v.is_empty()) {
        fail("GREFFIER_APP_FOLDER manque");
    }
    let app = greffier_core::settings::app_folder();
    let speed = number("GREFFIER_TEST_SPEED", 10.0);
    let idle = number("GREFFIER_TEST_IDLE", server::IDLE_QUIT);
    let _lock = server::lock_instance(&app).unwrap_or_else(|e| fail(e));
    let (tx, rx) = mpsc::channel();
    let _listener = server::listen(&server::socket_path(&app), move |request, reply| {
        let _ = tx.send((request, reply));
    })
    .unwrap_or_else(|e| fail(e));
    let start = Instant::now();
    let now = || start.elapsed().as_secs_f64();
    let mut host = TestHost { core: Greffier::open(&folder, greffier_core::demo::adapters()), background, idle: IdlePolicy::new(background, idle, now()) };
    let mut last = now();
    loop {
        match rx.recv_timeout(Duration::from_millis(20)) {
            Ok((request, reply)) => {
                host.idle.request(now());
                reply(server::answer(&mut host, request));
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        let t = now();
        host.core.advance((t - last) * speed);
        last = t;
        if host.idle.should_quit(t, host.core.busy()) {
            break;
        }
    }
}
