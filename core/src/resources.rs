//! Greffier's resources, all in the app's folder (`~/Library/Application Support/Greffier/`) and
//! nowhere else: the models, downloaded and verified by sha256, pyannote's private Python
//! environment, installed by uv, and the check of the `claude` CLI, the only dependency the app
//! does not install.
//!
//! [`AppResources`] is the app's [`Resources`] adapter: it decides what to download, resumes
//! interrupted downloads, verifies the sha256s and puts the files in place. The slow work
//! (downloading, computing a fingerprint, running uv or `claude`) goes through [`Tools`]: the
//! Mac's (`greffier_tools::mac::MacTools`) or the tests' fakes ([`crate::fake::FakeTools`]).
//!
//! On disk, a file being downloaded is called `<name>.partial`; it only takes its name once its
//! sha256 is verified. A file present at the right size is therefore ready.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use sha2::{Digest, Sha256};

use crate::adapters::{TaskProgress, ResourceState, Resource, Resources, BoxedTask};
use crate::meeting::Stage;

pub use crate::settings::app_folder;

/// The Whisper model, in the app's folder.
pub const WHISPER_MODEL: &str = "models/ggml-large-v3.bin";
/// The Silero VAD model, in the app's folder.
pub const VAD_MODEL: &str = "models/ggml-silero-v5.1.2.bin";
/// The pyannote model's folder, in the app's folder.
pub const PYANNOTE_MODEL: &str = "models/pyannote-community-1";

/// A file to download.
#[derive(Clone, Debug, PartialEq)]
pub struct ResourceFile {
    /// Relative to the app's folder.
    pub path: String,
    pub url: String,
    pub size: u64,
    /// In lowercase hexadecimal.
    pub sha256: String,
}

