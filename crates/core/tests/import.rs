//! Importing a past meeting: a media file alone, with its transcription (Greffier file, WebVTT or
//! SRT), or with its transcription and its summary. The files are copied into a new meeting
//! folder; the originals do not move.

mod bench;

use std::path::{Path, PathBuf};

use bench::Bench;
use greffier_core::folder::{Import, has_picture, mp4_duration, mp4_leading_video_gap};
use greffier_core::{Command, DateTime, Stage, Event, MeetingId, Role, Status};

/// The fixtures' video (12 s).
fn video() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/meetings-folder/2026-11-19 15h30 - Réunion/2026-11-19 15h30 - Réunion - enregistrement.mp4")
}

/// An MP4 whose video starts after a gap, as ffmpeg writes it when the first frame comes after 0
/// (here at 33 ms): an empty edit leads the video track's edit list. The player can decode no
/// picture in that gap, and fails on it.
fn video_after_a_gap() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mp4/ffmpeg-leading-gap.mp4")
}

/// An MP4 with sound only (1 s of silence, AAC), as an `.m4a` turned into `.mp4` gives.
fn audio_only_mp4() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mp4/audio-only.mp4")
}

/// A PCM WAV file of `seconds` of silence: 16 kHz, mono, 16 bits.
fn wav(seconds: f64) -> Vec<u8> {
    let data = (seconds * 32_000.0) as u32;
    let mut b = Vec::new();
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes()); // PCM
    b.extend_from_slice(&1u16.to_le_bytes()); // mono
    b.extend_from_slice(&16_000u32.to_le_bytes());
    b.extend_from_slice(&32_000u32.to_le_bytes()); // byte rate
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data.to_le_bytes());
    b.resize(b.len() + data as usize, 0);
    b
}

/// The number of bytes that differ between two files of the same length.
fn changed_bytes(a: &Path, b: &Path) -> usize {
    let (a, b) = (std::fs::read(a).unwrap(), std::fs::read(b).unwrap());
    assert_eq!(a.len(), b.len(), "same length");
    a.iter().zip(&b).filter(|(x, y)| x != y).count()
}

/// Files outside the meetings folder, as on the desktop.
struct Outside(PathBuf);

impl Outside {
    fn new(bench: &Bench) -> Outside {
        let d = bench.folder.with_extension("outside");
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("originals folder");
        Outside(d)
    }
    fn write(&self, name: &str, content: &str) -> PathBuf {
        let p = self.0.join(name);
        std::fs::write(&p, content).expect("original");
        p
    }
    fn write_bytes(&self, name: &str, content: &[u8]) -> PathBuf {
        let p = self.0.join(name);
        std::fs::write(&p, content).expect("original");
        p
    }
    fn copy_to(&self, from: &Path, name: &str) -> PathBuf {
        let p = self.0.join(name);
        std::fs::copy(from, &p).expect("original");
        p
    }
}

