//! The fake adapters: they advance with the time they are given and return hand-written
//! results. They serve the core's tests and the prototype (with the fake data of
//! [`crate::demo`]).

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use crate::adapters::*;
use crate::meeting::{Stage, Word, SpeakerTurn};
use crate::resources::{ClaudeState, Tools, sha256};
use crate::time::DateTime;

/// A task that lasts `duration` seconds of elapsed time, then returns its result.
pub struct FakeTask<T> {
    duration: f64,
    progress: f64,
    result: Option<Result<T, String>>,
}

impl<T> FakeTask<T> {
    pub fn new(duration: f64, result: Result<T, String>) -> FakeTask<T> {
        FakeTask { duration, progress: 0.0, result: Some(result) }
    }

    pub fn boxed(duration: f64, result: Result<T, String>) -> BoxedTask<T>
    where
        T: 'static,
    {
        Box::new(FakeTask::new(duration, result))
    }
}

impl<T> Task<T> for FakeTask<T> {
    fn poll(&mut self, elapsed: f64) -> TaskProgress<T> {
        if self.duration > 0.0 {
            self.progress += elapsed / self.duration;
        }
        if self.duration > 0.0 && self.progress < 1.0 {
            return TaskProgress::Running(self.progress);
        }
        match self.result.take() {
            Some(Ok(r)) => TaskProgress::Done(r),
            Some(Err(e)) => TaskProgress::Failed(e),
            None => TaskProgress::Failed("tâche déjà rendue".into()),
        }
    }
}

/// A capture whose time runs `speed` times faster than the elapsed time. It writes as it goes a
/// small fragmented MP4 with no picture or sound (one track, one fragment per poll), whose
/// duration reads back like that of a real recording, even cut off. Its final duration is at
/// least `min_duration`.
pub struct FakeCapture {
    speed: f64,
    min_duration: f64,
    duration: f64,
    file: Option<PathBuf>,
    /// Milliseconds already written in the fragments, and number of the next fragment.
    written: u64,
    fragment: u32,
    permissions: Permissions,
    /// The capture stops on its own at this instant, with this message.
    breakdown: Option<(f64, String)>,
    /// `stop` fails with this message (the file keeps the fragments already written).
    failure_on_stop: Option<String>,
}

impl FakeCapture {
    pub fn new(speed: f64, min_duration: f64) -> FakeCapture {
        FakeCapture { speed, min_duration, duration: 0.0, file: None, written: 0, fragment: 1, permissions: Permissions::GRANTED, breakdown: None, failure_on_stop: None }
    }

    /// The capture stops on its own after `duration` recorded seconds, as when macOS cuts the
    /// stream: it no longer advances and reports `message`.
    pub fn broken_at(mut self, duration: f64, message: &str) -> FakeCapture {
        self.breakdown = Some((duration, message.into()));
        self
    }

    /// Stopping fails with `message`, like a file that cannot be finished.
    pub fn failing_on_stop(mut self, message: &str) -> FakeCapture {
        self.failure_on_stop = Some(message.into());
        self
    }

    /// The initial permissions. A request is always accepted ("Autoriser"); a denied permission
    /// stays denied, as on macOS.
    pub fn with_permissions(mut self, permissions: Permissions) -> FakeCapture {
        self.permissions = permissions;
        self
    }

    /// Adds a fragment up to the current duration.
    fn write_so_far(&mut self) -> Result<(), String> {
        let Some(file) = &self.file else { return Ok(()) };
        let ms = (self.duration * 1000.0).round() as u64;
        if ms <= self.written {
            return Ok(());
        }
        let bytes = mp4::fragment(self.fragment, self.written, (ms - self.written) as u32);
        use std::io::Write;
        std::fs::OpenOptions::new().append(true).open(file).and_then(|mut f| f.write_all(&bytes)).map_err(|e| e.to_string())?;
        self.written = ms;
        self.fragment += 1;
        Ok(())
    }
}

impl Capture for FakeCapture {
    fn permissions(&self) -> Permissions {
        self.permissions
    }

    fn request_permission(&mut self, permission: Permission) {
        let state = self.permissions.state_mut(permission);
        if *state == PermissionState::ToRequest {
            *state = PermissionState::Granted;
        }
    }

