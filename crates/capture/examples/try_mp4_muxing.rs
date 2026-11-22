//! Turns a medium from the MP4 family into `.mp4` as an import does, without re-encoding.
//!
//! `cargo run -p greffier-capture --example try_mp4_muxing -- <medium> <output.mp4>`

use greffier_core::adapters::Mp4Muxer;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [from, to] = args.as_slice() else {
        eprintln!("usage: try_mp4_muxing <medium> <output.mp4>");
        std::process::exit(2);
    };
    match greffier_capture::MacMp4Muxer.to_mp4(from.as_ref(), to.as_ref()) {
        Ok(()) => println!("written: {to}"),
        Err(e) => {
            eprintln!("failed: {e}");
            std::process::exit(1);
        }
    }
}