impl Drop for Outside {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run_import(bench: &mut Bench, import: Import) -> MeetingId {
    let ev = bench.execute(Command::Import(import));
    ev.iter()
        .find_map(|e| match e {
            Event::MeetingImported(id) => Some(*id),
            _ => None,
        })
        .expect("the imported meeting")
}

fn import(recording: PathBuf) -> Import {
    Import { recording, transcription: None, summary: None, start: DateTime::new(2026, 10, 2, 9, 30), title: "Point client".into() }
}

#[test]
fn a_media_alone_becomes_a_meeting_to_process_from_transcription() {
    let mut bench = Bench::new();
    let outside = Outside::new(&bench);
    let original = outside.copy_to(&video(), "point client.mp4");
    let id = run_import(&mut bench, import(original.clone()));

    let r = bench.meeting(id);
    assert_eq!(r.folder_name, "2026-10-02 09h30 - Point client");
    assert_eq!(r.title, "Point client");
    assert_eq!(r.start, DateTime::new(2026, 10, 2, 9, 30));
    assert_eq!(r.duration, 12.0);
    assert_eq!(r.status, Status::ToProcess);
    assert_eq!(r.completed_stage, None);
    assert_eq!(r.files, [Role::Sheet, Role::Recording]);
    let copy = bench.greffier.folder().file(&r.folder_name, Role::Recording);
    assert_eq!(std::fs::read(copy).unwrap(), std::fs::read(video()).unwrap());
    assert!(original.is_file(), "the original stays where it was");

    // Read back identically at the next launch.
    let name = r.folder_name.clone();
    bench.reopen();
    let r = bench.named_meeting(&name);
    assert_eq!((r.status.clone(), r.completed_stage, r.duration, r.title.as_str()), (Status::ToProcess, None, 12.0, "Point client"));

    let id = r.id;
    bench.execute(Command::Process(id));
    assert_eq!(bench.advance_until(id, |s| *s == Status::Done), [Stage::Transcription, Stage::Review, Stage::Summary]);
}

#[test]
fn an_mp4_family_media_becomes_the_mp4_recording_without_touching_the_original() {
    for name in ["point.mov", "point.m4v", "point.m4a", "point.MOV"] {
        let mut bench = Bench::new();
        let outside = Outside::new(&bench);
        let original = outside.copy_to(&video(), name);
        let id = run_import(&mut bench, import(original.clone()));

        let r = bench.meeting(id);
        let folder = bench.greffier.folder();
        let mp4 = folder.file(&r.folder_name, Role::Recording);
        assert!(mp4.is_file(), "{name}: « … - enregistrement.mp4 »");
        assert_eq!(folder.recording(&r.folder_name), Some(mp4), "{name}: a single recording");
        assert_eq!(r.duration, 12.0);
        assert_eq!(std::fs::read(&original).unwrap(), std::fs::read(video()).unwrap(), "{name}: the original stays as is");
    }
}

#[test]
fn a_video_that_starts_after_a_gap_shows_its_first_frame_from_zero() {
    let mut bench = Bench::new();
    let outside = Outside::new(&bench);
    let original = outside.copy_to(&video_after_a_gap(), "point.mp4");
    assert!(mp4_leading_video_gap(&original).is_some_and(|g| g > 0.03), "the fixture starts with a gap");
    let id = run_import(&mut bench, import(original.clone()));

    let r = bench.meeting(id);
    let copy = bench.greffier.folder().file(&r.folder_name, Role::Recording);
    assert_eq!(mp4_leading_video_gap(&copy), None, "the copy has a picture from 0");
    // Only the gap's edit changes: the timeline after it, the media and the sound stay as they were.
    let changed = changed_bytes(&original, &copy);
    assert!((1..=8).contains(&changed), "{changed} bytes changed");
    assert_eq!(mp4_duration(&copy), mp4_duration(&original));
    assert_eq!(r.duration, mp4_duration(&original).unwrap());
    assert_eq!(std::fs::read(&original).unwrap(), std::fs::read(video_after_a_gap()).unwrap(), "the original stays as is");
    assert_eq!(bench.greffier.rescan_folder(), [], "Greffier's own copy is not a change made outside the app");
}

#[test]
fn a_video_imported_with_a_gap_before_the_fix_is_repaired_once_when_played() {
    let mut bench = Bench::on_fixtures();
    // A recording without a gap is played as is.
    let untouched = bench.named_meeting("2026-11-18 09h15 - Réunion").id;
    let file = bench.greffier.recording_to_play(untouched).expect("a recording");
    assert_eq!(std::fs::read(&file).unwrap(), std::fs::read(video()).unwrap());

    let name = "2026-11-19 15h30 - Réunion";
    let file = bench.greffier.folder().file(name, Role::Recording);
    std::fs::copy(video_after_a_gap(), &file).unwrap();
    bench.reopen();
    let id = bench.named_meeting(name).id;
    assert!(mp4_leading_video_gap(&file).is_some());

    assert_eq!(bench.greffier.recording_to_play(id), Some(file.clone()));
    assert_eq!(mp4_leading_video_gap(&file), None, "repaired in place");
    assert!((1..=8).contains(&changed_bytes(&video_after_a_gap(), &file)));
    assert_eq!(bench.greffier.rescan_folder(), [], "Greffier's own repair is not a change made outside the app");

    // Once repaired, nothing more is written.
    let modified = std::fs::metadata(&file).unwrap().modified().unwrap();
    assert_eq!(bench.greffier.recording_to_play(id), Some(file.clone()));
    assert_eq!(std::fs::metadata(&file).unwrap().modified().unwrap(), modified);
}

#[test]
fn a_video_that_starts_long_after_the_sound_is_left_as_is() {
    // Video starting 2 s after the sound: filling that gap would play the first 2 s of picture
    // twice, out of step with the sound. Only a gap of a few frames is filled.
    let long_gap = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mp4/ffmpeg-long-leading-gap.mp4");
    let mut bench = Bench::new();
    let outside = Outside::new(&bench);
    let original = outside.copy_to(&long_gap, "point.mp4");
    assert!(mp4_leading_video_gap(&original).is_some_and(|g| g > 1.9), "the fixture starts 2 s late");
    let id = run_import(&mut bench, import(original.clone()));

    let r = bench.meeting(id);
    let copy = bench.greffier.folder().file(&r.folder_name, Role::Recording);
    assert_eq!(std::fs::read(&copy).unwrap(), std::fs::read(&long_gap).unwrap(), "the copy is not rewritten");
    assert_eq!(bench.greffier.recording_to_play(id), Some(copy.clone()));
    assert_eq!(std::fs::read(&copy).unwrap(), std::fs::read(&long_gap).unwrap(), "nor when played");
}

#[test]
fn a_media_that_cannot_become_mp4_keeps_its_extension() {
    let mut bench = Bench::new();
    let outside = Outside::new(&bench);
    // A .mov that is not one: conversion to MP4 fails, it is copied as is.
    let id = run_import(&mut bench, import(outside.write("point.mov", "pas vraiment une vidéo")));
    let name = bench.meeting(id).folder_name.clone();
    let copy = bench.greffier.folder().recording(&name).expect("the imported media");
    assert_eq!(copy.file_name().and_then(|n| n.to_str()), Some("2026-10-02 09h30 - Point client - enregistrement.mov"));
    assert_eq!(std::fs::read_to_string(copy).unwrap(), "pas vraiment une vidéo");
    assert!(!bench.greffier.folder().file(&name, Role::Recording).exists(), "no half-written MP4");

    // Outside the MP4 family (WAV, MP3…), nothing to try.
    let mut bench = Bench::new();
    let outside = Outside::new(&bench);
    let id = run_import(&mut bench, import(outside.write("point.wav", "RIFF")));
    let name = bench.meeting(id).folder_name.clone();
    assert_eq!(bench.greffier.folder().recording(&name).and_then(|f| f.extension().map(|e| e.to_owned())), Some("wav".into()));
}

/// (displayed speaker, start, end, text) of each utterance.
fn utterances(bench: &Bench, id: MeetingId) -> Vec<(String, f64, f64, String)> {
    let r = bench.meeting(id);
    r.utterances.iter().map(|x| (r.speakers[r.displayed_speaker(x.speaker)].display_name(), x.start, x.end, x.text.clone())).collect()
}

fn utterance(who: &str, start: f64, end: f64, text: &str) -> (String, f64, f64, String) {
    (who.into(), start, end, text.into())
}

#[test]
fn a_media_and_its_webvtt_transcription_resume_at_review() {
    let mut bench = Bench::new();
    let outside = Outside::new(&bench);
    let vtt = outside.write(
        "point.vtt",
        "WEBVTT\n\nNOTE exporté de la visio\n\n1\n00:00:01.000 --> 00:00:03.500\n<v Alexandre>Bonjour Khalid,\non commence ?</v>\n\n2\n00:00:04.000 --> 00:00:05.250 align:start\n<v.chef Khalid>Oui, <i>vas-y</i>.\n\n00:06.000 --> 00:07.000\n<v Alexandre>Parfait.\n",
    );
    let id = run_import(&mut bench, Import { transcription: Some(vtt.clone()), ..import(outside.copy_to(&video(), "point.mp4")) });

    assert_eq!(
        utterances(&bench, id),
        [utterance("Alexandre", 1.0, 3.5, "Bonjour Khalid, on commence ?"), utterance("Khalid", 4.0, 5.25, "Oui, vas-y."), utterance("Alexandre", 6.0, 7.0, "Parfait.")]
    );
    let r = bench.meeting(id);
    let labels: Vec<&str> = r.speakers.iter().map(|l| l.label.as_str()).collect();
    assert_eq!(labels, ["SPEAKER_00", "SPEAKER_01"], "names are given to speakers, in the order they speak");
    assert_eq!(r.status, Status::ToProcess);
    assert_eq!(r.completed_stage, Some(Stage::Transcription));
    assert!(r.review.is_none(), "an imported transcription is not reviewed");
    assert_eq!(r.files, [Role::Sheet, Role::ReadableTranscription, Role::StructuredTranscription, Role::Recording]);
    assert!(vtt.is_file());

    // The sheet keeps the names, the structured transcription the utterances: everything comes back at the next launch.
    let before = r.clone();
    bench.reopen();
    let after = bench.named_meeting(&before.folder_name).clone();
    assert_eq!(greffier_core::Meeting { id: before.id, ..after }, before);

    let id = bench.named_meeting(&before.folder_name).id;
    bench.execute(Command::Process(id));
    assert_eq!(bench.advance_until(id, |s| *s == Status::Done), [Stage::Review, Stage::Summary]);
}

/// Imports the fixtures' media with this transcription; returns its utterances.
fn import_transcription(name: &str, content: &str) -> Vec<(String, f64, f64, String)> {
    let mut bench = Bench::new();
    let outside = Outside::new(&bench);
    let t = outside.write(name, content);
    let id = run_import(&mut bench, Import { transcription: Some(t), ..import(outside.copy_to(&video(), "point.mp4")) });
    utterances(&bench, id)
}

#[test]
fn srt_speaker_is_read_between_brackets() {
    let srt = "1\r\n00:00:00,051 --> 00:00:02,853\r\n[SPEAKER_02]: Oui, vas-y.\r\n\r\n2\r\n00:00:02,873 --> 00:00:08,057\r\n[SPEAKER_00]: On regarde le banc.\r\n\r\n3\r\n00:00:09,000 --> 00:00:10,000\r\n[Khalid] : D'accord.\r\n";
    assert_eq!(
        import_transcription("point.srt", srt),
        [utterance("SPEAKER_02", 0.051, 2.853, "Oui, vas-y."), utterance("SPEAKER_00", 2.873, 8.057, "On regarde le banc."), utterance("Khalid", 9.0, 10.0, "D'accord.")]
    );
}

#[test]
fn speaker_is_read_before_a_colon() {
    let vtt = "WEBVTT\n\n00:00:01.000 --> 00:00:02.000\nAlexandre : Bonjour.\n\n00:00:02.000 --> 00:00:03.000\nMarie-Claire Dupont: Salut, il est 10:30.\n\n00:00:03.000 --> 00:00:04.000\net on continue sans nom\n";
    assert_eq!(
        import_transcription("point.vtt", vtt),
        [utterance("Alexandre", 1.0, 2.0, "Bonjour."), utterance("Marie-Claire Dupont", 2.0, 3.0, "Salut, il est 10:30."), utterance("Marie-Claire Dupont", 3.0, 4.0, "et on continue sans nom")]
    );
}

#[test]
fn without_speakers_everything_goes_to_a_single_speaker() {
    let srt = "1\n00:00:01,000 --> 00:00:02,000\nBonjour à tous.\n\n2\n00:00:02,500 --> 00:00:04,000\nBref : on commence.\n\n3\n00:00:04,000 --> 00:00:05,000\nC'est parti.\n";
    assert_eq!(
        import_transcription("point.srt", srt),
        [utterance("SPEAKER_00", 1.0, 2.0, "Bonjour à tous."), utterance("SPEAKER_00", 2.5, 4.0, "Bref : on commence."), utterance("SPEAKER_00", 4.0, 5.0, "C'est parti.")]
    );
}

fn release_review(role: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fixtures/meetings-folder/2026-11-20 10h00 - Revue de release/2026-11-20 10h00 - Revue de release - {role}"))
}

#[test]
fn a_media_and_a_summary_without_transcription_restart_from_transcription() {
    let mut bench = Bench::new();
    let outside = Outside::new(&bench);
    let summary = outside.write("point.md", "Un bilan écrit ailleurs.\n");
    let id = run_import(&mut bench, Import { summary: Some(summary), ..import(outside.copy_to(&video(), "point.mp4")) });
    assert_eq!((bench.meeting(id).status.clone(), bench.meeting(id).completed_stage), (Status::ToProcess, None));

    bench.execute(Command::Process(id));
    assert_eq!(bench.advance_until(id, |s| *s == Status::Done), [Stage::Transcription, Stage::Review, Stage::Summary]);
}

#[test]
fn a_media_its_transcription_and_its_summary_go_through_no_stage() {
    let mut bench = Bench::new();
    let outside = Outside::new(&bench);
    let summary = "Un bilan écrit ailleurs, sans les sections de Greffier.\n";
    let id = run_import(
        &mut bench,
        Import {
            transcription: Some(outside.copy_to(&release_review("transcription.json"), "revue.json")),
            summary: Some(outside.write("revue.md", summary)),
            title: "Revue : release".into(),
            ..import(outside.copy_to(&video(), "revue.mp4"))
        },
    );

    let r = bench.meeting(id);
    assert_eq!(r.folder_name, "2026-10-02 09h30 - Revue - release");
    assert_eq!(r.title, "Revue : release");
    assert_eq!(r.status, Status::Done);
    assert_eq!(r.completed_stage, Some(Stage::Summary));
    assert_eq!(r.files, Role::ALL);
    assert_eq!(r.summary, summary);
    assert!(!r.is_new);
    assert_eq!(bench.read(&bench.greffier.folder().file(&r.folder_name, Role::Summary)), summary, "the summary is copied as is");
    // The Greffier file is taken over, but considered not reviewed.
    assert_eq!(r.utterances.len(), 3);
    assert_eq!(r.utterances[1].text, "Oui, si la recette passe vendredi.");
    assert!(r.review.is_none() && r.raw.is_empty());
    let json = bench.read(&bench.greffier.folder().file(&r.folder_name, Role::StructuredTranscription));
    assert!(!json.contains("\"review\""));

    assert!(bench.greffier.execute(Command::Process(id)).is_err(), "nothing left to do");
    let before = bench.meeting(id).clone();
    bench.reopen();
    let after = bench.named_meeting(&before.folder_name).clone();
    assert_eq!(greffier_core::Meeting { id: before.id, ..after }, before);
}

/// A real VTT (an old transcription, kept outside the repository) imports like any VTT.
/// Optional: `GREFFIER_REAL_VTT=<file.vtt> cargo test`; without it, the test checks nothing.
/// Only checks what holds for any VTT with `[SPEAKER_xx]:` labels.
#[test]
fn a_real_vtt_imports_like_any_vtt() {
    let Some(vtt) = std::env::var_os("GREFFIER_REAL_VTT").map(PathBuf::from) else { return };
    let content = std::fs::read_to_string(&vtt).expect("GREFFIER_REAL_VTT readable");
    let entries = content.lines().filter(|l| l.contains("-->")).count();
    let mut bench = Bench::new();
    let outside = Outside::new(&bench);
    let id = run_import(&mut bench, Import { transcription: Some(vtt.clone()), ..import(outside.copy_to(&video(), "real.mp4")) });

    let r = bench.meeting(id);
    assert_eq!(r.utterances.len(), entries, "one utterance per VTT entry");
    assert!(r.speakers.iter().all(|l| l.label.starts_with("SPEAKER_") && l.name.is_none()), "labels, not names");
    assert!(r.utterances.iter().all(|x| !x.text.starts_with("[SPEAKER_") && x.start <= x.end), "the « [SPEAKER_xx]: » is removed from the text");
    assert!(r.utterances.windows(2).all(|w| w[0].start <= w[1].start));
    assert_eq!(r.completed_stage, Some(Stage::Transcription));
}

/// A minimal MP4 whose `mvhd` box gives its creation date (seconds since 1904, UTC).
fn mp4_created_at(seconds_since_1904: u32) -> Vec<u8> {
    let boxed = |name: &[u8; 4], content: &[u8]| [&(8 + content.len() as u32).to_be_bytes()[..], name, content].concat();
    let mut mvhd = vec![0u8; 100];
    mvhd[4..8].copy_from_slice(&seconds_since_1904.to_be_bytes());
    mvhd[12..16].copy_from_slice(&1000u32.to_be_bytes());
    mvhd[16..20].copy_from_slice(&90_000u32.to_be_bytes());
    [boxed(b"ftyp", b"isom\0\0\x02\0isom"), boxed(b"moov", &boxed(b"mvhd", &mvhd))].concat()
}

#[test]
fn dropped_files_give_the_media_transcription_summary_date_and_title() {
    let bench = Bench::new();
    let outside = Outside::new(&bench);
    let media = outside.0.join("visio.mp4");
    // 2026-10-02 07:30:00 UTC = 3,873,771,000 s since 1904.
    std::fs::write(&media, mp4_created_at(3_873_771_000)).unwrap();
    let vtt = outside.write("visio.vtt", "WEBVTT\n\n00:01.000 --> 00:02.000\nBonjour.\n");
    let summary = outside.write("notes.md", "---\ntype: bilan\n---\n\n# Bilan — Point client Axe\n\n## En bref\n\nRien.\n");

    let import = bench.greffier.propose_import(&[vtt.clone(), summary.clone(), media.clone()]).unwrap();
    assert_eq!(import, Import { recording: media, transcription: Some(vtt), summary: Some(summary), start: DateTime::new(2026, 10, 2, 7, 30), title: "Point client Axe".into() });
    assert_eq!(bench.greffier.folder().names(), Vec::<String>::new(), "proposing writes nothing");
}

#[test]
fn without_date_in_media_the_date_is_the_modification_date_and_the_title_the_file_name() {
    let mut a = bench::adapters();
    let requested_audio = std::rc::Rc::new(std::cell::RefCell::new(None));
    let note = requested_audio.clone();
    a.transcriber = Box::new(greffier_core::fake::FakeTranscriber::new(bench::STAGE_DURATION, move |d| {
        *note.borrow_mut() = Some(d.audio.clone());
        Ok(bench::words())
    }));
    let mut bench = Bench::with(a);
    let outside = Outside::new(&bench);
    let audio = outside.write("Daily du lundi.m4a", "pas vraiment de l'audio");
    // 2026-11-16 08:45:00 UTC.
    let monday = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_794_818_700);
    std::fs::File::options().write(true).open(&audio).unwrap().set_modified(monday).unwrap();

