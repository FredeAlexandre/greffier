//! Smoke tests of the real capture on this Mac, run on purpose with `cargo xtask smoke`
//! (`#[ignore]`d by `cargo test`): ScreenCaptureKit and AVAssetWriter record a few seconds, then
//! ffprobe reads the file back; the keep-awake assertion is looked for in `pmset -g assertions`.
//!
//! They need the "Screen & System Audio Recording" and "Microphone" permissions for the app that
//! runs them (Terminal, the IDE: macOS asks the app at the top of the process tree), and ffprobe /
//! ffmpeg (`brew install ffmpeg`): without ffprobe they are skipped, without a permission they
//! fail and say which one to grant. They record the main screen and the microphone, and one of
//! them plays a short tone on the Mac's output.
#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use greffier_capture::sleep::KeepAwake;
use greffier_capture::writer::FRAGMENT_INTERVAL;
use greffier_capture::MacCapture;
use greffier_core::adapters::{Capture, CaptureRequest, PermissionState, Sources};
use greffier_core::{Command as CoreCommand, Greffier};
use objc2_core_graphics::{CGDisplayPixelsHigh, CGDisplayPixelsWide, CGMainDisplayID};
use serde_json::Value;

/// How long each recording lasts.
const SECONDS: f64 = 5.0;
/// ffmpeg's volumedetect gives -91 dB for digital silence (all samples at zero).
const SILENCE_DB: f64 = -90.0;
/// A crash loses at most this much of the recording: the fragment being written (2 s) and the
/// encoder's latency.
const MAX_LOST: f64 = 3.0;
/// The child process of [`a_recording_cut_without_stopping_stays_readable`] records into this file.
const CUT_FILE: &str = "GREFFIER_SMOKE_CUT_FILE";

/// One ScreenCaptureKit stream at a time, even if the tests run in parallel.
static CAPTURE: Mutex<()> = Mutex::new(());

#[test]
#[ignore = "real Mac: records the screen and the microphone (cargo xtask smoke)"]
fn a_recording_is_h264_within_1080p_at_30_fps_with_three_aac_tracks() {
    let Some(ffprobe) = program("ffprobe") else { return skip("ffprobe not found (brew install ffmpeg)") };
    let _one = one_capture();
    let temp = TempDir::new("format");
    let file = temp.0.join("recording.mp4");
    let returned = record(&file, Sources::default(), || ());
    let probe = probe(&ffprobe, &file);

    let streams = probe["streams"].as_array().expect("streams");
    let video: Vec<&Value> = streams.iter().filter(|s| s["codec_type"] == "video").collect();
    let audio: Vec<&Value> = streams.iter().filter(|s| s["codec_type"] == "audio").collect();
    assert_eq!(video.len(), 1, "one video track: {streams:#?}");
    let v = video[0];
    assert_eq!(v["codec_name"], "h264", "{v:#?}");

    // The main screen, fitted in 1920 × 1080, aspect ratio kept, even dimensions.
    let (w, h) = (int(&v["width"]), int(&v["height"]));
    let main = CGMainDisplayID();
    let (sw, sh) = (CGDisplayPixelsWide(main) as f64, CGDisplayPixelsHigh(main) as f64);
    assert!(w <= 1920 && h <= 1080, "{w}×{h} does not fit in 1920×1080");
    assert!(w % 2 == 0 && h % 2 == 0, "{w}×{h}: odd dimension");
    let (ratio, screen_ratio) = (w as f64 / h as f64, sw / sh);
    assert!((ratio / screen_ratio - 1.0).abs() < 0.005, "{w}×{h} (ratio {ratio:.4}) does not keep the screen's {sw}×{sh} (ratio {screen_ratio:.4})");
    let fills = w >= 1918 || h >= 1078 || (w as f64 - sw).abs() <= 1.0;
    assert!(fills, "{w}×{h}: smaller than needed to fit the {sw}×{sh} screen in 1080p");

    // 30 fps, constant: as many frames as 30 per second of video.
    assert_eq!(v["r_frame_rate"], "30/1", "{v:#?}");
    let video_duration = number(&v["duration"]);
    let frames = int(&v["nb_read_packets"]) as f64;
    assert!((frames - 30.0 * video_duration).abs() <= 3.0, "{frames} frames in {video_duration:.2} s: not 30 fps");

    // Three AAC tracks, 48 kHz stereo: the mix (the only default track, the one a player plays),
    // then the microphone, then the Mac audio.
    assert_eq!(audio.len(), 3, "three audio tracks: {streams:#?}");
    for (i, a) in audio.iter().enumerate() {
        assert_eq!((a["codec_name"].as_str(), a["sample_rate"].as_str(), a["channels"].as_i64()), (Some("aac"), Some("48000"), Some(2)), "audio track {i}: {a:#?}");
    }
    let defaults: Vec<i64> = audio.iter().map(|a| int(&a["disposition"]["default"])).collect();
    assert_eq!(defaults, [1, 0, 0], "only the mix (first audio track) plays by default");

    // The length recorded, the one stop() announced, and the tracks', all agree.
    let duration = number(&probe["format"]["duration"]);
    assert!((SECONDS - 0.5..=SECONDS + 1.0).contains(&duration), "{duration:.2} s recorded for {SECONDS} s");
    assert!((returned - duration).abs() < 0.3, "stop() announced {returned:.2} s, the file lasts {duration:.2} s");
    for s in streams {
        let d = number(&s["duration"]);
        assert!((d - duration).abs() < 0.3, "track {} lasts {d:.2} s, the file {duration:.2} s", s["index"]);
    }
}