impl ResourceFile {
    pub fn new(path: &str, url: &str, size: u64, sha256: &str) -> ResourceFile {
        ResourceFile { path: path.into(), url: url.into(), size, sha256: sha256.into() }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Content {
    Files(Vec<ResourceFile>),
    /// The private Python environment, installed by uv from its project and its lock file.
    Python { pyproject: String, lock: String },
    /// The `claude` CLI, which is only checked.
    Claude,
}

/// A row of the catalogue, that is, of the first-launch screen.
#[derive(Clone, Debug, PartialEq)]
pub struct Element {
    pub name: String,
    pub detail: String,
    pub stages: Vec<Stage>,
    pub content: Content,
}

impl Element {
    pub fn files(name: &str, detail: &str, stages: &[Stage], files: Vec<ResourceFile>) -> Element {
        Element { name: name.into(), detail: detail.into(), stages: stages.to_vec(), content: Content::Files(files) }
    }
    pub fn python(name: &str, detail: &str, pyproject: &str, lock: &str) -> Element {
        Element { name: name.into(), detail: detail.into(), stages: vec![Stage::Transcription], content: Content::Python { pyproject: pyproject.into(), lock: lock.into() } }
    }
    pub fn claude(name: &str) -> Element {
        Element { name: name.into(), detail: "CLI d'Anthropic".into(), stages: vec![Stage::Review, Stage::Summary], content: Content::Claude }
    }
}

/// Everything processing needs.
#[derive(Clone, Debug, PartialEq)]
pub struct Catalogue(pub Vec<Element>);

const RELEASE_PYANNOTE: &str = "https://github.com/FredeAlexandre/greffier/releases/download/pyannote-community-1";

impl Catalogue {
    /// Greffier's resources, with pinned sha256s (the vault's for whisper and Silero; those of
    /// pyannote revision 3533c8c, republished as a release of the repository by `cargo xtask publish-pyannote`).
    pub fn for_greffier() -> Catalogue {
        let pyannote = |path: &str, asset: &str, size: u64, sha: &str| ResourceFile::new(&format!("{PYANNOTE_MODEL}/{path}"), &format!("{RELEASE_PYANNOTE}/{asset}"), size, sha);
        Catalogue(vec![
            Element::files(
                "Whisper large-v3",
                "huggingface.co/ggerganov/whisper.cpp",
                &[Stage::Transcription],
                vec![ResourceFile::new(
                    WHISPER_MODEL,
                    "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3.bin",
                    3_095_033_483,
                    "64d182b440b98d5203c4f9bd541544d84c605196c4f7b845dfa11fb23594d1e2",
                )],
            ),
            Element::files(
                "Silero VAD v5.1.2",
                "huggingface.co/ggml-org/whisper-vad",
                &[Stage::Transcription],
                vec![ResourceFile::new(
                    VAD_MODEL,
                    "https://huggingface.co/ggml-org/whisper-vad/resolve/main/ggml-silero-v5.1.2.bin",
                    885_098,
                    "29940d98d42b91fbd05ce489f3ecf7c72f0a42f027e4875919a28fb4c04ea2cf",
                )],
            ),
            Element::files(
                "pyannote community-1",
                "release GitHub de Greffier · CC-BY-4.0",
                &[Stage::Transcription],
                vec![
                    pyannote("config.yaml", "config.yaml", 444, "5ce2bfa9a938dc132cec1172592d65173cbb8f444ea1e4133f10f9391de155be"),
                    pyannote("segmentation/pytorch_model.bin", "segmentation-pytorch_model.bin", 5_906_507, "7ad24338d844fb95985486eb1a464e32d229f6d7a03c9abe60f978bacf3f816e"),
                    pyannote("embedding/pytorch_model.bin", "embedding-pytorch_model.bin", 26_646_242, "6f10ff60898a1d185fa22e1d11e0bfa8a92efec811f11bca48cb8cafebefd929"),
                    pyannote("plda/plda.npz", "plda-plda.npz", 133_852, "9b77bcd840692710dd3496f62ecfeed8d8e5f002fd991b785079b244eab7d255"),
                    pyannote("plda/xvec_transform.npz", "plda-xvec_transform.npz", 134_376, "325f1ce8e48f7e55e9c8aa47e05d2766b7c48c4b25b8de8dd751e7a4cc5fbe8f"),
                ],
            ),
            Element::python("Python privé", "pyannote.audio 4.0.7 · installé par uv", PYPROJECT, LOCKFILE),
            Element::claude("Claude Code"),
        ])
    }

    /// What is left to download, in bytes.
    pub fn to_download(&self, root: &Path) -> u64 {
        self.0
            .iter()
            .flat_map(|e| match &e.content {
                Content::Files(f) => f.as_slice(),
                _ => &[],
            })
            .filter(|f| !is_ready(root, f))
            .map(|f| f.size - std::fs::metadata(partial(root, f)).map_or(0, |m| m.len()).min(f.size))
            .sum()
    }
}

/// pyannote's Python project, taken from the vault (dependencies pinned by `uv.lock`).
pub const PYPROJECT: &str = include_str!("../resources/python/pyproject.toml");
pub const LOCKFILE: &str = include_str!("../resources/python/uv.lock");

/// What the check of the `claude` CLI found.
#[derive(Clone, Debug, PartialEq)]
pub enum ClaudeState {
    Connected { version: String, account: String },
    NotConnected { version: String },
    Missing,
    Failed(String),
}

/// The resources' slow work, outside the core.
pub trait Tools {
    /// Downloads `url` into `partial`, resuming at the end of what it already contains.
    fn download(&mut self, url: &str, partial: &Path, size: u64) -> BoxedTask<()>;
    /// The file's sha256, in lowercase hexadecimal.
    fn fingerprint(&mut self, file: &Path) -> BoxedTask<String>;
    /// Installs the Python environment described by `project` (`pyproject.toml`, `uv.lock`) into
    /// `folder`, writing nothing elsewhere, then checks that `import pyannote.audio` succeeds there.
    fn install_python(&mut self, project: &Path, folder: &Path) -> BoxedTask<()>;
    fn check_claude(&mut self) -> BoxedTask<ClaudeState>;
}

/// The sha256 of bytes, in lowercase hexadecimal.
pub fn sha256(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|o| format!("{o:02x}")).collect()
}

fn final_(root: &Path, f: &ResourceFile) -> PathBuf {
    root.join(&f.path)
}

fn partial(root: &Path, f: &ResourceFile) -> PathBuf {
    root.join(format!("{}.partial", f.path))
}

fn size_on_disk(path: &Path) -> u64 {
    std::fs::metadata(path).map_or(0, |m| m.len())
}

/// A file is ready when it has its name at the right size: it is only renamed once its sha256
/// is verified.
fn is_ready(root: &Path, f: &ResourceFile) -> bool {
    std::fs::metadata(final_(root, f)).is_ok_and(|m| m.is_file() && m.len() == f.size)
}

/// The space taken by a folder, without following links.
fn storage(folder: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(folder) else { return 0 };
    entries
        .flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => storage(&e.path()),
            Ok(t) if t.is_file() => e.metadata().map_or(0, |m| m.len()),
            _ => 0,
        })
        .sum()
}

const PYTHON_FOLDER: &str = "python";

/// The Python project's fingerprint: the install stamp records it, a new lock file reinstalls
/// the environment.
fn project_fingerprint(pyproject: &str, lock: &str) -> String {
    sha256(format!("{pyproject}\0{lock}").as_bytes())
}