    fn start(&mut self, request: CaptureRequest) -> Result<(), String> {
        if let Some(parent) = request.file.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(&request.file, mp4::start()).map_err(|e| e.to_string())?;
        self.duration = 0.0;
        self.written = 0;
        self.fragment = 1;
        self.file = Some(request.file);
        Ok(())
    }

    fn poll(&mut self, elapsed: f64) -> CaptureState {
        let mut error = None;
        if self.file.is_some() {
            self.duration += elapsed * self.speed;
            if let Some((end, message)) = &self.breakdown
                && self.duration >= *end
            {
                self.duration = *end;
                error = Some(message.clone());
            }
            let _ = self.write_so_far();
        }
        CaptureState { duration: self.duration, levels: Levels { micro: 0.5, mac_audio: 0.5 }, error }
    }

    fn stop(&mut self) -> Result<f64, String> {
        if let Some(message) = &self.failure_on_stop {
            self.file = None;
            return Err(message.clone());
        }
        self.duration = self.duration.max(self.min_duration);
        self.write_so_far()?;
        self.file.take().ok_or("aucun enregistrement en cours")?;
        Ok(self.duration)
    }
}

/// The boxes of the fake capture's fragmented MP4: one track at a thousandth of a second.
mod mp4 {
    fn boxed(name: &[u8; 4], content: &[u8]) -> Vec<u8> {
        let mut b = ((content.len() + 8) as u32).to_be_bytes().to_vec();
        b.extend_from_slice(name);
        b.extend_from_slice(content);
        b
    }

    fn integers(v: &[u32]) -> Vec<u8> {
        v.iter().flat_map(|x| x.to_be_bytes()).collect()
    }

    /// `ftyp` and `moov`: unknown duration, one track (ID 1, timescale 1000).
    pub fn start() -> Vec<u8> {
        let matrix = [0x0001_0000, 0, 0, 0, 0x0001_0000, 0, 0, 0, 0x4000_0000];
        let mut mvhd = integers(&[0, 0, 0, 1000, 0, 0x0001_0000, 0x0100_0000, 0, 0]);
        mvhd.extend(integers(&matrix));
        mvhd.extend(integers(&[0, 0, 0, 0, 0, 0, 2]));
        let mut tkhd = integers(&[3, 0, 0, 1, 0, 0, 0, 0, 0, 0]);
        tkhd.extend(integers(&matrix));
        tkhd.extend(integers(&[0, 0]));
        let mdhd = integers(&[0, 0, 0, 1000, 0, 0x55c4_0000]);
        let hdlr = [integers(&[0, 0]), b"soun".to_vec(), integers(&[0, 0, 0]), b"Greffier\0".to_vec()].concat();
        let mdia = [boxed(b"mdhd", &mdhd), boxed(b"hdlr", &hdlr)].concat();
        let trak = [boxed(b"tkhd", &tkhd), boxed(b"mdia", &mdia)].concat();
        let mvex = boxed(b"trex", &integers(&[0, 1, 1, 0, 0, 0]));
        let moov = [boxed(b"mvhd", &mvhd), boxed(b"trak", &trak), boxed(b"mvex", &mvex)].concat();
        [boxed(b"ftyp", &[b"iso5".to_vec(), integers(&[0]), b"iso5iso6mp41".to_vec()].concat()), boxed(b"moov", &moov)].concat()
    }

    /// A fragment of one sample of `duration` ms starting at `start` ms, and its empty `mdat`.
    pub fn fragment(number: u32, start: u64, duration: u32) -> Vec<u8> {
        let tfhd = integers(&[0x0002_0000, 1]);
        let tfdt = [integers(&[0x0100_0000]), start.to_be_bytes().to_vec()].concat();
        let trun = integers(&[0x0000_0100, 1, duration]);
        let traf = [boxed(b"tfhd", &tfhd), boxed(b"tfdt", &tfdt), boxed(b"trun", &trun)].concat();
        let moof = [boxed(b"mfhd", &integers(&[0, number])), boxed(b"traf", &traf)].concat();
        [boxed(b"moof", &moof), boxed(b"mdat", &[])].concat()
    }
}

type Producer<D, T> = Box<dyn FnMut(&D) -> Result<T, String>>;

/// Returns, after `duration` seconds, the words its function produces.
pub struct FakeTranscriber {
    duration: f64,
    produce: Producer<TranscriptionRequest, Vec<Word>>,
}