#[test]
#[ignore = "real Mac: records the screen and the microphone (cargo xtask smoke)"]
fn the_microphone_goes_to_the_mix_and_to_its_own_track_only() {
    let Some(ffmpeg) = program("ffmpeg") else { return skip("ffmpeg not found (brew install ffmpeg)") };
    let _one = one_capture();
    let temp = TempDir::new("micro");
    let file = temp.0.join("recording.mp4");
    record(&file, Sources { screen: true, micro: true, mac_audio: false }, || ());

    // A microphone always hears something (its noise floor); a track without a source is zeros.
    let [mix, micro, mac_audio] = [0, 1, 2].map(|i| max_volume(&ffmpeg, &file, i));
    eprintln!("loudest sample: mix {mix} dB, microphone {micro} dB, Mac audio {mac_audio} dB");
    assert!(mac_audio <= SILENCE_DB, "the Mac audio track (third) hears something ({mac_audio} dB) without the Mac audio source: are the tracks in the order mix, microphone, Mac audio?");
    assert!(micro > SILENCE_DB, "the microphone track is silent ({micro} dB): is the microphone muted or disconnected?");
    assert!(mix > SILENCE_DB, "the mix (first track) is silent ({mix} dB) while the microphone track is not ({micro} dB)");
}

#[test]
#[ignore = "real Mac: records the screen and the Mac audio, plays a tone (cargo xtask smoke)"]
fn the_mac_audio_goes_to_the_mix_and_to_its_own_track_only() {
    let Some(ffmpeg) = program("ffmpeg") else { return skip("ffmpeg not found (brew install ffmpeg)") };
    let _one = one_capture();
    let temp = TempDir::new("mac-audio");
    let (file, tone) = (temp.0.join("recording.mp4"), temp.0.join("tone.wav"));
    write_tone(&tone, SECONDS - 1.0);
    let mut player = None;
    record(&file, Sources { screen: true, micro: false, mac_audio: true }, || {
        player = Some(Command::new("/usr/bin/afplay").arg(&tone).spawn().expect("afplay"));
    });
    if let Some(mut p) = player {
        let _ = p.kill();
        let _ = p.wait();
    }

    // The tone is written at -20 dBFS: the Mac audio track and the mix carry it, the microphone
    // track (no source) is zeros.
    let [mix, micro, mac_audio] = [0, 1, 2].map(|i| max_volume(&ffmpeg, &file, i));
    eprintln!("loudest sample: mix {mix} dB, microphone {micro} dB, Mac audio {mac_audio} dB");
    assert!(micro <= SILENCE_DB, "the microphone track (second) hears something ({micro} dB) without the microphone source: are the tracks in the order mix, microphone, Mac audio?");
    assert!(
        mac_audio > -40.0,
        "the Mac audio track (third) is silent ({mac_audio} dB) while afplay played a tone. Either « {} » lacks the system audio permission — System Settings › Privacy & Security › Screen & System Audio Recording, in both lists (« Screen & System Audio Recording » and « System Audio Recording Only ») — or the capture lost the Mac audio.",
        responsible_app()
    );
    assert!(mix > -40.0, "the mix (first track) is silent ({mix} dB) while the Mac audio track carries the tone ({mac_audio} dB)");
}