/// What is happening to an element during an installation.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Phase {
    Download,
    Verification,
    Installation,
}

struct Shared {
    root: PathBuf,
    catalogue: Catalogue,
    tools: Box<dyn Tools>,
    phases: HashMap<usize, Phase>,
    failures: HashMap<usize, String>,
    claude: Option<ClaudeState>,
    /// The space taken by the Python, computed once (the folder holds tens of thousands of
    /// files).
    python_disk_usage: Option<u64>,
}

impl Shared {
    fn python_folder(&self) -> PathBuf {
        self.root.join(PYTHON_FOLDER)
    }

    fn python_ready(&self, pyproject: &str, lock: &str) -> bool {
        std::fs::read_to_string(self.python_folder().join("ready")).is_ok_and(|t| t.trim() == project_fingerprint(pyproject, lock))
    }

    fn resource(&mut self, i: usize) -> Resource {
        let e = &self.catalogue.0[i];
        let mut r = Resource { name: e.name.clone(), detail: e.detail.clone(), state: ResourceState::Missing, size: 0, total: 0, help: None, stages: e.stages.clone() };
        let phase = self.phases.get(&i).copied();
        match &e.content {
            Content::Files(files) => {
                let total: u64 = files.iter().map(|f| f.size).sum();
                let received: u64 = files.iter().map(|f| size_on_disk(&final_(&self.root, f)) + size_on_disk(&partial(&self.root, f))).sum();
                r.size = received;
                r.total = total;
                r.state = match phase {
                    Some(Phase::Verification) => ResourceState::Verifying,
                    Some(_) => ResourceState::Downloading { received: received.min(total), total },
                    None if files.iter().all(|f| is_ready(&self.root, f)) => ResourceState::Ready,
                    None => match self.failures.get(&i) {
                        Some(m) => ResourceState::Failed(m.clone()),
                        None if received > 0 => ResourceState::Interrupted { received: received.min(total), total },
                        None => ResourceState::Missing,
                    },
                };
            }
            Content::Python { pyproject, lock } => {
                let is_ready = self.python_ready(pyproject, lock);
                r.state = match (phase, self.failures.get(&i)) {
                    (Some(_), _) => ResourceState::Installing,
                    (None, Some(m)) => ResourceState::Failed(m.clone()),
                    (None, None) if is_ready => ResourceState::Ready,
                    (None, None) => ResourceState::Missing,
                };
                if phase.is_none() {
                    let folder = self.python_folder();
                    r.size = *self.python_disk_usage.get_or_insert_with(|| storage(&folder));
                }
            }
            Content::Claude => {
                (r.state, r.help) = match (phase, &self.claude) {
                    (Some(_), _) | (None, None) => (ResourceState::Verifying, None),
                    (None, Some(ClaudeState::Connected { version, account })) => {
                        r.detail = format!("{version} · connectée ({account})");
                        (ResourceState::Ready, None)
                    }
                    (None, Some(ClaudeState::NotConnected { version })) => {
                        r.detail = format!("{version} · pas connectée");
                        (
                            ResourceState::Failed("La CLI claude est installée mais n'est pas connectée.".into()),
                            Some("Dans un terminal, lancer `claude` puis `/login`, puis cliquer sur « Vérifier à nouveau ».".into()),
                        )
                    }
                    (None, Some(ClaudeState::Missing)) => (
                        ResourceState::Missing,
                        Some("La relecture et le bilan passent par Claude Code. L'installer (https://claude.com/claude-code), lancer `claude` une fois pour se connecter, puis cliquer sur « Vérifier à nouveau ».".into()),
                    ),
                    (None, Some(ClaudeState::Failed(m))) => (ResourceState::Failed(m.clone()), Some("Vérifier que `claude` se lance dans un terminal, puis cliquer sur « Vérifier à nouveau ».".into())),
                };
            }
        }
        r
    }
}

/// The app's resources adapter, over an app folder and a catalogue.
pub struct AppResources {
    shared: Rc<RefCell<Shared>>,
}

impl AppResources {
    pub fn new(root: impl Into<PathBuf>, catalogue: Catalogue, tools: impl Tools + 'static) -> AppResources {
        let shared = Shared { root: root.into(), catalogue, tools: Box::new(tools), phases: HashMap::new(), failures: HashMap::new(), claude: None, python_disk_usage: None };
        AppResources { shared: Rc::new(RefCell::new(shared)) }
    }
}

