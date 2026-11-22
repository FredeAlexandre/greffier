//! Holds the sleep assertion for a few seconds: `pmset -g assertions` shows it under the name
//! "Greffier (test)" (`PreventUserIdleDisplaySleep` with `--screen`), then nothing.
//!
//! `cargo run -p greffier-capture --example try_sleep [-- seconds] [--screen]`

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let seconds: u64 = args.iter().find_map(|s| s.parse().ok()).unwrap_or(5);
    let screen = args.iter().any(|a| a == "--screen");
    let guard = greffier_capture::sleep::KeepAwake::new("Greffier (test)", screen).expect("assertion");
    println!("{}: held {seconds} s", guard.reason());
    std::thread::sleep(std::time::Duration::from_secs(seconds));
    drop(guard);
    println!("released");
}
