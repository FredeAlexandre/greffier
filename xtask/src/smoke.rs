//! `cargo xtask smoke [--with-claude] [--only SUITE]`: runs the `#[ignore]`d tests against the real
//! Mac, which `cargo test` leaves out: the capture (ScreenCaptureKit, the MP4 read back by
//! ffprobe, the keep-awake assertion), the engines (whisper.cpp, pyannote) and, with
//! `--with-claude` because it costs tokens, the review and summary by the `claude` CLI. Their
//! output goes by as it comes; a summary follows, with the tests skipped for lack of a
//! prerequisite (model, meeting audio, ffprobe, signed-in `claude`).

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};
use std::time::Instant;

use crate::{Failure, Result, root, step};

/// A suite: its name, the crate and the test file, and the extra test-harness arguments.
struct Suite {
    name: &'static str,
    package: &'static str,
    test: &'static str,
    harness: &'static [&'static str],
}

const SUITES: [Suite; 3] = [
    // One recording at a time; the child process of the cut-recording test does nothing alone.
    Suite { name: "capture", package: "greffier-capture", test: "real_capture", harness: &["--test-threads=1", "--skip", "child_process_"] },
    Suite { name: "engines", package: "greffier-engines", test: "real_engines", harness: &[] },
    Suite { name: "claude", package: "greffier-tools", test: "real_claude", harness: &[] },
];

struct Outcome {
    name: &'static str,
    passed: bool,
    result: String,
    skipped: Vec<String>,
    seconds: f64,
}

pub fn main(args: &[String]) -> Result<()> {
    let mut with_claude = false;
    let mut only: Option<String> = None;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--with-claude" => with_claude = true,
            "--only" => only = Some(args.next().ok_or_else(|| Failure("--only needs a suite: capture, engines or claude.".into()))?.clone()),
            other => return Err(Failure(format!("unexpected argument \"{other}\" (--with-claude, --only capture|engines|claude)."))),
        }
    }
    if let Some(o) = &only
        && !SUITES.iter().any(|s| s.name == o)
    {
        return Err(Failure(format!("unknown suite \"{o}\": capture, engines or claude.")));
    }
    let chosen: Vec<&Suite> = SUITES.iter().filter(|s| only.as_deref().map_or(s.name != "claude" || with_claude, |o| o == s.name)).collect();

    let mut outcomes = Vec::new();
    for suite in &chosen {
        step(&format!("Smoke tests: {} ({})", suite.name, suite.test));
        outcomes.push(run(suite)?);
    }

    println!();
    step("Summary");
    for o in &outcomes {
        println!("  {:<8} {}  {} ({:.0} s)", o.name, if o.passed { "ok    " } else { "FAILED" }, o.result, o.seconds);
        for s in &o.skipped {
            println!("           skipped: {s}");
        }
    }
    if only.is_none() && !with_claude {
        println!("  claude   not run (--with-claude runs it: costs Claude tokens)");
    }
    let failed: Vec<&str> = outcomes.iter().filter(|o| !o.passed).map(|o| o.name).collect();
    if failed.is_empty() { Ok(()) } else { Err(Failure(format!("failed: {} (see above).", failed.join(", ")))) }
}

/// Runs one suite's ignored tests, echoing their output, and reads their outcome from it.
fn run(suite: &Suite) -> Result<Outcome> {
    let start = Instant::now();
    let mut child = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
        .args(["test", "--package", suite.package, "--test", suite.test, "--", "--ignored", "--nocapture"])
        .args(suite.harness)
        .current_dir(root())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| Failure(format!("cargo test could not be started: {e}")))?;
    let echo = |stream: Box<dyn Read + Send>, err: bool| {
        std::thread::spawn(move || {
            let mut lines = Vec::new();
            for line in BufReader::new(stream).lines().map_while(std::result::Result::ok) {
                if err {
                    let _ = writeln!(std::io::stderr(), "{line}");
                } else {
                    let _ = writeln!(std::io::stdout(), "{line}");
                }
                lines.push(line);
            }
            lines
        })
    };
    let out = echo(Box::new(child.stdout.take().expect("stdout")), false);
    let err = echo(Box::new(child.stderr.take().expect("stderr")), true);
    let status = child.wait()?;
    let lines: Vec<String> = out.join().unwrap_or_default().into_iter().chain(err.join().unwrap_or_default()).collect();
    let result = lines.iter().rev().find_map(|l| l.strip_prefix("test result: ")).map_or_else(|| "did not run (build failure?)".to_string(), |r| {
        // "ok. 5 passed; 0 failed; 0 ignored; …" → "5 passed; 0 failed"
        let r = r.split_once(". ").map_or(r, |(_, counts)| counts);
        r.split(';').take(2).collect::<Vec<_>>().join(";")
    });
    let skipped = lines.iter().filter_map(|l| l.split_once("SKIPPED: ").map(|(_, why)| why.to_string())).collect();
    Ok(Outcome { name: suite.name, passed: status.success(), result, skipped, seconds: start.elapsed().as_secs_f64() })
}