impl FakeTranscriber {
    pub fn new(duration: f64, produce: impl FnMut(&TranscriptionRequest) -> Result<Vec<Word>, String> + 'static) -> FakeTranscriber {
        FakeTranscriber { duration, produce: Box::new(produce) }
    }
    pub fn fixed(duration: f64, words: Vec<Word>) -> FakeTranscriber {
        FakeTranscriber::new(duration, move |_| Ok(words.clone()))
    }
}

impl Transcriber for FakeTranscriber {
    fn transcribe(&mut self, request: TranscriptionRequest) -> BoxedTask<Vec<Word>> {
        FakeTask::boxed(self.duration, (self.produce)(&request))
    }
}

/// Returns, after `duration` seconds, the speaker turns its function produces.
pub struct FakeDiarizer {
    duration: f64,
    produce: Producer<DiarizationRequest, Vec<SpeakerTurn>>,
}

impl FakeDiarizer {
    pub fn new(duration: f64, produce: impl FnMut(&DiarizationRequest) -> Result<Vec<SpeakerTurn>, String> + 'static) -> FakeDiarizer {
        FakeDiarizer { duration, produce: Box::new(produce) }
    }
    pub fn fixed(duration: f64, turns: Vec<SpeakerTurn>) -> FakeDiarizer {
        FakeDiarizer::new(duration, move |_| Ok(turns.clone()))
    }
}

impl Diarizer for FakeDiarizer {
    fn diarize(&mut self, request: DiarizationRequest) -> BoxedTask<Vec<SpeakerTurn>> {
        FakeTask::boxed(self.duration, (self.produce)(&request))
    }
}

/// An agent whose review and summary are given by two functions. The summary (Markdown) is
/// written to the requested file right at the call, as the agent would do.
pub struct FakeAgent {
    review_duration: f64,
    review: Producer<ReviewRequest, ReviewResult>,
    summary_duration: f64,
    summary: Producer<SummaryRequest, (String, SummaryResult)>,
}

impl FakeAgent {
    pub fn new(
        review_duration: f64,
        review: impl FnMut(&ReviewRequest) -> Result<ReviewResult, String> + 'static,
        summary_duration: f64,
        summary: impl FnMut(&SummaryRequest) -> Result<(String, SummaryResult), String> + 'static,
    ) -> FakeAgent {
        FakeAgent { review_duration, review: Box::new(review), summary_duration, summary: Box::new(summary) }
    }
}

impl Agent for FakeAgent {
    fn review(&mut self, request: ReviewRequest) -> BoxedTask<ReviewResult> {
        FakeTask::boxed(self.review_duration, (self.review)(&request))
    }

    fn summarize(&mut self, request: SummaryRequest) -> BoxedTask<SummaryResult> {
        let result = (self.summary)(&request).and_then(|(md, result)| {
            std::fs::write(&request.summary_file, md).map_err(|e| format!("{} : {e}", request.summary_file.display()))?;
            Ok(result)
        });
        FakeTask::boxed(self.summary_duration, result)
    }
}

/// Resources that are ready, or missing until an installation of `duration` seconds succeeds,
/// or frozen in the given states (the UI scenes).
pub struct FakeResources {
    ready: Rc<Cell<bool>>,
    install_duration: f64,
    failure: Option<String>,
    fixed: Option<Vec<Resource>>,
}

impl FakeResources {
    pub fn ready() -> FakeResources {
        FakeResources { ready: Rc::new(Cell::new(true)), install_duration: 0.0, failure: None, fixed: None }
    }
    pub fn missing(install_duration: f64) -> FakeResources {
        FakeResources { ready: Rc::new(Cell::new(false)), install_duration, failure: None, fixed: None }
    }
    /// Always these resources; installing and checking change nothing.
    pub fn fixed(resources: Vec<Resource>) -> FakeResources {
        FakeResources { fixed: Some(resources), ..FakeResources::ready() }
    }
    /// The installation fails with this message.
    pub fn failing(mut self, message: &str) -> FakeResources {
        self.failure = Some(message.into());
        self
    }
}

const RESOURCE_NAMES: [&str; 4] = ["Whisper large-v3", "Silero VAD v5.1.2", "pyannote community-1", "Python privé"];

struct Installation {
    task: FakeTask<()>,
    ready: Rc<Cell<bool>>,
}