impl Resources for AppResources {
    fn state(&mut self) -> Vec<Resource> {
        let mut p = self.shared.borrow_mut();
        (0..p.catalogue.0.len()).map(|i| p.resource(i)).collect()
    }

    fn install(&mut self) -> BoxedTask<()> {
        Box::new(Installation::start(self.shared.clone(), true))
    }

    fn check(&mut self) -> BoxedTask<()> {
        Box::new(Installation::start(self.shared.clone(), false))
    }
}

enum Operation {
    Download(BoxedTask<()>),
    Verification(BoxedTask<String>),
}

struct CurrentFile {
    element: usize,
    file: ResourceFile,
    operation: Operation,
}

/// An installation: the files one by one (smallest first), the Python at the same time, and the
/// check of `claude`.
struct Installation {
    shared: Rc<RefCell<Shared>>,
    remaining: VecDeque<(usize, ResourceFile)>,
    running: Option<CurrentFile>,
    python: Option<(usize, BoxedTask<()>)>,
    claude: Option<(usize, BoxedTask<ClaudeState>)>,
    failures: Vec<String>,
}

impl Installation {
    fn start(shared: Rc<RefCell<Shared>>, install: bool) -> Installation {
        let mut remaining = Vec::new();
        let mut python = None;
        let mut claude = None;
        let mut failures = Vec::new();
        {
            let mut p = shared.borrow_mut();
            let p = &mut *p;
            for (i, e) in p.catalogue.0.iter().enumerate() {
                match &e.content {
                    Content::Files(files) if install => {
                        p.failures.remove(&i);
                        let missing: Vec<ResourceFile> = files.iter().filter(|f| !is_ready(&p.root, f)).cloned().collect();
                        if !missing.is_empty() {
                            let total: u64 = missing.iter().map(|f| f.size).sum();
                            remaining.push((total, i, missing));
                        }
                    }
                    Content::Python { pyproject, lock } if install => {
                        p.failures.remove(&i);
                        if !p.python_ready(pyproject, lock) {
                            let folder = p.root.join(PYTHON_FOLDER);
                            let project = folder.join("project");
                            let written = std::fs::create_dir_all(&project)
                                .and_then(|()| std::fs::remove_file(folder.join("ready")).or_else(|e| if e.kind() == std::io::ErrorKind::NotFound { Ok(()) } else { Err(e) }))
                                .and_then(|()| std::fs::write(project.join("pyproject.toml"), pyproject))
                                .and_then(|()| std::fs::write(project.join("uv.lock"), lock));
                            match written {
                                Ok(()) => {
                                    p.phases.insert(i, Phase::Installation);
                                    python = Some((i, p.tools.install_python(&project, &folder)));
                                }
                                Err(err) => {
                                    let m = format!("{} : {err}", project.display());
                                    failures.push(format!("{} : {m}", e.name));
                                    p.failures.insert(i, m);
                                }
                            }
                        }
                    }
                    Content::Claude => {
                        p.phases.insert(i, Phase::Verification);
                        claude = Some((i, p.tools.check_claude()));
                    }
                    _ => {}
                }
            }
        }
        remaining.sort_by_key(|(total, i, _)| (*total, *i));
        let remaining = remaining.into_iter().flat_map(|(_, i, files)| files.into_iter().map(move |f| (i, f))).collect();
        let mut installation = Installation { shared, remaining, running: None, python, claude, failures };
        installation.next();
        installation
    }

    fn failure(&mut self, element: usize, message: String) {
        let mut p = self.shared.borrow_mut();
        p.phases.remove(&element);
        self.failures.push(format!("{} : {message}", p.catalogue.0[element].name));
        p.failures.insert(element, message);
        self.remaining.retain(|(e, _)| *e != element);
    }

    /// Starts the next file in the queue, if none is already running.
    fn next(&mut self) {
        while self.running.is_none() {
            let Some((element, file)) = self.remaining.pop_front() else { return };
            let mut p = self.shared.borrow_mut();
            let (end, part) = (final_(&p.root, &file), partial(&p.root, &file));
            if is_ready(&p.root, &file) {
                continue;
            }
            // A file at the wrong size under its final name was not verified: throw it away.
            let _ = std::fs::remove_file(&end);
            if size_on_disk(&part) > file.size {
                let _ = std::fs::remove_file(&part);
            }
            if let Some(parent) = part.parent()
                && let Err(e) = std::fs::create_dir_all(parent)
            {
                drop(p);
                self.failure(element, format!("{} : {e}", parent.display()));
                continue;
            }
            let operation = if size_on_disk(&part) == file.size {
                p.phases.insert(element, Phase::Verification);
                Operation::Verification(p.tools.fingerprint(&part))
            } else {
                p.phases.insert(element, Phase::Download);
                Operation::Download(p.tools.download(&file.url, &part, file.size))
            };
            self.running = Some(CurrentFile { element, file, operation });
        }
    }

