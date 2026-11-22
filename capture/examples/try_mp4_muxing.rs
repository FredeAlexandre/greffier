//! Passe un média de la famille MP4 en `.mp4` comme à l'import, sans réencodage.
//!
//! `cargo run -p greffier-capture --example essai_mise_en_mp4 -- <média> <sortie.mp4>`

use greffier_core::adapters::Mp4Muxer;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [from, to] = args.as_slice() else {
        eprintln!("usage : essai_mise_en_mp4 <média> <sortie.mp4>");
        std::process::exit(2);
    };
    match greffier_capture::MacMp4Muxer.to_mp4(from.as_ref(), to.as_ref()) {
        Ok(()) => println!("écrit : {to}"),
        Err(e) => {
            eprintln!("échec : {e}");
            std::process::exit(1);
        }
    }
}
