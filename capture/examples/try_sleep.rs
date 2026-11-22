//! Tient l'assertion de non-veille quelques secondes : `pmset -g assertions` la montre au nom de
//! « Greffier (essai) » (`PreventUserIdleDisplaySleep` avec `--ecran`), puis plus rien.
//!
//! `cargo run -p greffier-capture --example essai_veille [-- secondes] [--ecran]`

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let seconds: u64 = args.iter().find_map(|s| s.parse().ok()).unwrap_or(5);
    let screen = args.iter().any(|a| a == "--ecran");
    let guard = greffier_capture::sleep::KeepAwake::new("Greffier (essai)", screen).expect("assertion");
    println!("{} : tenue {seconds} s", guard.reason());
    std::thread::sleep(std::time::Duration::from_secs(seconds));
    drop(guard);
    println!("relâchée");
}
