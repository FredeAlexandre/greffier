//! Greffier's binary: with a command (`greffier list`, `show`, `status`, `help`…) the command
//! line (`greffier-cli`), which reads the meetings folder without opening the window; otherwise
//! the app (see the library, `lib.rs`): its window, Rune's flags (`--snap`, `--scene`…)
//! included, or `--background` without a window, as the command line launches it.

#![forbid(unsafe_code)]

fn main() {
    let args: Vec<String> = std::env::args_os().skip(1).map(|a| a.to_string_lossy().into_owned()).collect();
    if greffier_cli::is_command(&args) {
        use std::io::Write;
        let mut out = std::io::stdout().lock();
        let code = greffier_cli::run(&args, &mut out, &mut std::io::stderr().lock());
        let _ = out.flush();
        std::process::exit(code);
    }
    greffier::run(&args);
}
