//! Tient l'assertion de non-veille quelques secondes : `pmset -g assertions` la montre au nom de
//! « Greffier (essai) » (`PreventUserIdleDisplaySleep` avec `--ecran`), puis plus rien.
//!
//! `cargo run -p greffier-capture --example essai_veille [-- secondes] [--ecran]`

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let secondes: u64 = args.iter().find_map(|s| s.parse().ok()).unwrap_or(5);
    let ecran = args.iter().any(|a| a == "--ecran");
    let garde = greffier_capture::veille::PasDeVeille::new("Greffier (essai)", ecran).expect("assertion");
    println!("{} : tenue {secondes} s", garde.raison());
    std::thread::sleep(std::time::Duration::from_secs(secondes));
    drop(garde);
    println!("relâchée");
}
