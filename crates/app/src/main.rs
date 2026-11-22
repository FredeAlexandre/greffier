//! Greffier — the UI (Rune). It reads the core's state (`greffier_core`) and sends it commands.
//! The core reads the meetings of the meetings folder chosen at first launch and processes them
//! with the real adapters (whisper.cpp, the `claude` CLI…). The resources (models, private
//! Python, `claude` CLI) live in `~/Library/Application Support/Greffier`.
//!
//! `cargo run` opens the window. `cargo run -- --snap out.png --scene meeting` renders a scene
//! headless (`--list-scenes` for the list): headless, the app starts from the prototype's fake
//! data, never from the meetings folder. `GREFFIER_FOLDER=… cargo run` opens another meetings
//! folder for a single launch.

#![forbid(unsafe_code)]

mod import;
mod speakers;
mod meeting;
mod overlays;
mod pages;
mod record_shortcut;
mod redo;
mod settings;
mod sidebar;
mod watch;
mod timeline;
mod title;
mod ui;
mod check;

use std::path::{Path, PathBuf};
use std::time::Duration;

use rune::{AppHandle, AppSpec, Chrome, Cx, El, FileDialogOptions, Key, KeyEvent, Player, PlayerState, PointerPress, Shortcut, ShortcutError, ShortcutId, Spring, TextEdit, Tx, Waker};
use rune_kit::shell::{self, PaneMotion, PaneState, SidebarToggle, SplitView};
use rune_kit::{ControlSize, Kit, ShortcutRecorderState};

use greffier_menubar::{Choice, MenubarIcon};
use greffier_capture::sleep::KeepAwake;
use greffier_core::adapters::ResourceState;
use greffier_core::fake::FakeResources;
use greffier_core::{Command, Stage, Event, Greffier, MeetingId, Rejection, Meeting, Role, demo, lexicon};

use record_shortcut::{RecordShortcut, Registrar};

pub const SPEEDS: [f64; 3] = [1.0, 1.5, 2.0];

