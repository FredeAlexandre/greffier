//! Greffier — prototype d'interface (Rune). Données fictives, traitements simulés.
//!
//! `cargo run` ouvre la fenêtre. `cargo run -- --snap out.png --scene meeting` rend une scène
//! sans fenêtre (`--list-scenes` pour la liste).

#![forbid(unsafe_code)]

mod bilan;
mod data;
mod meeting;
mod overlays;
mod pages;
mod sidebar;
mod timeline;
mod ui;

use std::time::Duration;

use rune::{AppSpec, Chrome, Cx, El, Key, KeyEvent, PointerPress, Spring, TextEdit, Tx};
use rune_kit::shell::{self, PaneMotion, PaneState, SidebarToggle, SplitView};
use rune_kit::{ControlSize, Kit};

use data::{DoubtStatus, Meeting, Stage};

/// Le temps de l'enregistrement va 20 fois plus vite que l'horloge, pour la démo.
pub const REC_SPEED: f64 = 20.0;
pub const SPEEDS: [f64; 3] = [1.0, 1.5, 2.0];

#[derive(Clone, Copy, PartialEq)]
pub enum Page {
    Overview,
    Recording,
    Meeting(u32),
    Settings,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Tab {
    Summary,
    Transcript,
    Doubts,
    People,
    Info,
}

pub const TABS: [Tab; 5] = [Tab::Summary, Tab::Transcript, Tab::Doubts, Tab::People, Tab::Info];

pub struct Recording {
    pub elapsed: f64,
    pub markers: Vec<f64>,
    pub people: Option<u8>,
    pub phase: f64,
}

pub struct Toast {
    pub text: String,
    pub meeting: Option<u32>,
    pub until: f64,
}

pub struct App {
    pub sidebar: PaneState,
    pub meetings: Vec<Meeting>,
    pub page: Page,
    pub tab: Tab,
    pub pos: f64,
    pub playing: bool,
    pub speed: usize,
    pub follow: bool,
    pub current: Option<usize>,
    pub followed: Option<usize>,
    /// Images depuis l'ouverture de l'onglet Transcription (la liste doit exister pour défiler).
    pub transcript_frames: u32,
    pub rec: Option<Recording>,
    pub picking: Option<usize>,
    pub toast: Option<Toast>,
    pub auto_process: bool,
    pub sources: [bool; 3],
    pub clock: f64,
    pub last: Option<f64>,
    pub next_id: u32,
    pub collapsed: [bool; 4],
    /// Recherche ⌘K : ouverte quand `Some`.
    pub search: Option<TextEdit>,
    /// Menu du clic droit sur une réunion de la barre latérale.
    pub menu: Option<(u32, PointerPress)>,
    pub confirm_delete: Option<u32>,
    /// Vidéo en plein écran (la fenêtre passe aussi en plein écran macOS).
    pub fullscreen: bool,
}

impl App {
    pub fn meeting(&self) -> Option<&Meeting> {
        match self.page {
            Page::Meeting(id) => self.meetings.iter().find(|m| m.id == id),
            _ => None,
        }
    }
    pub fn meeting_mut(&mut self) -> Option<&mut Meeting> {
        match self.page {
            Page::Meeting(id) => self.meetings.iter_mut().find(|m| m.id == id),
            _ => None,
        }
    }
    pub fn by_id(&mut self, id: u32) -> Option<&mut Meeting> {
        self.meetings.iter_mut().find(|m| m.id == id)
    }
    pub fn open(&mut self, id: u32) {
        if self.page != Page::Meeting(id) {
            self.page = Page::Meeting(id);
            self.pos = 0.0;
            self.playing = false;
            self.current = None;
            self.followed = None;
            self.picking = None;
            self.tab = Tab::Summary;
            self.fullscreen = false;
        }
        if let Some(m) = self.by_id(id) {
            m.unread = false;
        }
    }
    pub fn open_at(&mut self, id: u32, t: f64) {
        self.open(id);
        self.seek(t);
    }
    pub fn seek(&mut self, t: f64) {
        let d = self.meeting().map_or(0.0, |m| m.duration);
        self.pos = t.clamp(0.0, d);
        self.current = self.meeting().and_then(|m| m.utterance_at(self.pos));
    }
    pub fn seek_and_play(&mut self, t: f64) {
        self.seek(t);
        self.playing = true;
    }
    pub fn toggle_play(&mut self) {
        if self.meeting().is_some_and(|m| m.stage == Stage::Done) {
            self.playing = !self.playing;
        }
    }
    pub fn start_recording(&mut self) {
        if self.rec.is_none() {
            self.rec = Some(Recording { elapsed: 0.0, markers: vec![], people: None, phase: 0.0 });
        }
        self.playing = false;
        self.page = Page::Recording;
    }
    pub fn stop_recording(&mut self) {
        let Some(rec) = self.rec.take() else { return };
        let id = self.next_id;
        self.next_id += 1;
        let mut m = data::unprocessed(id, 0, "sam. 21/11", "14h02", rec.elapsed.max(60.0));
        m.people = rec.people;
        m.markers = rec.markers.iter().enumerate().map(|(i, t)| data::Marker { at: *t, label: format!("Repère {}", i + 1) }).collect();
        if self.auto_process {
            m.stage = Stage::Queued;
        }
        self.meetings.insert(0, m);
        self.open(id);
    }
    pub fn process(&mut self, id: u32) {
        if let Some(m) = self.by_id(id) {
            m.stage = Stage::Queued;
            m.progress = 0.0;
        }
    }
    pub fn redo_bilan(&mut self, id: u32) {
        if let Some(m) = self.by_id(id) {
            m.stage = Stage::Summarizing;
            m.progress = 0.0;
        }
    }
    pub fn stop_processing(&mut self, id: u32) {
        if let Some(m) = self.by_id(id) {
            m.stage = Stage::Recorded;
            m.progress = 0.0;
        }
    }
    pub fn delete(&mut self, id: u32) {
        self.meetings.retain(|m| m.id != id);
        if self.page == Page::Meeting(id) {
            self.page = Page::Overview;
            self.playing = false;
        }
        self.confirm_delete = None;
    }
    pub fn add_marker(&mut self) {
        if let Some(r) = &mut self.rec {
            r.markers.push(r.elapsed);
        }
    }
    pub fn resolve_doubt(&mut self, i: usize, status: DoubtStatus) {
        if let Some(m) = self.meeting_mut() {
            m.doubts[i].status = status;
        }
    }
    pub fn toggle_fullscreen(&mut self, tx: &mut Tx) {
        self.fullscreen = !self.fullscreen && self.meeting().is_some_and(|m| m.stage == Stage::Done);
        tx.toggle_full_screen();
    }
    pub fn open_search(&mut self) {
        let mut edit = TextEdit::new("");
        edit.focused = true;
        self.search = Some(edit);
        self.menu = None;
    }
}

fn init() -> App {
    App {
        sidebar: PaneState::new(272.0).range(200.0, 420.0),
        meetings: data::sample(),
        page: Page::Overview,
        tab: Tab::Summary,
        pos: 0.0,
        playing: false,
        speed: 0,
        follow: true,
        current: None,
        followed: None,
        transcript_frames: 0,
        rec: None,
        picking: None,
        toast: None,
        auto_process: true,
        sources: [true, true, true],
        clock: 0.0,
        last: None,
        next_id: 100,
        collapsed: [false; 4],
        search: None,
        menu: None,
        confirm_delete: None,
        fullscreen: false,
    }
}

/// Avance la lecture, l'enregistrement et les traitements simulés.
fn poll(s: &mut App, clock: f64, tx: &mut Tx) -> bool {
    let dt = s.last.map_or(0.0, |l| (clock - l).clamp(0.0, 0.25));
    s.clock = clock;
    let mut active = false;
    let mut changed = false;

    if s.playing {
        active = true;
        changed = true;
        let speed = SPEEDS[s.speed];
        let dur = s.meeting().map_or(0.0, |m| m.duration);
        s.pos = (s.pos + dt * speed).min(dur);
        if s.pos >= dur {
            s.playing = false;
        }
        s.current = s.meeting().and_then(|m| m.utterance_at(s.pos));
    }
    // La transcription suit la réplique en cours (lecture, saut, changement d'onglet).
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

    if let Some(r) = &mut s.rec {
        active = true;
        changed = true;
        r.elapsed += dt * REC_SPEED;
        r.phase += dt;
    }

    // Une réunion à la fois : la première en attente démarre quand rien ne tourne.
    if !s.meetings.iter().any(|m| m.stage.processing()) {
        if let Some(m) = s.meetings.iter_mut().rev().find(|m| m.stage == Stage::Queued) {
            m.stage = Stage::Transcribing;
            m.progress = 0.0;
            changed = true;
        }
    }
    let mut finished = None;
    if let Some(m) = s.meetings.iter_mut().find(|m| m.stage.processing()) {
        active = true;
        changed = true;
        m.progress += dt / m.stage.sim_secs();
        if m.progress >= 1.0 {
            m.progress = 0.0;
            m.stage = m.stage.next();
            if m.stage == Stage::Done {
                finished = Some(m.id);
            }
        }
    }
    if let Some(id) = finished {
        if let Some(i) = s.meetings.iter().position(|m| m.id == id) {
            let old = &s.meetings[i];
            // Un bilan refait garde sa réunion ; une nouvelle reçoit le contenu simulé.
            if old.utterances.is_empty() {
                let mut done = data::generic(id, old.ago, &old.date_label.clone(), &old.start.clone(), old.duration, &data::NEW_MEETING, u64::from(id));
                done.markers = std::mem::take(&mut s.meetings[i].markers);
                done.people = s.meetings[i].people;
                for sp in &mut done.speakers {
                    sp.name = None;
                }
                s.meetings[i] = done;
            }
            let m = &mut s.meetings[i];
            m.unread = s.page != Page::Meeting(id);
            s.toast = Some(Toast { text: format!("Bilan prêt : {}", m.title), meeting: Some(id), until: clock + 6.0 });
        }
    }

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
    if s.search.is_some() || s.confirm_delete.is_some() {
        return false;
    }
    if ev.is_command('r') && ev.mods.shift {
        if s.rec.is_some() { s.stop_recording() } else { s.start_recording() }
        return true;
    }
    if ev.is_command('m') && s.rec.is_some() {
        s.add_marker();
        return true;
    }
    for (i, c) in ['1', '2', '3', '4', '5'].into_iter().enumerate() {
        if ev.is_command(c) && s.meeting().is_some() {
            s.tab = TABS[i];
            return true;
        }
    }
    if s.meeting().is_none() || ev.mods.cmd {
        return false;
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
        Page::Overview => pages::overview(s, &k),
        Page::Recording => pages::recording(s, &k),
        Page::Settings => pages::settings(s, &k),
        Page::Meeting(_) => match s.meeting() {
            Some(m) => meeting::page(s, m, &k, cx),
            None => pages::overview(s, &k),
        },
    };
    let main = pages::with_toast(s, &k, main);
    // La bande d'outils de SplitView est vide : le contenu remonte par-dessus tant que la barre
    // latérale est ouverte. Fermée, la bande garde la place des feux et du bouton de la barre.
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
    rune_kit::root(shell, layer)
}

fn scene_meeting(s: &mut App) {
    s.open(2);
    s.seek(data::hms(0, 44, 5));
}
fn scene_transcript(s: &mut App) {
    scene_meeting(s);
    s.tab = Tab::Transcript;
}
fn scene_doubts(s: &mut App) {
    scene_meeting(s);
    s.tab = Tab::Doubts;
}
fn scene_people(s: &mut App) {
    scene_meeting(s);
    s.tab = Tab::People;
}
fn scene_info(s: &mut App) {
    scene_meeting(s);
    s.tab = Tab::Info;
}
fn scene_recording(s: &mut App) {
    s.start_recording();
    if let Some(r) = &mut s.rec {
        r.elapsed = 754.0;
        r.markers = vec![312.0];
        r.phase = 1.3;
    }
}
fn scene_processing(s: &mut App) {
    s.open(1);
    if let Some(m) = s.by_id(1) {
        m.stage = Stage::Reviewing;
        m.progress = 0.4;
    }
}
fn scene_unprocessed(s: &mut App) {
    s.open(1);
}
fn scene_settings(s: &mut App) {
    s.page = Page::Settings;
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

fn main() {
    let chrome = Chrome { full_size_content: true, traffic_lights: Some((16.0, 25.0)), drag_regions: true };
    let spec = AppSpec {
        on_key: Some(on_key),
        poll: Some(poll),
        min_size: Some((980.0, 640.0)),
        scenes: &[
            ("meeting", scene_meeting),
            ("transcript", scene_transcript),
            ("doubts", scene_doubts),
            ("people", scene_people),
            ("info", scene_info),
            ("recording", scene_recording),
            ("processing", scene_processing),
            ("unprocessed", scene_unprocessed),
            ("settings", scene_settings),
            ("search", scene_search),
            ("menu", scene_menu),
            ("delete", scene_delete),
            ("fullscreen", scene_fullscreen),
        ],
        ..AppSpec::new("Greffier", (1360.0, 860.0), init, view)
    };
    rune::App::new(spec).chrome(chrome).run();
}
