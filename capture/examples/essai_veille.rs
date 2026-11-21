//! Tient l'assertion de non-veille quelques secondes : `pmset -g assertions` la montre au nom de
//! « Greffier (essai) », puis plus rien.
//!
//! `cargo run -p greffier-capture --example essai_veille [-- secondes]`

fn main() {
    let secondes: u64 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(5);
    let garde = greffier_capture::veille::PasDeVeille::new("Greffier (essai)").expect("assertion");
    println!("{} : tenue {secondes} s", garde.raison());
    std::thread::sleep(std::time::Duration::from_secs(secondes));
    drop(garde);
    println!("relâchée");
}