#[derive(Clone, Copy, PartialEq)]
pub enum Page {
    /// First launch: choose the meetings folder.
    Welcome,
    Overview,
    Recording,
    Meeting(MeetingId),
    Settings,
    /// First launch: download and check the resources.
    Installation,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Tab {
    Summary,
    Transcript,
    Doubts,
    Speakers,
    Info,
}

pub const TABS: [Tab; 5] = [Tab::Summary, Tab::Transcript, Tab::Doubts, Tab::Speakers, Tab::Info];

pub struct Toast {
    pub text: String,
    pub meeting: Option<MeetingId>,
    pub until: f64,
}

pub struct App {
    /// The core: meetings, recording, processing queue. Only changed through commands.
    pub core: Greffier,
    pub sidebar: PaneState,
    pub page: Page,
    pub tab: Tab,
    pub pos: f64,
    pub playing: bool,
    pub speed: usize,
    pub follow: bool,
    pub current: Option<usize>,
    pub followed: Option<usize>,
    /// Frames since the Transcript tab opened (the list must exist before it can scroll).
    pub transcript_frames: u32,
    /// Phase of the recording animation (pulse, levels).
    pub rec_phase: f64,
    /// Correcting a speaker's name or merging a speaker, in the Speakers tab.
    pub picking: Option<speakers::Picking>,
    pub toast: Option<Toast>,
    pub clock: f64,
    pub last: Option<f64>,
    pub collapsed: [bool; 5],
    /// ⌘K search: open when `Some`.
    pub search: Option<TextEdit>,
    /// Right-click menu on a meeting in the sidebar.
    pub menu: Option<(MeetingId, PointerPress)>,
    pub confirm_delete: Option<MeetingId>,
    /// Full-screen video (the window also goes macOS full screen).
    pub fullscreen: bool,
    /// The open meeting's recording, when its folder has one. Without it (fake data), playback
    /// is simulated.
    pub video: Option<(MeetingId, Player)>,
    /// Wakes the UI when the video loads, ends or fails.
    pub waker: Option<Waker>,
    /// The chosen meetings folder (`None` for the prototype's fake data).
    pub folder: Option<PathBuf>,
    /// First launch: nothing but choosing the meetings folder until it is done (no recording
    /// may go into a folder that was not chosen).
    pub needs_folder: bool,
    /// The last problem with the meetings folder, shown in the settings.
    pub folder_error: Option<String>,
    /// To register (and unregister) the global shortcuts once the app is running.
    pub handle: Option<AppHandle<App>>,
    /// The global shortcut that starts and stops recording (configurable): the settings field,
    /// and what is registered of it.
    pub shortcut: ShortcutRecorderState,
    pub record_shortcut: RecordShortcut<ShortcutId>,
    /// ⌘M adds a marker from anywhere, for the duration of the recording.
    pub marker_id: Option<ShortcutId>,
    /// Why the recording did not start (permission, source…).
    pub record_error: Option<String>,
    /// The recordings shorter than a minute offered for deletion, oldest first: the question is
    /// asked about each in turn. The core does not process one until it is kept
    /// ([`App::keep_short_recording`]).
    pub short_recordings: Vec<MeetingId>,
    /// What the lexicon contains, for the settings.
    pub lexicon: lexicon::Overview,
    /// The lexicon's people: the names offered in one click to name a speaker.
    pub people: Vec<String>,
    /// Reports changes made in the meetings folder outside the app.
    pub watch: Option<watch::FolderWatch>,
    /// The import window, open when `Some`.
    pub import: Option<import::Draft>,
    /// The meeting just imported: we ask when to start its processing.
    pub process_imported: Option<MeetingId>,
    /// The menu bar icon (window only, installed at launch).
    pub menu_bar: Option<MenubarIcon>,
    /// The « Changer le titre » window, open when `Some`.
    pub rename: Option<title::Renaming>,
    /// The « Refaire la transcription » window, open when `Some`.
    pub redo_transcription: Option<redo::RedoTranscription>,
    /// The Transcript tab's « Refaire… » menu, open.
    pub redo_menu: bool,
    /// The keep-awake assertion, held during a recording or processing (window only).
    pub awake: Option<KeepAwake>,
    /// The passage to check selected with the keyboard (see `check::selected`).
    pub doubt: usize,
    /// The manual correction in progress (passage to check or utterance).
    pub correction: Option<check::Correction>,
    /// « Écouter » on a passage: playback stops at the end of its excerpt.
    pub listen_until: Option<f64>,
}

impl App {
    pub fn meeting(&self) -> Option<&Meeting> {
        match self.page {
            Page::Meeting(id) => self.core.meeting(id),
            _ => None,
        }
    }
    /// A window is open over the page: the page's shortcuts, ⌘ ones included, wait until it
    /// closes.
    pub fn window_open(&self) -> bool {
        self.search.is_some()
            || self.rename.is_some()
            || self.import.is_some()
            || self.process_imported.is_some()
            || self.confirm_delete.is_some()
            || !self.short_recordings.is_empty()
            || self.redo_transcription.is_some()
    }
    /// A text field is open: letters, space and arrows are typing, even after a click elsewhere
    /// took the field's focus, so no single-key shortcut of the page fires. Every field of the
    /// app belongs here.
    pub fn text_field_open(&self) -> bool {
        self.search.is_some() || self.rename.is_some() || self.import.is_some() || self.correction.is_some() || speakers::naming_shown(self)
    }
    /// The open meeting's video.
    pub fn player(&self) -> Option<&Player> {
        match (self.page, &self.video) {
            (Page::Meeting(id), Some((vid, p))) if id == *vid => Some(p),
            _ => None,
        }
    }
    /// The open meeting's duration: that of its files, otherwise that of the video.
    pub fn duration(&self) -> f64 {
        let d = self.meeting().map_or(0.0, |m| m.duration);
        if d > 0.0 { d } else { self.player().and_then(Player::duration).unwrap_or(0.0) }
    }
    /// Sends a command to the core and acts on its events.
    pub fn command(&mut self, c: Command) {
        if let Err(rejection) = self.try_command(c) {
            eprintln!("greffier: {rejection}");
        }
    }
    fn try_command(&mut self, c: Command) -> Result<(), Rejection> {
        let events = self.core.execute(c);
        self.follow_core();
        self.handle(events?);
        Ok(())
    }
    fn handle(&mut self, events: Vec<Event>) {
        for e in events {
            match e {
                Event::MeetingRecorded(id) => self.open(id),
                Event::RecordingInterrupted { meeting, message } => {
                    eprintln!("greffier: {message}");
                    self.register_marker_shortcut(false);
                    self.open(meeting);
                    self.toast = Some(Toast { text: "Enregistrement interrompu : ce qui a été capturé est gardé.".into(), meeting: Some(meeting), until: self.clock + 8.0 });
                    if let Some(h) = &self.handle {
                        h.open();
                    }
                }
                Event::DeletionProposed(id) => self.short_recordings.push(id),
                Event::MeetingImported(id) => self.imported(id),
                Event::SummaryReady { meeting, title } => {
                    if self.page == Page::Meeting(meeting) {
                        self.command(Command::MarkRead(meeting));
                    }
                    self.toast = Some(Toast { text: format!("Bilan prêt : {title}"), meeting: Some(meeting), until: self.clock + 6.0 });
                    self.notify_summary(meeting);
                }
                Event::StageDone { stage: Stage::Review, .. } if !rune::is_headless() => {
                    // The review may have added to the lexicon.
                    self.lexicon = self.core.lexicon();
                    self.people = settings::people(&self.core);
                }
                Event::StageFailed { message, .. } | Event::WriteFailed { message, .. } => eprintln!("greffier: {message}"),
                Event::ResourcesInstalled => {
                    self.toast = Some(Toast { text: "Ressources installées : Greffier est prêt.".into(), meeting: None, until: self.clock + 6.0 });
                }
                Event::InstallationFailed(message) => eprintln!("greffier: resource installation: {message}"),
                Event::MeetingDisappeared(id) => self.forget(id),
                Event::MeetingReviewed(id) if self.page == Page::Meeting(id) => {
                    // The utterances and speakers may have changed.
                    self.picking = None;
                    self.current = self.meeting().and_then(|m| m.utterance_at(self.pos));
                    if self.video.is_none() {
                        self.load_video(id);
                    }
                }
                _ => {}
            }
        }
    }
    /// Opens the lexicon in the default text editor (headless: does nothing).
    pub fn open_lexicon(&mut self) {
        if rune::is_headless() {
            return;
        }
        let Some(path) = settings::lexicon_path() else { return };
        if let Err(e) = std::process::Command::new("/usr/bin/open").arg("-t").arg(&path).spawn() {
            eprintln!("greffier: opening the lexicon: {e}");
        }
    }
    pub fn open(&mut self, id: MeetingId) {
        if self.page != Page::Meeting(id) {
            self.page = Page::Meeting(id);
            self.redo_menu = false;
            self.pos = 0.0;
            self.playing = false;
            self.current = None;
            self.followed = None;
            self.picking = None;
            self.doubt = 0;
            self.correction = None;
            self.tab = Tab::Summary;
            self.fullscreen = false;
            self.load_video(id);
        }
        if self.core.meeting(id).is_some_and(|m| m.is_new) {
            self.command(Command::MarkRead(id));
        }
    }
    /// The macOS « Bilan prêt » notification, when the app is not in front (otherwise it already
    /// has its toast). A click opens the meeting (`open_folder_named`), found by its folder: the
    /// id does not survive an app restart. Only the installed app can post.
    fn notify_summary(&self, id: MeetingId) {
        if rune::is_headless() || rune_notify::app_focused() {
            return;
        }
        let Some(m) = self.core.meeting(id) else { return };
        let n = rune_notify::Notification::new(format!("summary:{}", m.folder_name), "Bilan prêt")
            .body(meeting::display_title(m, true))
            .info("folder", m.folder_name.clone());
        rune_notify::post(&n);
    }
    /// Opens the meeting whose folder has this name (click on a notification).
    pub fn open_folder_named(&mut self, name: &str) {
        if let Some(id) = self.core.meetings().iter().find(|r| r.folder_name == name).map(|r| r.id) {
            self.open(id);
        }
    }
    pub fn open_at(&mut self, id: MeetingId, t: f64) {
        self.open(id);
        self.seek(t);
    }
    /// Opens the meeting's recording, sound on, if there is one.
    fn load_video(&mut self, id: MeetingId) {
        self.video = None;
        let Some(m) = self.core.meeting(id) else { return };
        if !m.files.contains(&Role::Recording) {
            return;
        }
        let Some(file) = self.core.recording_to_play(id) else { return };
        let player = Player::file(file);
        player.set_muted(false);
        player.set_rate(SPEEDS[self.speed] as f32);
        if let Some(w) = &self.waker {
            player.set_waker(w.clone());
        }
        self.video = Some((id, player));
    }
    pub fn seek(&mut self, t: f64) {
        self.listen_until = None;
        self.pos = t.clamp(0.0, self.duration());
        if let Some(p) = self.player() {
            p.seek(self.pos);
        }
        self.current = self.meeting().and_then(|m| m.utterance_at(self.pos));
    }
    pub fn seek_and_play(&mut self, t: f64) {
        self.seek(t);
        self.play();
    }
    fn play(&mut self) {
        // A video the player cannot open ignores `play`: say why rather than flicker.
        if let Some(PlayerState::Failed(e)) = self.player().map(Player::state) {
            eprintln!("greffier: video: {e}");
            self.toast = Some(Toast { text: format!("Impossible de lire la vidéo : {e}"), meeting: None, until: self.clock + 8.0 });
            return;
        }
        if let Some(p) = self.player() {
            p.play();
        }
        self.playing = true;
    }
    fn pause(&mut self) {
        self.listen_until = None;
        if let Some(p) = self.player() {
            p.pause();
        }
        self.playing = false;
    }
    pub fn toggle_play(&mut self) {
        if !self.meeting().is_some_and(meeting::playable) {
            return;
        }
        if self.playing {
            self.pause();
        } else {
            self.play();
        }
    }
    pub fn set_speed(&mut self, i: usize) {
        self.speed = i;
        if let Some(p) = self.player() {
            p.set_rate(SPEEDS[i] as f32);
        }
    }
    /// Reads the meetings of a meetings folder and remembers it for later launches.
    pub fn use_folder(&mut self, dir: PathBuf) {
        if self.core.busy() {
            self.folder_error = Some("Impossible de changer de dossier pendant un enregistrement ou un traitement.".into());
            return;
        }
        if let Err(e) = settings::choose_meetings_folder(&dir) {
            self.folder_error = Some(e);
            return;
        }
        let settings = self.core.settings().clone();
        self.core = real_core(&dir, true);
        self.command(Command::ChangeSettings(settings));
        self.folder = Some(dir);
        self.watch_folder();
        self.needs_folder = false;
        self.follow_core();
        self.folder_error = None;
        self.video = None;
        self.playing = false;
        if self.page == Page::Welcome && resources_missing(&self.core) {
            self.page = Page::Installation;
        } else if matches!(self.page, Page::Welcome | Page::Meeting(_)) {
            self.page = Page::Overview;
        }
    }
    /// The macOS folder picker, to choose the meetings folder.
    pub fn pick_folder(&mut self, tx: &mut Tx) {
        let directory = self.folder.clone().or_else(|| settings::suggested_folder().parent().map(Path::to_path_buf));
        let options = FileDialogOptions { title: "Dossier de réunions".into(), directory, ..Default::default() };
        tx.open_folder(options, |s: &mut App, answer, _| match answer {
            Ok(Some(dir)) => s.use_folder(dir),
            Ok(None) => {}
            Err(e) => s.folder_error = Some(e.to_string()),
        });
    }
    pub fn recorded(&self) -> bool {
        self.core.recording().is_some()
    }
    /// Installs the menu bar icon. Its choices do the same as the global shortcut, on the UI
    /// thread (`AppHandle::update`), like it.
    fn install_menu_bar(&mut self) {
        let Some(h) = self.handle.clone() else { return };
        self.menu_bar = MenubarIcon::new(move |choice| match choice {
            Choice::Start => h.update(|s: &mut App, _| if !s.recorded() { s.toggle_recording() }),
            Choice::Stop => h.update(|s: &mut App, _| if s.recorded() { s.toggle_recording() }),
            Choice::Open => h.open(),
            Choice::Quit => h.quit(),
        });
        self.show_menu_bar();
    }
    /// What follows the core's state outside the window: the menu bar icon and the keep-awake
    /// assertion.
    fn follow_core(&mut self) {
        self.show_menu_bar();
        self.keep_awake();
    }
    /// The Mac does not sleep during a recording (nor does the screen) or a processing: the
    /// assertion follows [`Greffier::keep_mac_awake`] (taken again when the reason changes).
    fn keep_awake(&mut self) {
        let keep_awake = if rune::is_headless() { None } else { self.core.keep_mac_awake() };
        if self.awake.as_ref().map(|a| (a.reason(), a.screen())) == keep_awake.map(|e| (e.reason, e.screen)) {
            return;
        }
        self.awake = None;
        if let Some(keep_awake) = keep_awake {
            match KeepAwake::new(keep_awake.reason, keep_awake.screen) {
                Ok(a) => self.awake = Some(a),
                Err(e) => eprintln!("greffier: {e}"),
            }
        }
    }
    /// Shows the recording state in the menu bar.
    fn show_menu_bar(&mut self) {
        let state = match self.core.recording() {
            Some(r) => greffier_menubar::State::Recording { duration: r.duration },
            None => greffier_menubar::State::Ready { possible: !self.needs_folder },
        };
        if let Some(b) = &mut self.menu_bar {
            b.show(state);
        }
    }
    pub fn start_recording(&mut self) {
        if self.needs_folder {
            return;
        }
        if !self.recorded() {
            self.rec_phase = 0.0;
            self.record_error = self.try_command(Command::StartRecording).err().map(|r| r.to_string());
            if self.recorded() {
                self.register_marker_shortcut(true);
            } else if let Some(h) = &self.handle {
                // Started from the menu bar or the shortcut with the window hidden: show why.
                h.open();
            }
        }
        if let Some((_, p)) = &self.video {
            p.pause();
        }
        self.playing = false;
        self.page = Page::Recording;
    }
    pub fn stop_recording(&mut self) {
        if self.recorded() {
            self.command(Command::StopRecording);
        }
        self.register_marker_shortcut(false);
    }
    pub fn toggle_recording(&mut self) {
        if self.recorded() { self.stop_recording() } else { self.start_recording() }
    }
    /// ⌘M from anywhere during the recording (given back to other apps when it stops).
    fn register_marker_shortcut(&mut self, on: bool) {
        let Some(h) = &self.handle else { return };
        match (on, self.marker_id) {
            (true, None) => match h.register_shortcut("cmd+m".parse().expect("cmd+m"), |s: &mut App, _| s.add_marker()) {
                Ok(id) => self.marker_id = Some(id),
                Err(e) => eprintln!("greffier: ⌘M for markers: {e}"),
            },
            (false, Some(id)) => {
                let _ = h.unregister_shortcut(id);
                self.marker_id = None;
            }
            _ => {}
        }
    }
    /// Registers the remembered global recording shortcut, at launch.
    pub fn register_record_shortcut(&mut self) {
        let shortcut = self.shortcut.shortcut().cloned();
        if let Err(e) = self.change_shortcut(shortcut.as_ref()) {
            eprintln!("greffier: shortcut {}: {e}", shortcut.map(|s| s.to_string()).unwrap_or_default());
            self.shortcut.mark_unregistered();
        }
    }
    /// Replaces the global recording shortcut and remembers it; keeps the old one (and
    /// remembers nothing) if the new one is refused.
    pub fn change_shortcut(&mut self, next: Option<&Shortcut>) -> Result<(), ShortcutError> {
        let h = self.handle.as_ref().ok_or(ShortcutError::NotRunning)?;
        self.record_shortcut.change(h, next)?;
        // The field takes `next` only once this returns: remember `next`, not the field's value.
        self.save_settings_with(next);
        Ok(())
    }
    /// The settings field of the recording shortcut. Each press or key it handles may start or
    /// end a capture: the registration follows right after (`follow_shortcut_capture`).
    pub fn shortcut_field(&mut self) -> &mut ShortcutRecorderState {
        if let Some(h) = &self.handle {
            h.update_if(|s: &mut App, _| {
                s.follow_shortcut_capture();
                false
            });
        }
        &mut self.shortcut
    }
    /// Releases the global recording shortcut while the settings field captures one (its keys
    /// reach the field instead of starting a recording), and registers the field's value after.
    fn follow_shortcut_capture(&mut self) {
        let Some(h) = &self.handle else { return };
        let chosen = self.shortcut.shortcut().cloned();
        if let Err(e) = self.record_shortcut.follow_capture(h, self.shortcut.is_recording(), chosen.as_ref()) {
            eprintln!("greffier: shortcut {}: {e}", chosen.map(|s| s.to_string()).unwrap_or_default());
            self.shortcut.mark_unregistered();
        }
    }
    /// Remembers sources, automatic start and shortcut (not for the fake data).
    fn save_settings(&mut self) {
        let shortcut = self.shortcut.shortcut().cloned();
        self.save_settings_with(shortcut.as_ref());
    }
    /// [`App::save_settings`] with `shortcut` as the recording shortcut (`None`: none).
    fn save_settings_with(&self, shortcut: Option<&Shortcut>) {
        if rune::is_headless() || self.folder.is_none() && !self.needs_folder {
            return;
        }
        let shortcut = shortcut.map(|s| s.to_string()).unwrap_or_default();
        if let Err(e) = settings::remember_recording_settings(self.core.settings(), &shortcut) {
            eprintln!("greffier: settings: {e}");
        }
    }
    pub fn process(&mut self, id: MeetingId) {
        self.command(Command::Process(id));
    }
    /// The short recording the question is about: the oldest one not answered yet.
    pub fn short_recording(&self) -> Option<MeetingId> {
        self.short_recordings.first().copied()
    }
    /// Answers « Garder » to the short recording question: the meeting is kept, and processed if
    /// processing starts at stop. The question moves on to the next short recording, if any.
    pub fn keep_short_recording(&mut self) {
        if let Some(id) = self.short_recording() {
            self.short_recordings.remove(0);
            self.command(Command::KeepShortRecording(id));
        }
    }
    pub fn stop_processing(&mut self, id: MeetingId) {
        self.command(Command::StopProcessing(id));
    }
    /// Moves the meeting to the trash (after confirmation); if the trash refuses, it stays and a
    /// toast says why.
    pub fn delete(&mut self, id: MeetingId) {
        self.confirm_delete = None;
        match self.try_command(Command::Delete(id)) {
            Ok(()) => self.forget(id),
            Err(rejection) => self.toast = Some(Toast { text: format!("Suppression impossible : {rejection}"), meeting: Some(id), until: self.clock + 8.0 }),
        }
    }
    /// Shows the meeting's folder in the Finder (headless: does nothing).
    pub fn reveal(&mut self, id: MeetingId) {
        let Some(folder) = self.core.meeting_folder(id) else { return };
        if rune::is_headless() {
            return;
        }
        if let Err(e) = std::process::Command::new("/usr/bin/open").arg("-R").arg(&folder).spawn() {
            eprintln!("greffier: show in Finder: {e}");
        }
    }
    /// Copies the summary, as written (Markdown), to the clipboard.
    pub fn copy_summary(&mut self, id: MeetingId, tx: &mut Tx) {
        let Some(m) = self.core.meeting(id) else { return };
        if m.summary.trim().is_empty() {
            return;
        }
        tx.copy(m.summary.clone());
        self.toast = Some(Toast { text: "Bilan copié en Markdown.".into(), meeting: Some(id), until: self.clock + 4.0 });
    }
    /// The meeting no longer exists (deleted in or outside the app): leave its page.
    fn forget(&mut self, id: MeetingId) {
        if self.video.as_ref().is_some_and(|(v, _)| *v == id) {
            self.video = None;
        }
        if self.page == Page::Meeting(id) {
            self.page = Page::Overview;
            self.playing = false;
        }
        if self.menu.is_some_and(|(m, _)| m == id) {
            self.menu = None;
        }
        if self.confirm_delete == Some(id) {
            self.confirm_delete = None;
        }
        self.short_recordings.retain(|r| *r != id);
    }
    /// Watches the chosen meetings folder, once the UI is ready to be woken.
    fn watch_folder(&mut self) {
        self.watch = None;
        let (Some(dir), Some(waker)) = (&self.folder, &self.waker) else { return };
        match watch::FolderWatch::new(dir, waker.clone()) {
            Ok(w) => self.watch = Some(w),
            Err(e) => eprintln!("greffier: cannot watch the meetings folder ({e})"),
        }
    }
    pub fn add_marker(&mut self) {
        if self.recorded() {
            self.command(Command::AddMarker);
        }
    }
    /// Screen, Mac audio, microphone.
    pub fn source(&self, i: usize) -> bool {
        let s = &self.core.settings().sources;
        [s.screen, s.mac_audio, s.micro][i]
    }
    pub fn toggle_source(&mut self, i: usize) {
        let mut r = self.core.settings().clone();
        let s = &mut r.sources;
        let v = [&mut s.screen, &mut s.mac_audio, &mut s.micro];
        *v[i] = !*v[i];
        self.command(Command::ChangeSettings(r));
        self.record_error = None;
        self.save_settings();
    }
    pub fn toggle_auto_process(&mut self) {
        let mut r = self.core.settings().clone();
        r.process_on_stop = !r.process_on_stop;
        self.command(Command::ChangeSettings(r));
        self.save_settings();
    }
    /// The macOS folder picker, to open a context folder to the agent.
    pub fn pick_context_folder(&mut self, tx: &mut Tx) {
        let options = FileDialogOptions { title: "Dossier de contexte de l'agent".into(), directory: self.folder.clone(), ..Default::default() };
        tx.open_folder(options, |s: &mut App, answer, _| {
            if let Ok(Some(dir)) = answer {
                let mut r = s.core.settings().clone();
                if !r.context_folders.contains(&dir) {
                    r.context_folders.push(dir);
                    s.command(Command::ChangeSettings(r));
                    s.save_settings();
                }
            }
        });
    }
    pub fn remove_context_folder(&mut self, i: usize) {
        let mut r = self.core.settings().clone();
        if i < r.context_folders.len() {
            r.context_folders.remove(i);
            self.command(Command::ChangeSettings(r));
            self.save_settings();
        }
    }
    pub fn toggle_fullscreen(&mut self, tx: &mut Tx) {
        self.fullscreen = !self.fullscreen && self.meeting().is_some_and(meeting::playable);
        tx.toggle_full_screen();
    }
    /// Shows a tab of the meeting's page. Focus leaves the control that had it (a field of the
    /// tab that goes away, the tab strip): the page's keys, Space and those of the tab, act at once.
    pub fn show_tab(&mut self, tab: Tab, tx: &mut Tx) {
        self.tab = tab;
        tx.blur();
    }
    /// Opens a meeting from the sidebar. A click leaves nothing focused, so Space plays at once.
    /// The arrow keys keep focus on the list, to go on through it; Space or Return on the open
    /// meeting's row then play or pause, as Space does on the page.
    pub fn open_from_sidebar(&mut self, id: MeetingId, tx: &mut Tx) {
        if !tx.by_keys() {
            self.open(id);
            tx.blur();
        } else if self.page == Page::Meeting(id) {
            self.toggle_play();
        } else {
            self.open(id);
        }
    }
    pub fn open_search(&mut self) {
        let mut edit = TextEdit::new("");
        edit.focused = true;
        self.search = Some(edit);
        self.menu = None;
    }
}

/// The prototype's meetings folder: a temporary directory, emptied at each launch.
fn prototype_folder() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join("greffier-prototype");
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// The prototype's core: fake data, temporary folder.
fn prototype() -> Greffier {
    Greffier::with_meetings(prototype_folder(), demo::adapters(), demo::meetings())
}

/// The window's core on a meetings folder, with the real resources, whose checks it starts right
/// away, and the lexicon and search cache from the app folder.
fn real_core(folder: &Path, meetings: bool) -> Greffier {
    let mut core = if meetings { Greffier::open(folder, settings::adapters()) } else { Greffier::with_meetings(folder, settings::adapters(), vec![]) };
    let lexicon = settings::lexicon_path();
    let search_cache = Some(settings::search_cache());
    for c in [Command::ChangeSettings(greffier_core::Settings { lexicon, search_cache, ..core.settings().clone() }), Command::CheckResources] {
        if let Err(e) = core.execute(c) {
            eprintln!("greffier: {e}");
        }
    }
    core
}

/// The first-launch resources screen, as long as transcription cannot run.
fn resources_missing(core: &Greffier) -> bool {
    core.resources().iter().any(|r| r.stages.contains(&Stage::Transcription) && r.state != ResourceState::Ready)
}

fn init() -> App {
    // Headless (scenes, snapshots), the app starts from the fake data: deterministic rendering,
    // and never a write to the real meetings folder or to the app folder.
    let folder = if rune::is_headless() { None } else { settings::meetings_folder() };
    let (mut core, page) = match &folder {
        _ if rune::is_headless() => (prototype(), Page::Overview),
        Some(dir) => {
            let core = real_core(dir, true);
            let page = if resources_missing(&core) { Page::Installation } else { Page::Overview };
            (core, page)
        }
        None => (real_core(&settings::suggested_folder(), false), Page::Welcome),
    };
    // Headless, the default settings: deterministic rendering.
    let remembered = if rune::is_headless() { greffier_core::settings::PersistedSettings::default() } else { settings::remembered() };
    // The lexicon and the cache come from the app folder (`real_core`), not from the remembered settings.
    let settings = remembered.apply(core.settings().clone());
    let _ = core.execute(Command::ChangeSettings(settings));
    let shortcut = remembered.shortcut;
    let lexicon = core.lexicon();
    let people = settings::people(&core);
    App {
        core,
        sidebar: PaneState::new(272.0).range(200.0, 420.0),
        page,
        tab: Tab::Summary,
        pos: 0.0,
        playing: false,
        speed: 0,
        follow: true,
        current: None,
        followed: None,
        transcript_frames: 0,
        rec_phase: 0.0,
        picking: None,
        toast: None,
        clock: 0.0,
        last: None,
        collapsed: [false; 5],
        search: None,
        menu: None,
        confirm_delete: None,
        fullscreen: false,
        video: None,
        waker: None,
        needs_folder: page == Page::Welcome,
        folder,
        folder_error: None,
        handle: None,
        shortcut: ShortcutRecorderState::new(shortcut.parse().ok()),
        record_shortcut: RecordShortcut::default(),
        marker_id: None,
        record_error: None,
        short_recordings: Vec::new(),
        lexicon,
        people,
        watch: None,
        import: None,
        process_imported: None,
        menu_bar: None,
        rename: None,
        redo_transcription: None,
        redo_menu: false,
        awake: None,
        doubt: 0,
        correction: None,
        listen_until: None,
    }
}

fn setup(s: &mut App, waker: Waker) {
    if let Some((_, p)) = &s.video {
        p.set_waker(waker.clone());
    }
    s.waker = Some(waker);
    s.watch_folder();
}

/// The app registers the recording shortcut system-wide; it starts or stops the recording.
impl Registrar for AppHandle<App> {
    type Id = ShortcutId;
    fn register(&self, shortcut: &Shortcut) -> Result<ShortcutId, ShortcutError> {
        self.register_shortcut(shortcut.clone(), |s: &mut App, _| s.toggle_recording())
    }
    fn unregister(&self, id: ShortcutId) {
        let _ = self.unregister_shortcut(id);
    }
}

/// Advances playback and moves the core's time forward (recording, processing queue).
fn poll(s: &mut App, clock: f64, tx: &mut Tx) -> bool {
    let dt = s.last.map_or(0.0, |l| (clock - l).clamp(0.0, 0.25));
    s.clock = clock;
    let mut active = false;
    let mut changed = false;

    // The shortcut field left the screen while capturing (a recording started from the menu bar
    // opens its page): the capture ends there, and the global shortcut comes back.
    if s.page != Page::Settings && s.shortcut.is_recording() {
        s.shortcut.cancel();
        s.follow_shortcut_capture();
        changed = true;
    }
    // The video only plays on its meeting's page.
    if s.video.is_some() && s.player().is_none() {
        s.video = None;
        s.playing = false;
    }
    if let Some(p) = s.player() {
        // The real video gives the position; read it again while it plays.
        let (pos, playing) = (p.time(), p.is_playing());
        if playing || s.playing != playing || (pos - s.pos).abs() > 0.01 {
            changed = true;
        }
        s.playing = playing;
        s.pos = pos;
        if playing {
            active = true;
        }
        s.current = s.meeting().and_then(|m| m.utterance_at(s.pos));
    } else if s.playing {
        active = true;
        changed = true;
        let speed = SPEEDS[s.speed];
        let dur = s.duration();
        s.pos = (s.pos + dt * speed).min(dur);
        if s.pos >= dur {
            s.playing = false;
        }
        s.current = s.meeting().and_then(|m| m.utterance_at(s.pos));
    }
    // « Écouter » on a passage to check stops at the end of its excerpt.
    if s.listen_until.is_some_and(|t| s.playing && s.pos >= t) {
        s.pause();
        changed = true;
    }
    // A correction no longer shown (other tab, other page) is abandoned.
    if s.correction.is_some() && !check::correction_shown(s) {
        s.correction = None;
        changed = true;
    }
    // The transcript follows the current utterance (playback, seek, tab change).
    if s.tab != Tab::Transcript {
        s.followed = None;
        s.transcript_frames = 0;
    } else if s.follow && (s.current != s.followed || s.transcript_frames < 3) {
        if let Some(i) = s.current {
            tx.scroll_center_in("transcript-scroll", format!("u{i}"));
        }
        s.followed = s.current;
        s.transcript_frames += 1;
        active = true;
    }

    if s.recorded() {
        s.rec_phase += dt;
    }
    // A file changed in the meetings folder: the core rereads what changed outside the app.
    if s.watch.as_ref().is_some_and(watch::FolderWatch::has_changed) {
        let events = s.core.rescan_folder();
        changed |= !events.is_empty();
        s.handle(events);
    }
    let events = s.core.advance(dt);
    if s.core.busy() || !events.is_empty() {
        active = true;
        changed = true;
    }
    s.handle(events);
    s.follow_core();

    if let Some(t) = &s.toast {
        if clock > t.until {
            s.toast = None;
            changed = true;
        } else {
            active = true;
        }
    }

    if active {
        s.last = Some(clock);
        tx.wake_after(Duration::from_millis(80));
    } else {
        s.last = None;
    }
    changed
}

fn on_key(s: &mut App, ev: &KeyEvent, tx: &mut Tx) -> bool {
    if s.fullscreen && ev.key == Key::Escape {
        s.toggle_fullscreen(tx);
        return true;
    }
    if shell::is_sidebar_toggle(ev) {
        s.sidebar.toggle();
        return true;
    }
    if ev.is_command('k') {
        if s.search.is_some() { s.search = None } else { s.open_search() }
        return true;
    }
    // The correction, inline in the page, keeps the ⌘ shortcuts from it too (⌘1 to ⌘5 would
    // abandon it).
    if s.window_open() || s.correction.is_some() || s.shortcut.is_recording() {
        return false;
    }
    // The global shortcut takes ⇧⌘R when registered; otherwise (headless, refused), the app keeps it.
    if ev.is_command('r') && ev.mods.shift && !s.record_shortcut.is_registered() {
        s.toggle_recording();
        return true;
    }
    // ⌘⌫: delete the open meeting (after confirmation).
    if ev.mods.cmd && ev.key == Key::Backspace && let Page::Meeting(id) = s.page {
        s.confirm_delete = Some(id);
        return true;
    }
    if ev.is_command('m') && s.recorded() {
        s.add_marker();
        return true;
    }
    for (i, c) in ['1', '2', '3', '4', '5'].into_iter().enumerate() {
        if ev.is_command(c) && s.meeting().is_some() {
            s.show_tab(TABS[i], tx);
            return true;
        }
    }
    if s.meeting().is_none() || ev.mods.cmd || s.text_field_open() {
        return false;
    }
    if s.tab == Tab::Doubts && check::on_key(s, ev, tx) {
        return true;
    }
    if s.tab == Tab::Transcript
        && matches!(&ev.key, Key::Text(t) if t.eq_ignore_ascii_case("c"))
        && let Some(i) = s.current
    {
        s.start_correction(check::Target::Utterance(i));
        return true;
    }
    match &ev.key {
        Key::Text(t) if t == " " => s.toggle_play(),
        Key::Left => s.seek(s.pos - 10.0),
        Key::Right => s.seek(s.pos + 10.0),
        Key::Text(t) if t == "f" => s.toggle_fullscreen(tx),
        _ => return false,
    }
    true
}

fn view(s: &App, cx: &mut Cx<App>) -> El {
    let k = Kit::neutral(cx.system_dark());
    let mut layer = Vec::new();
    let main = match s.page {
        _ if s.needs_folder => pages::welcome(s, &k),
        Page::Welcome => pages::welcome(s, &k),
        Page::Overview => pages::overview(s, &k),
        Page::Recording => pages::recording(s, &k),
        Page::Settings => pages::settings(s, &k),
        Page::Installation => pages::installation(s, &k),
        Page::Meeting(_) => match s.meeting() {
            Some(m) => meeting::page(s, m, &k, cx, &mut layer),
            None => pages::overview(s, &k),
        },
    };
    let main = pages::with_toast(s, &k, main);
    // SplitView's toolbar strip is empty: the content moves up over it while the sidebar is
    // open. Closed, the strip keeps room for the traffic lights and the sidebar button.
    let main = if s.sidebar.is_open() { main.mt(-shell::TOOLBAR_HEIGHT) } else { main };
    let shell = SplitView::new(main)
        .sidebar(&s.sidebar, |s: &mut App| &mut s.sidebar, sidebar::view(s, &k, &mut layer))
        .titlebar_inset(cx.titlebar_inset())
        .frame_inset(8.0)
        .toolbar_in_panel()
        .sidebar_padding(18.0, 10.0)
        .sidebar_toggle(SidebarToggle::new().size(ControlSize::Large).past_lights(16.0).edge(18.0).gap(4.0).in_header())
        .pane_motion(PaneMotion::new().spring(Spring::new(0.3, 0.0)))
        .reduced_motion(cx.is_reduced_motion())
        .build(&k);
    meeting::fullscreen(s, &k, cx, &mut layer);
    overlays::search(s, &k, &mut layer);
    overlays::confirm_delete(s, &k, &mut layer);
    overlays::short_recording(s, &k, &mut layer);
    title::window(s, &k, &mut layer);
    redo::window(s, &k, &mut layer);
    import::window(s, &k, &mut layer);
    import::process_window(s, &k, &mut layer);
    // Files dropped anywhere on the window open the import.
    rune_kit::root(shell, layer).on_drop_files(|s: &mut App, files, _| s.propose_import(files))
}

fn scene_meeting(s: &mut App) {
    s.open(2);
    s.seek(greffier_core::hms(0, 44, 5));
}
fn scene_transcript(s: &mut App) {
    scene_meeting(s);
    s.tab = Tab::Transcript;
}
fn scene_doubts(s: &mut App) {
    scene_meeting(s);
    s.tab = Tab::Doubts;
}
/// One passage settled (hypothesis accepted), and the next one being corrected.
fn scene_doubt_fix(s: &mut App) {
    scene_doubts(s);
    s.resolve_doubt(0, greffier_core::Resolution::AcceptHypothesis);
    if let Some(i) = s.meeting().and_then(|m| check::selected(s, m)) {
        s.start_correction(check::Target::Passage(i));
    }
}
/// Correcting an utterance in the Transcript tab.
fn scene_transcript_fix(s: &mut App) {
    scene_transcript(s);
    if let Some(i) = s.current {
        s.start_correction(check::Target::Utterance(i));
    }
}
fn scene_people(s: &mut App) {
    scene_meeting(s);
    s.tab = Tab::Speakers;
}
/// Speakers: correcting SPEAKER_01's name, open.
fn scene_people_naming(s: &mut App) {
    scene_people(s);
    s.start_naming(1);
}
/// Speakers: SPEAKER_02 merged into Jérôme, merging Pablo open.
fn scene_people_merged(s: &mut App) {
    scene_people(s);
    s.command(Command::MergeSpeakers { meeting: 2, speaker: 2, into: 0 });
    s.start_merging(4);
}
fn scene_info(s: &mut App) {
    scene_meeting(s);
    s.tab = Tab::Info;
}
/// 12 min 34 of recording (time sped up ×20), a marker at 5 min 12.
fn scene_recording(s: &mut App) {
    s.start_recording();
    s.core.advance(312.0 / demo::RECORDING_SPEED);
    s.add_marker();
    s.core.advance(442.0 / demo::RECORDING_SPEED);
    s.rec_phase = 1.3;
}
/// The recording page before starting: sources and permissions.
fn scene_record_ready(s: &mut App) {
    s.page = Page::Recording;
}
/// When a 12 s recording stops (real-time capture): offered for deletion.
fn scene_short_recording(s: &mut App) {
    let adapters = greffier_core::adapters::Adapters { capture: Box::new(greffier_core::fake::FakeCapture::new(1.0, 0.0)), ..demo::adapters() };
    s.core = Greffier::with_meetings(prototype_folder(), adapters, demo::meetings());
    s.start_recording();
    s.core.advance(12.0);
    s.stop_recording();
}
/// Two short recordings stopped one after the other, the first one kept: the question is now about
/// the second (8 s).
fn scene_short_recordings(s: &mut App) {
    scene_short_recording(s);
    s.start_recording();
    s.core.advance(8.0);
    s.stop_recording();
    s.keep_short_recording();
}
/// The meeting to process, transcribed, 40 % into the review.
fn scene_processing(s: &mut App) {
    s.open(1);
    s.process(1);
    s.core.advance(0.0);
    s.core.advance(demo::simulated_duration(Stage::Transcription));
    s.core.advance(0.4 * demo::simulated_duration(Stage::Review));
}
/// The queue: today's meeting in transcription, the release review waiting behind it.
fn scene_queue(s: &mut App) {
    let second = s.core.meetings()[1].id;
    s.process(1);
    s.core.advance(0.0);
    s.core.advance(0.5 * demo::simulated_duration(Stage::Transcription));
    s.command(Command::Redo { meeting: second, stage: Stage::Summary });
    s.open(second);
}
/// Transcription failed: the message and « Réessayer cette étape ».
fn scene_failed(s: &mut App) {
    let transcriber = greffier_core::fake::FakeTranscriber::new(1.0, |_| Err("Le média n'a pas de son.".into()));
    let adapters = greffier_core::adapters::Adapters { transcriber: Box::new(transcriber), ..demo::adapters() };
    s.core = Greffier::with_meetings(prototype_folder(), adapters, demo::meetings());
    s.open(1);
    s.process(1);
    s.core.advance(0.0);
    s.core.advance(1.0);
}
/// The Transcript tab's « Refaire… » menu.
fn scene_redo(s: &mut App) {
    scene_transcript(s);
    s.redo_menu = true;
}
/// « Refaire la transcription… »: the speaker count before running it again.
fn scene_redo_transcription(s: &mut App) {
    scene_transcript(s);
    s.start_redo_transcription(2);
    if let Some(r) = &mut s.redo_transcription {
        r.number = Some(4);
    }
}
fn scene_unprocessed(s: &mut App) {
    s.open(1);
}
fn scene_settings(s: &mut App) {
    s.page = Page::Settings;
}
/// The « Changer le titre » window on the open meeting.
fn scene_rename(s: &mut App) {
    scene_meeting(s);
    s.start_rename(2);
}
/// The settings with two context folders open to the agent.
fn scene_context(s: &mut App) {
    let folders = ["/Users/odile/Documents/Notes", "/Users/odile/Projets/Atelier"].map(PathBuf::from).to_vec();
    s.command(Command::ChangeSettings(greffier_core::Settings { context_folders: folders, ..s.core.settings().clone() }));
    s.page = Page::Settings;
}
/// First launch: a download, an installation, a wrong sha256, `claude` missing.
fn scene_installation(s: &mut App) {
    let adapters = greffier_core::adapters::Adapters { resources: Box::new(FakeResources::fixed(demo::installing_resources())), ..demo::adapters() };
    s.core = Greffier::with_meetings(prototype_folder(), adapters, demo::meetings());
    s.page = Page::Installation;
}
fn scene_search(s: &mut App) {
    s.open_search();
    if let Some(e) = &mut s.search {
        *e = TextEdit::new("bookmarks");
        e.focused = true;
    }
}
fn scene_menu(s: &mut App) {
    s.menu = Some((2, PointerPress { x: 180.0, y: 520.0 }));
}
fn scene_fullscreen(s: &mut App) {
    scene_meeting(s);
    s.fullscreen = true;
}
fn scene_delete(s: &mut App) {
    s.confirm_delete = Some(2);
}
fn scene_welcome(s: &mut App) {
    s.core = Greffier::with_meetings(prototype_folder(), demo::adapters(), vec![]);
    s.page = Page::Welcome;
    s.needs_folder = true;
}
/// The core tests' sample meetings (a copy), the complete meeting open at 0:00:05.
fn scene_folder(s: &mut App) {
    let dir = prototype_folder();
    copy_dir(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../core/tests/fixtures/meetings-folder"), &dir);
    s.core = Greffier::open(&dir, demo::adapters());
    s.folder = Some(dir);
    if let Some(id) = s.core.meetings().first().map(|m| m.id) {
        s.open_at(id, 5.0);
    }
}
fn copy_dir(from: &Path, to: &Path) {
    let _ = std::fs::create_dir_all(to);
    for e in std::fs::read_dir(from).into_iter().flatten().flatten() {
        let target = to.join(e.file_name());
        if e.file_type().is_ok_and(|t| t.is_dir()) {
            copy_dir(&e.path(), &target);
        } else {
            let _ = std::fs::copy(e.path(), target);
        }
    }
}
/// The sample meetings: the one that only has its recording.
fn scene_video_only(s: &mut App) {
    scene_folder(s);
    if let Some(id) = s.core.meetings().get(1).map(|m| m.id) {
        s.open(id);
    }
}

/// The import window on a sample meeting's video and transcription.
fn scene_import(s: &mut App) {
    scene_folder(s);
    s.page = Page::Overview;
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../core/tests/fixtures/meetings-folder/2026-11-18 09h15 - Réunion");
    let dir = std::env::temp_dir().join("greffier-prototype-import");
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::create_dir_all(&dir);
    let media = dir.join("Point banc de test.mp4");
    let transcription = dir.join("Point banc de test.json");
    let _ = std::fs::copy(fixture.join("2026-11-18 09h15 - Réunion - enregistrement.mp4"), &media);
    let _ = std::fs::copy(fixture.join("2026-11-18 09h15 - Réunion - transcription.json"), &transcription);
    s.propose_import(&[media, transcription]);
    // The copied file's date changes at each render: pin it.
    if let Some(d) = &mut s.import {
        d.date = TextEdit::new("2026-11-18");
        d.hour = TextEdit::new("09:15");
    }
}
/// After the import: the now-or-later question.
fn scene_imported(s: &mut App) {
    scene_import(s);
    s.confirm_import();
}

fn main() {
    let chrome = Chrome { full_size_content: true, traffic_lights: Some((16.0, 25.0)), drag_regions: true };
    let spec = AppSpec {
        on_key: Some(on_key),
        poll: Some(poll),
        setup: Some(setup),
        min_size: Some((980.0, 640.0)),
        scenes: &[
            ("meeting", scene_meeting),
            ("transcript", scene_transcript),
            ("doubts", scene_doubts),
            ("doubt-fix", scene_doubt_fix),
            ("transcript-fix", scene_transcript_fix),
            ("people", scene_people),
            ("people-naming", scene_people_naming),
            ("people-merged", scene_people_merged),
            ("info", scene_info),
            ("recording", scene_recording),
            ("record-ready", scene_record_ready),
            ("short-recording", scene_short_recording),
            ("short-recordings", scene_short_recordings),
            ("processing", scene_processing),
            ("queue", scene_queue),
            ("failed", scene_failed),
            ("unprocessed", scene_unprocessed),
            ("redo", scene_redo),
            ("redo-transcription", scene_redo_transcription),
            ("settings", scene_settings),
            ("context", scene_context),
            ("rename", scene_rename),
            ("installation", scene_installation),
            ("search", scene_search),
            ("menu", scene_menu),
            ("delete", scene_delete),
            ("fullscreen", scene_fullscreen),
            ("welcome", scene_welcome),
            ("folder", scene_folder),
            ("video-only", scene_video_only),
            ("import", scene_import),
            ("imported", scene_imported),
        ],
        ..AppSpec::new("Greffier", (1360.0, 860.0), init, view)
    };
    let import = rune::MenuItem::new("Importer…", |s: &mut App, tx: &mut rune::Tx| s.pick_import(tx)).id("import").menu("Fichier").key_equivalent("cmd+i".parse().expect("shortcut"));
    // Closing the window keeps the app (and the menu bar icon) running: ⌘Q quits it.
    let app = rune::App::new(spec).chrome(chrome).menu_item(import).keep_running();
    let handle = app.handle();
    let shared = handle.clone();
    let shared_for_clicks = handle.clone();
    handle.update(move |s, _| {
        s.handle = Some(shared);
        s.register_record_shortcut();
        if !rune::is_headless() {
            s.install_menu_bar();
            // A click on « Bilan prêt » opens the meeting, even if the window was closed.
            let h = shared_for_clicks.clone();
            rune_notify::on_click(move |click| {
                let Some(name) = click.get("folder").map(str::to_owned) else { return };
                h.open();
                h.update(move |s: &mut App, _| {
                    s.open_folder_named(&name);
                    rune_notify::bring_to_front();
                });
            });
        }
    });
    app.run();
}