    fn advance_files(&mut self, elapsed: f64) {
        let Some(mut c) = self.running.take() else { return };
        let (end, part) = {
            let p = self.shared.borrow();
            (final_(&p.root, &c.file), partial(&p.root, &c.file))
        };
        match &mut c.operation {
            Operation::Download(t) => match t.poll(elapsed) {
                TaskProgress::Running(_) => self.running = Some(c),
                TaskProgress::Done(()) => {
                    let mut p = self.shared.borrow_mut();
                    p.phases.insert(c.element, Phase::Verification);
                    c.operation = Operation::Verification(p.tools.fingerprint(&part));
                    self.running = Some(c);
                }
                TaskProgress::Failed(m) => {
                    self.failure(c.element, format!("Téléchargement interrompu ({m}). Il reprendra là où il s'est arrêté."));
                }
            },
            Operation::Verification(t) => match t.poll(elapsed) {
                TaskProgress::Running(_) => self.running = Some(c),
                TaskProgress::Done(fingerprint) if fingerprint == c.file.sha256 => match std::fs::rename(&part, &end) {
                    Ok(()) => {
                        if !self.remaining.iter().any(|(e, _)| *e == c.element) {
                            self.shared.borrow_mut().phases.remove(&c.element);
                        }
                    }
                    Err(e) => self.failure(c.element, format!("{} : {e}", end.display())),
                },
                TaskProgress::Done(fingerprint) => {
                    let _ = std::fs::remove_file(&part);
                    self.failure(
                        c.element,
                        format!("sha256 incorrect pour {} (reçu {fingerprint}, attendu {}). Le fichier est supprimé ; il sera retéléchargé au prochain essai.", c.file.path, c.file.sha256),
                    );
                }
                TaskProgress::Failed(m) => self.failure(c.element, format!("Vérification du sha256 impossible ({m}).")),
            },
        }
        self.next();
    }

    fn advance_python(&mut self, elapsed: f64) {
        let Some((i, t)) = &mut self.python else { return };
        let i = *i;
        match t.poll(elapsed) {
            TaskProgress::Running(_) => {}
            TaskProgress::Done(()) => {
                self.python = None;
                let mut p = self.shared.borrow_mut();
                p.phases.remove(&i);
                p.python_disk_usage = None;
                if let Content::Python { pyproject, lock } = &p.catalogue.0[i].content {
                    let stamp = project_fingerprint(pyproject, lock);
                    let path = p.python_folder().join("ready");
                    if let Err(e) = std::fs::write(&path, stamp) {
                        drop(p);
                        self.failure(i, format!("{} : {e}", path.display()));
                    }
                }
            }
            TaskProgress::Failed(m) => {
                self.python = None;
                self.shared.borrow_mut().python_disk_usage = None;
                self.failure(i, m);
            }
        }
    }

    fn advance_claude(&mut self, elapsed: f64) {
        let Some((i, t)) = &mut self.claude else { return };
        let i = *i;
        let result = match t.poll(elapsed) {
            TaskProgress::Running(_) => return,
            TaskProgress::Done(e) => e,
            TaskProgress::Failed(m) => ClaudeState::Failed(m),
        };
        self.claude = None;
        let mut p = self.shared.borrow_mut();
        p.phases.remove(&i);
        p.claude = Some(result);
    }
}

impl crate::adapters::Task<()> for Installation {
    fn poll(&mut self, elapsed: f64) -> TaskProgress<()> {
        self.advance_files(elapsed);
        self.advance_python(elapsed);
        self.advance_claude(elapsed);
        if self.running.is_some() || self.python.is_some() || self.claude.is_some() {
            return TaskProgress::Running(0.0);
        }
        if self.failures.is_empty() { TaskProgress::Done(()) } else { TaskProgress::Failed(self.failures.join("\n")) }
    }

    fn stop(&mut self) {
        let mut p = self.shared.borrow_mut();
        if let Some(c) = &mut self.running {
            match &mut c.operation {
                Operation::Download(t) => t.stop(),
                Operation::Verification(t) => t.stop(),
            }
        }
        if let Some((_, t)) = &mut self.python {
            t.stop();
        }
        if let Some((_, t)) = &mut self.claude {
            t.stop();
        }
        p.phases.clear();
        p.python_disk_usage = None;
        self.running = None;
        self.python = None;
        self.claude = None;
        self.remaining.clear();
    }
}