impl Task<()> for Installation {
    fn poll(&mut self, elapsed: f64) -> TaskProgress<()> {
        let a = self.task.poll(elapsed);
        if a == TaskProgress::Done(()) {
            self.ready.set(true);
        }
        a
    }
}

impl Resources for FakeResources {
    fn state(&mut self) -> Vec<Resource> {
        if let Some(fixed) = &self.fixed {
            return fixed.clone();
        }
        let state = if self.ready.get() { ResourceState::Ready } else { ResourceState::Missing };
        let resource = |name: &str, stages: Vec<Stage>, state: ResourceState| Resource { name: name.into(), detail: String::new(), state, size: 0, total: 0, help: None, stages };
        RESOURCE_NAMES
            .iter()
            .map(|n| resource(n, vec![Stage::Transcription], state.clone()))
            .chain([resource("Claude Code", vec![Stage::Review, Stage::Summary], ResourceState::Ready)])
            .collect()
    }

    fn install(&mut self) -> BoxedTask<()> {
        let result = match &self.failure {
            Some(e) => Err(e.clone()),
            None => Ok(()),
        };
        Box::new(Installation { task: FakeTask::new(self.install_duration, result), ready: self.ready.clone() })
    }

    fn check(&mut self) -> BoxedTask<()> {
        FakeTask::boxed(0.0, Ok(()))
    }
}

/// The resource tools, without network or uv: downloads served from memory at
/// `bytes_per_second` (written little by little into the partial file, like curl), real
/// fingerprints, a Python and a `claude` CLI with the given result.
pub struct FakeTools {
    bytes_per_second: f64,
    contents: HashMap<String, Vec<u8>>,
    cuts: HashMap<String, u64>,
    corrupted: HashSet<String>,
    fingerprint_duration: f64,
    python: (f64, Result<(), String>),
    claude: ClaudeState,
}

impl FakeTools {
    pub fn new(bytes_per_second: f64) -> FakeTools {
        FakeTools {
            bytes_per_second,
            contents: HashMap::new(),
            cuts: HashMap::new(),
            corrupted: HashSet::new(),
            fingerprint_duration: 0.0,
            python: (0.0, Ok(())),
            claude: ClaudeState::Connected { version: "2.1.0".into(), account: "claude.ai".into() },
        }
    }
    pub fn serve(mut self, url: &str, content: &[u8]) -> FakeTools {
        self.contents.insert(url.into(), content.to_vec());
        self
    }
    /// The next download of `url` is interrupted once the partial file reaches `bytes`.
    pub fn cut_once(mut self, url: &str, bytes: u64) -> FakeTools {
        self.cuts.insert(url.into(), bytes);
        self
    }
    /// The next download of `url` receives wrong bytes.
    pub fn corrupt_once(mut self, url: &str) -> FakeTools {
        self.corrupted.insert(url.into());
        self
    }
    pub fn fingerprint_duration(mut self, seconds: f64) -> FakeTools {
        self.fingerprint_duration = seconds;
        self
    }
    pub fn python(mut self, duration: f64, result: Result<(), String>) -> FakeTools {
        self.python = (duration, result);
        self
    }
    pub fn claude(mut self, state: ClaudeState) -> FakeTools {
        self.claude = state;
        self
    }
}

struct FakeDownload {
    content: Vec<u8>,
    partial: PathBuf,
    bytes_per_second: f64,
    position: f64,
    written: u64,
    cutoff: Option<u64>,
    corrupted: bool,
}

impl Task<()> for FakeDownload {
    fn poll(&mut self, elapsed: f64) -> TaskProgress<()> {
        let total = self.content.len() as u64;
        self.position += elapsed * self.bytes_per_second;
        let target = (self.position as u64).min(total).min(self.cutoff.unwrap_or(u64::MAX)).max(self.written);
        let mut bytes = self.content[self.written as usize..target as usize].to_vec();
        if self.corrupted {
            bytes.iter_mut().for_each(|o| *o = !*o);
        }
        let written = std::fs::OpenOptions::new().create(true).append(true).open(&self.partial).and_then(|mut f| std::io::Write::write_all(&mut f, &bytes));
        if let Err(e) = written {
            return TaskProgress::Failed(format!("{} : {e}", self.partial.display()));
        }
        self.written = target;
        if self.cutoff.is_some_and(|c| self.written >= c) {
            return TaskProgress::Failed("connexion perdue".into());
        }
        if self.written >= total {
            return TaskProgress::Done(());
        }
        TaskProgress::Running(self.written as f64 / total.max(1) as f64)
    }
}