    let proposed = bench.greffier.propose_import(std::slice::from_ref(&audio)).unwrap();
    assert_eq!((proposed.start, proposed.title.as_str(), proposed.transcription.clone(), proposed.summary.clone()), (DateTime::new(2026, 11, 16, 8, 45), "Daily du lundi", None, None));

    // The date and title can be changed before confirming; an audio file that does not convert to
    // MP4 (it is not one) keeps its extension.
    let id = run_import(&mut bench, Import { start: DateTime::new(2026, 11, 16, 9, 0), title: "Daily".into(), ..proposed });
    let name = bench.meeting(id).folder_name.clone();
    assert_eq!(name, "2026-11-16 09h00 - Daily");
    let copy = bench.greffier.folder().folder(&name).join("2026-11-16 09h00 - Daily - enregistrement.m4a");
    assert!(copy.is_file());
    assert_eq!(bench.meeting(id).files, [Role::Sheet, Role::Recording]);
    bench.execute(Command::Process(id));
    bench.advance_until(id, |s| *s == Status::Done);
    assert_eq!(requested_audio.borrow().as_ref(), Some(&copy), "processing reads the imported audio");
}

#[test]
fn a_title_from_a_file_name_drops_the_meetings_folder_date_and_role() {
    let bench = Bench::new();
    let outside = Outside::new(&bench);
    let title = |name: &str| bench.greffier.propose_import(&[outside.write(name, "")]).unwrap().title;
    assert_eq!(title("2026-11-04 14h33 - Point client - enregistrement.mp4"), "Point client");
    assert_eq!(title("2026-11-04 14h33 - enregistrement.mp4"), "Réunion");
    assert_eq!(title("Point client - transcription.m4a"), "Point client");
    assert_eq!(title("2026-11-04 14h33 - Revue - sprint 12.mov"), "Revue - sprint 12");
    assert_eq!(title("Démo du jeudi.mp4"), "Démo du jeudi");
}