#[test]
#[ignore = "real Mac: records the screen and the microphone in a child process (cargo xtask smoke)"]
fn a_recording_cut_without_stopping_stays_readable() {
    let Some(ffprobe) = program("ffprobe") else { return skip("ffprobe not found (brew install ffmpeg)") };
    let _one = one_capture();
    require_permissions(Sources::default());
    let temp = TempDir::new("cut");
    let file = temp.0.join("recording.mp4");
    // The app dies in the middle of a recording: a child process records, then quits without
    // stopping the capture or finishing the file.
    let status = Command::new(std::env::current_exe().expect("test binary"))
        .args(["--ignored", "--exact", "child_process_records_then_quits_without_stopping", "--nocapture", "--test-threads=1"])
        .env(CUT_FILE, &file)
        .status()
        .expect("child process");
    assert!(status.success(), "the child process failed ({status}): see its output above");

    let probe = probe(&ffprobe, &file);
    let streams = probe["streams"].as_array().expect("streams");
    let kinds: Vec<&str> = streams.iter().filter_map(|s| s["codec_type"].as_str()).collect();
    assert_eq!(kinds, ["video", "audio", "audio", "audio"], "{streams:#?}");
    // What the app reads to show the meeting's length: everything up to the last complete
    // fragment.
    let duration = greffier_core::folder::mp4_duration(&file).expect("the core reads the cut file's duration");
    eprintln!("{duration:.2} s kept of {SECONDS} s recorded (fragments of {FRAGMENT_INTERVAL} s)");
    assert!((SECONDS - MAX_LOST..=SECONDS + 0.5).contains(&duration), "{duration:.2} s kept of {SECONDS} s recorded: a crash should lose at most the last {MAX_LOST} s");
}

#[test]
#[ignore = "child process of a_recording_cut_without_stopping_stays_readable: does nothing on its own"]
fn child_process_records_then_quits_without_stopping() {
    let Some(file) = std::env::var_os(CUT_FILE).map(PathBuf::from) else { return };
    let mut capture = MacCapture::new();
    capture.start(CaptureRequest { file, sources: Sources::default() }).unwrap_or_else(|e| panic!("{e}"));
    let start = Instant::now();
    while start.elapsed().as_secs_f64() < SECONDS {
        std::thread::sleep(Duration::from_millis(250));
        if let Some(e) = capture.poll(0.25).error {
            panic!("{e}");
        }
    }
    std::process::exit(0);
}

#[test]
#[ignore = "real Mac: takes the sleep assertions and reads pmset (cargo xtask smoke)"]
fn the_mac_stays_awake_while_recording_and_processing_then_no_longer() {
    // What the core asks for, on fake adapters; the real assertion, read back from pmset.
    let temp = TempDir::new("awake");
    let mut greffier = Greffier::open(&temp.0, greffier_core::demo::adapters());
    greffier.execute(CoreCommand::StartRecording).expect("recording");
    let recording = greffier.keep_mac_awake().expect("the Mac stays awake during a recording");
    assert!(recording.screen, "the display stays on during a recording: ScreenCaptureKit captures nothing once it is off");
    let held = KeepAwake::new(recording.reason, recording.screen).expect("assertion");
    assert_eq!(assertions(recording.reason), ["PreventUserIdleDisplaySleep"], "pmset -g assertions:\n{}", pmset());
    drop(held);
    assert_released(recording.reason);

    greffier.execute(CoreCommand::StopRecording).expect("stop");
    let processing = greffier.keep_mac_awake().expect("the Mac stays awake while the meeting is processed");
    assert!(!processing.screen, "the display may turn off while processing");
    let held = KeepAwake::new(processing.reason, processing.screen).expect("assertion");
    assert_eq!(assertions(processing.reason), ["PreventUserIdleSystemSleep"], "pmset -g assertions:\n{}", pmset());
    drop(held);
    assert_released(processing.reason);
}