impl Tools for FakeTools {
    fn download(&mut self, url: &str, partial: &Path, _size: u64) -> BoxedTask<()> {
        let Some(content) = self.contents.get(url).cloned() else { return FakeTask::boxed(0.0, Err(format!("{url} : 404"))) };
        let already = std::fs::metadata(partial).map_or(0, |m| m.len()).min(content.len() as u64);
        Box::new(FakeDownload {
            content,
            partial: partial.to_path_buf(),
            bytes_per_second: self.bytes_per_second,
            position: already as f64,
            written: already,
            cutoff: self.cuts.remove(url),
            corrupted: self.corrupted.remove(url),
        })
    }

    fn fingerprint(&mut self, file: &Path) -> BoxedTask<String> {
        let result = std::fs::read(file).map(|o| sha256(&o)).map_err(|e| format!("{} : {e}", file.display()));
        FakeTask::boxed(self.fingerprint_duration, result)
    }

    fn install_python(&mut self, _project: &Path, _folder: &Path) -> BoxedTask<()> {
        FakeTask::boxed(self.python.0, self.python.1.clone())
    }

    fn check_claude(&mut self) -> BoxedTask<ClaudeState> {
        FakeTask::boxed(0.0, Ok(self.claude.clone()))
    }
}

/// A stopped clock.
pub struct FakeClock(pub DateTime);

impl Clock for FakeClock {
    fn now(&self) -> DateTime {
        self.0
    }
}

/// A trash in a temporary folder (deleted with the last copy): trashed folders are moved there
/// and remembered. Copies share the same trash, to look inside it after plugging it in.
#[derive(Clone)]
pub struct FakeTrash(std::sync::Arc<TrashBin>);

struct TrashBin {
    folder: PathBuf,
    breakdown: Option<String>,
    trashed: std::sync::Mutex<Vec<PathBuf>>,
}

impl Drop for TrashBin {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.folder);
    }
}

impl FakeTrash {
    pub fn new() -> FakeTrash {
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let folder = std::env::temp_dir().join(format!("greffier-trash-{}-{n}", std::process::id()));
        FakeTrash(std::sync::Arc::new(TrashBin { folder, breakdown: None, trashed: Default::default() }))
    }

    /// A trash that refuses everything, with this message.
    pub fn broken(self, message: &str) -> FakeTrash {
        let folder = self.0.folder.clone();
        FakeTrash(std::sync::Arc::new(TrashBin { folder, breakdown: Some(message.into()), trashed: Default::default() }))
    }

    /// Where the trashed folders are (under their name).
    pub fn folder(&self) -> &Path {
        &self.0.folder
    }

    /// The trashed paths, in order.
    pub fn trashed(&self) -> Vec<PathBuf> {
        self.0.trashed.lock().expect("trash").clone()
    }
}

impl Default for FakeTrash {
    fn default() -> FakeTrash {
        FakeTrash::new()
    }
}

impl Trash for FakeTrash {
    fn trash(&self, path: &Path) -> Result<(), String> {
        if let Some(m) = &self.0.breakdown {
            return Err(m.clone());
        }
        let name = path.file_name().ok_or("rien à jeter")?;
        std::fs::create_dir_all(&self.0.folder).map_err(|e| e.to_string())?;
        std::fs::rename(path, self.0.folder.join(name)).map_err(|e| format!("{} : {e}", path.display()))?;
        self.0.trashed.lock().expect("trash").push(path.to_path_buf());
        Ok(())
    }
}

/// A fake MP4 muxing: a file that starts like an MP4 (`ftyp` box) is copied as is, anything
/// else is rejected.
#[derive(Clone, Copy, Debug, Default)]
pub struct FakeMp4Muxer;

impl Mp4Muxer for FakeMp4Muxer {
    fn to_mp4(&self, from: &Path, to: &Path) -> Result<(), String> {
        let bytes = std::fs::read(from).map_err(|e| format!("{} : {e}", from.display()))?;
        if bytes.get(4..8) != Some(b"ftyp".as_slice()) {
            return Err(format!("{} : pas un conteneur MP4", from.display()));
        }
        std::fs::write(to, bytes).map_err(|e| format!("{} : {e}", to.display()))
    }
}