#[test]
fn an_import_without_media_or_with_two_media_is_rejected() {
    let bench = Bench::new();
    let outside = Outside::new(&bench);
    let vtt = outside.write("a.vtt", "WEBVTT\n");
    let pdf = outside.write("a.pdf", "");
    let mp4 = outside.copy_to(&video(), "a.mp4");
    let mov = outside.copy_to(&video(), "b.mov");
    for files in [vec![vtt.clone()], vec![mp4.clone(), mov], vec![mp4, pdf], vec![]] {
        assert!(bench.greffier.propose_import(&files).is_err(), "{files:?} accepted");
    }
}

#[test]
fn a_failed_import_leaves_nothing_in_the_meetings_folder() {
    let mut bench = Bench::new();
    let outside = Outside::new(&bench);
    let empty = outside.write("vide.vtt", "WEBVTT\n\nNOTE rien ici\n");
    let rejection = bench.greffier.execute(Command::Import(Import { transcription: Some(empty), ..import(outside.copy_to(&video(), "a.mp4")) }));
    assert!(rejection.is_err());
    assert!(bench.greffier.meetings().is_empty());
    assert_eq!(std::fs::read_dir(&bench.folder).unwrap().count(), 0);
}

#[test]
fn an_imported_meeting_is_not_seen_as_modified_outside_the_app_and_its_audio_is_watched() {
    let mut bench = Bench::new();
    let outside = Outside::new(&bench);
    let audio = outside.write("Point.m4a", "pas vraiment de l'audio");
    let id = run_import(&mut bench, import(audio));

    // The files copied and written by Greffier do not count as modifications.
    assert_eq!(bench.greffier.rescan_folder(), []);

    // The imported recording that does not convert to MP4 keeps its extension, and its
    // modification is still seen.
    let name = bench.meeting(id).folder_name.clone();
    let copy = bench.greffier.folder().recording(&name).expect("the imported audio");
    assert_eq!(copy.extension().and_then(|e| e.to_str()), Some("m4a"));
    std::fs::write(&copy, "un autre audio, plus long que le premier").unwrap();
    assert_eq!(bench.greffier.rescan_folder(), [Event::MeetingReviewed(id)]);
}