// --- Recording ---

fn one_capture() -> std::sync::MutexGuard<'static, ()> {
    CAPTURE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Records [`SECONDS`] with the real capture; `started` runs once the capture has started.
/// Returns the duration announced by `stop()`.
fn record(file: &Path, sources: Sources, started: impl FnOnce()) -> f64 {
    require_permissions(sources);
    let mut capture = MacCapture::new();
    capture.start(CaptureRequest { file: file.to_path_buf(), sources }).unwrap_or_else(|e| panic!("the capture does not start: {e}"));
    started();
    let start = Instant::now();
    while start.elapsed().as_secs_f64() < SECONDS {
        std::thread::sleep(Duration::from_millis(250));
        if let Some(e) = capture.poll(0.25).error {
            panic!("{e}");
        }
    }
    capture.stop().unwrap_or_else(|e| panic!("the capture does not stop: {e}"))
}

/// Fails, saying what to grant to whom, if a permission the sources need is missing.
fn require_permissions(sources: Sources) {
    let p = MacCapture::new().permissions();
    let app = responsible_app();
    if (sources.screen || sources.mac_audio) && p.screen != PermissionState::Granted {
        panic!("screen recording is not allowed ({:?}): grant « {app} » the « Screen & System Audio Recording » permission in System Settings › Privacy & Security, then restart it.", p.screen);
    }
    if sources.micro && p.micro != PermissionState::Granted {
        panic!("the microphone is not allowed ({:?}): grant « {app} » the « Microphone » permission in System Settings › Privacy & Security, then restart it.", p.micro);
    }
}

/// The app macOS asks for the permissions: the first `.app` among the test's ancestors.
fn responsible_app() -> String {
    let mut pid = std::process::id();
    for _ in 0..30 {
        let Ok(out) = Command::new("/bin/ps").args(["-o", "ppid=,comm=", "-p", &pid.to_string()]).output() else { break };
        let line = String::from_utf8_lossy(&out.stdout).trim().to_string();
        let Some((parent, command)) = line.split_once(' ') else { break };
        if let Some(i) = command.find(".app/") {
            return command[..i].rsplit('/').next().unwrap_or(command).to_string();
        }
        match parent.trim().parse() {
            Ok(p) if p > 1 => pid = p,
            _ => break,
        }
    }
    "the app that runs the tests (Terminal, your IDE)".into()
}

