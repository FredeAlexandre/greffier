//! Passe un média de la famille MP4 en `.mp4` comme à l'import, sans réencodage.
//!
//! `cargo run -p greffier-capture --example essai_mise_en_mp4 -- <média> <sortie.mp4>`

use greffier_coeur::adaptateurs::MiseEnMp4;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [de, vers] = args.as_slice() else {
        eprintln!("usage : essai_mise_en_mp4 <média> <sortie.mp4>");
        std::process::exit(2);
    };
    match greffier_capture::MiseEnMp4DuMac.en_mp4(de.as_ref(), vers.as_ref()) {
        Ok(()) => println!("écrit : {vers}"),
        Err(e) => {
            eprintln!("échec : {e}");
            std::process::exit(1);
        }
    }
}