#[test]
fn an_audio_file_kept_as_is_has_its_duration_and_the_sheet_links_to_it() {
    let mut bench = Bench::new();
    let outside = Outside::new(&bench);
    let id = run_import(&mut bench, import(outside.write_bytes("point.wav", &wav(2.5))));

    // Not an MP4: its duration comes from the adapter (AVFoundation in the app), not 0.
    let name = bench.meeting(id).folder_name.clone();
    assert_eq!(bench.meeting(id).duration, 2.5);
    // The sheet's link leads to the file that exists, `.wav`, not to a missing `.mp4`.
    let sheet = std::fs::read_to_string(bench.greffier.folder().file(&name, Role::Sheet)).unwrap();
    assert!(sheet.contains(&format!("- [Enregistrement](<{name} - enregistrement.wav>)")), "{sheet}");
    assert!(!sheet.contains("enregistrement.mp4"), "{sheet}");

    bench.reopen();
    assert_eq!(bench.named_meeting(&name).duration, 2.5, "read back from the sheet");
}

#[test]
fn only_a_recording_with_a_video_track_has_a_picture() {
    let bench = Bench::new();
    let outside = Outside::new(&bench);
    assert!(has_picture(&video()));
    assert!(!has_picture(&audio_only_mp4()), "an MP4 with sound only plays as audio");
    assert!(!has_picture(&outside.write_bytes("point.wav", &wav(1.0))));
    // A file whose boxes cannot be read is judged by its extension.
    assert!(has_picture(&outside.write("point.mov", "pas vraiment une vidéo")));
    assert!(!has_picture(&outside.write("point.mp3", "pas vraiment de l'audio")));
}