/// A 880 Hz tone at -20 dBFS, 48 kHz 16-bit stereo WAV.
fn write_tone(file: &Path, seconds: f64) {
    let rate = 48_000u32;
    let n = (seconds * f64::from(rate)) as u32;
    let mut b = Vec::with_capacity(44 + n as usize * 4);
    let data = n * 4;
    b.extend(b"RIFF");
    b.extend((36 + data).to_le_bytes());
    b.extend(b"WAVEfmt ");
    b.extend(16u32.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    b.extend(2u16.to_le_bytes());
    b.extend(rate.to_le_bytes());
    b.extend((rate * 4).to_le_bytes());
    b.extend(4u16.to_le_bytes());
    b.extend(16u16.to_le_bytes());
    b.extend(b"data");
    b.extend(data.to_le_bytes());
    for i in 0..n {
        let s = ((2.0 * std::f64::consts::PI * 880.0 * f64::from(i) / f64::from(rate)).sin() * 0.1 * 32767.0) as i16;
        b.extend(s.to_le_bytes());
        b.extend(s.to_le_bytes());
    }
    std::fs::write(file, b).expect("tone");
}

// --- Reading the file back ---

/// ffprobe's view of the file: streams (with their packet count) and container.
fn probe(ffprobe: &Path, file: &Path) -> Value {
    let out = Command::new(ffprobe).args(["-v", "error", "-print_format", "json", "-show_streams", "-show_format", "-count_packets"]).arg(file).output().expect("ffprobe");
    assert!(out.status.success(), "ffprobe cannot read {}: {}", file.display(), String::from_utf8_lossy(&out.stderr));
    serde_json::from_slice(&out.stdout).expect("ffprobe JSON")
}

/// The loudest sample of the `index`-th audio track, in dBFS (ffmpeg's volumedetect).
fn max_volume(ffmpeg: &Path, file: &Path, index: usize) -> f64 {
    let out = Command::new(ffmpeg).args(["-nostats", "-hide_banner", "-i"]).arg(file).args(["-map", &format!("0:a:{index}"), "-af", "volumedetect", "-f", "null", "-"]).output().expect("ffmpeg");
    let log = String::from_utf8_lossy(&out.stderr);
    log.lines()
        .find_map(|l| l.split_once("max_volume:").map(|(_, v)| v.trim().trim_end_matches("dB").trim().parse::<f64>().expect("max_volume")))
        .unwrap_or_else(|| panic!("no level for audio track {index}:\n{log}"))
}

fn number(v: &Value) -> f64 {
    v.as_str().and_then(|s| s.parse().ok()).or_else(|| v.as_f64()).unwrap_or_else(|| panic!("not a number: {v}"))
}

fn int(v: &Value) -> i64 {
    v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok())).unwrap_or_else(|| panic!("not an integer: {v}"))
}

// --- Keep awake ---

fn pmset() -> String {
    let out = Command::new("/usr/bin/pmset").args(["-g", "assertions"]).output().expect("pmset");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// The kinds of this process's assertions named `reason`, as pmset lists them:
/// `pid 123(name): [0x…] 00:00:03 PreventUserIdleDisplaySleep named: "reason"`.
fn assertions(reason: &str) -> Vec<String> {
    let ours = format!("pid {}(", std::process::id());
    // pmset does not write the names in UTF-8 (« réunion » comes out as one stray byte per
    // accented letter): compare them on their ASCII letters.
    let ascii = |s: &str| s.chars().map(|c| if c.is_ascii() { c } else { char::REPLACEMENT_CHARACTER }).collect::<String>();
    let named = format!("named: \"{}\"", ascii(reason));
    pmset()
        .lines()
        .filter(|l| l.contains(&ours) && l.contains(&named))
        .filter_map(|l| l.split_once("] ")?.1.split_whitespace().nth(1).map(str::to_string))
        .collect()
}

/// Released assertions leave pmset's list (give it a second).
fn assert_released(reason: &str) {
    let start = Instant::now();
    while !assertions(reason).is_empty() && start.elapsed() < Duration::from_secs(2) {
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(assertions(reason).is_empty(), "« {reason} » is still held after release:\n{}", pmset());
}

// --- Prerequisites ---

/// Says why the test checks nothing on this Mac (`cargo xtask smoke` lists these lines).
fn skip(why: &str) {
    eprintln!("SKIPPED: {why}");
}

/// A program from the PATH, or from Homebrew (an IDE may not have the terminal's PATH).
fn program(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect::<Vec<_>>()).unwrap_or_default();
    path.into_iter().chain(["/opt/homebrew/bin", "/usr/local/bin"].map(PathBuf::from)).map(|d| d.join(name)).find(|p| p.is_file())
}

/// A temporary folder, removed afterwards unless the test failed (its files are then kept to
/// look at, and the path is printed).
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let path = std::env::temp_dir().join(format!("greffier-smoke-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("temporary folder");
        TempDir(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!("kept for inspection: {}", self.0.display());
        } else {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}
