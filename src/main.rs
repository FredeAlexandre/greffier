//! Greffier — l'interface (Rune). Elle lit l'état du cœur (`greffier_coeur`) et lui envoie des
//! commandes ; le cœur tourne ici sur les données fictives et les faux adaptateurs du prototype.
//!
//! `cargo run` ouvre la fenêtre. `cargo run -- --snap out.png --scene meeting` rend une scène
//! sans fenêtre (`--list-scenes` pour la liste).

#![forbid(unsafe_code)]

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

use greffier_coeur::{Commande, Etape, Evenement, Greffier, IdReunion, Reunion, Statut, Tranche, fictif};

pub const SPEEDS: [f64; 3] = [1.0, 1.5, 2.0];

#[derive(Clone, Copy, PartialEq)]
pub enum Page {
    Overview,
    Recording,
    Meeting(IdReunion),
    Settings,
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
    pub meeting: Option<IdReunion>,
    pub until: f64,
}

pub struct App {
    /// Le cœur : réunions, enregistrement, file de traitement. On ne le change que par commandes.
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
    /// Images depuis l'ouverture de l'onglet Transcription (la liste doit exister pour défiler).
    pub transcript_frames: u32,
    /// Phase de l'animation de l'enregistrement (pulsation, niveaux).
    pub rec_phase: f64,
    pub picking: Option<usize>,
    pub toast: Option<Toast>,
    pub clock: f64,
    pub last: Option<f64>,
    pub collapsed: [bool; 4],
    /// Recherche ⌘K : ouverte quand `Some`.
    pub search: Option<TextEdit>,
    /// Menu du clic droit sur une réunion de la barre latérale.
    pub menu: Option<(IdReunion, PointerPress)>,
    pub confirm_delete: Option<IdReunion>,
    /// Vidéo en plein écran (la fenêtre passe aussi en plein écran macOS).
    pub fullscreen: bool,
}

impl App {
    pub fn meeting(&self) -> Option<&Reunion> {
        match self.page {
            Page::Meeting(id) => self.core.reunion(id),
            _ => None,
        }
    }
    /// Envoie une commande au cœur et donne suite à ses événements.
    pub fn command(&mut self, c: Commande) {
        match self.core.executer(c) {
            Ok(events) => self.handle(events),
            Err(refus) => eprintln!("greffier : {refus}"),
        }
    }
    fn handle(&mut self, events: Vec<Evenement>) {
        for e in events {
            match e {
                Evenement::ReunionEnregistree(id) => self.open(id),
                Evenement::BilanPret { reunion, titre } => {
                    if self.page == Page::Meeting(reunion) {
                        self.command(Commande::MarquerLue(reunion));
                    }
                    self.toast = Some(Toast { text: format!("Bilan prêt : {titre}"), meeting: Some(reunion), until: self.clock + 6.0 });
                }
                Evenement::EtapeEnEchec { message, .. } => eprintln!("greffier : {message}"),
                _ => {}
            }
        }
    }
    pub fn open(&mut self, id: IdReunion) {
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
        if self.core.reunion(id).is_some_and(|m| m.nouvelle) {
            self.command(Commande::MarquerLue(id));
        }
    }
    pub fn open_at(&mut self, id: IdReunion, t: f64) {
        self.open(id);
        self.seek(t);
    }
    pub fn seek(&mut self, t: f64) {
        let d = self.meeting().map_or(0.0, |m| m.duree);
        self.pos = t.clamp(0.0, d);
        self.current = self.meeting().and_then(|m| m.replique_a(self.pos));
    }
    pub fn seek_and_play(&mut self, t: f64) {
        self.seek(t);
        self.playing = true;
    }
    pub fn toggle_play(&mut self) {
        if self.meeting().is_some_and(|m| m.statut == Statut::Terminee) {
            self.playing = !self.playing;
        }
    }
    pub fn recording(&self) -> bool {
        self.core.enregistrement().is_some()
    }
    pub fn start_recording(&mut self) {
        if !self.recording() {
            self.rec_phase = 0.0;
            self.command(Commande::DemarrerEnregistrement);
        }
        self.playing = false;
        self.page = Page::Recording;
    }
    pub fn stop_recording(&mut self) {
        if self.recording() {
            self.command(Commande::ArreterEnregistrement);
        }
    }
    pub fn process(&mut self, id: IdReunion) {
        self.command(Commande::Traiter(id));
    }
    pub fn redo_bilan(&mut self, id: IdReunion) {
        self.command(Commande::Refaire { reunion: id, etape: Etape::Bilan });
    }
    pub fn stop_processing(&mut self, id: IdReunion) {
        self.command(Commande::ArreterTraitement(id));
    }
    pub fn delete(&mut self, id: IdReunion) {
        self.command(Commande::Supprimer(id));
        if self.page == Page::Meeting(id) {
            self.page = Page::Overview;
            self.playing = false;
        }
        self.confirm_delete = None;
    }
    pub fn add_marker(&mut self) {
        if self.recording() {
            self.command(Commande::PoserRepere);
        }
    }
    pub fn resolve_doubt(&mut self, passage: usize, tranche: Tranche) {
        if let Page::Meeting(reunion) = self.page {
            self.command(Commande::Trancher { reunion, passage, tranche });
        }
    }
    /// Écran, son du Mac, micro.
    pub fn source(&self, i: usize) -> bool {
        let s = &self.core.reglages().sources;
        [s.ecran, s.son_du_mac, s.micro][i]
    }
    pub fn toggle_source(&mut self, i: usize) {
        let mut r = self.core.reglages().clone();
        let s = &mut r.sources;
        let v = [&mut s.ecran, &mut s.son_du_mac, &mut s.micro];
        *v[i] = !*v[i];
        self.command(Commande::ChangerReglages(r));
    }
    pub fn toggle_auto_process(&mut self) {
        let mut r = self.core.reglages().clone();
        r.traiter_a_l_arret = !r.traiter_a_l_arret;
        self.command(Commande::ChangerReglages(r));
    }
    pub fn toggle_fullscreen(&mut self, tx: &mut Tx) {
        self.fullscreen = !self.fullscreen && self.meeting().is_some_and(|m| m.statut == Statut::Terminee);
        tx.toggle_full_screen();
    }
    pub fn open_search(&mut self) {
        let mut edit = TextEdit::new("");
        edit.focused = true;
        self.search = Some(edit);
        self.menu = None;
    }
}

/// Le dossier de réunions du prototype : un répertoire temporaire, vidé à chaque lancement.
fn prototype_folder() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join("greffier-prototype");
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn init() -> App {
    App {
        core: Greffier::avec_reunions(prototype_folder(), fictif::adaptateurs(), fictif::reunions()),
        sidebar: PaneState::new(272.0).range(200.0, 420.0),
        page: Page::Overview,
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
        collapsed: [false; 4],
        search: None,
        menu: None,
        confirm_delete: None,
        fullscreen: false,
    }
}

/// Avance la lecture et fait passer le temps du cœur (enregistrement, file de traitement).
fn poll(s: &mut App, clock: f64, tx: &mut Tx) -> bool {
    let dt = s.last.map_or(0.0, |l| (clock - l).clamp(0.0, 0.25));
    s.clock = clock;
    let mut active = false;
    let mut changed = false;

    if s.playing {
        active = true;
        changed = true;
        let speed = SPEEDS[s.speed];
        let dur = s.meeting().map_or(0.0, |m| m.duree);
        s.pos = (s.pos + dt * speed).min(dur);
        if s.pos >= dur {
            s.playing = false;
        }
        s.current = s.meeting().and_then(|m| m.replique_a(s.pos));
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

    if s.recording() {
        s.rec_phase += dt;
    }
    let events = s.core.avancer(dt);
    if s.core.occupe() || !events.is_empty() {
        active = true;
        changed = true;
    }
    s.handle(events);

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
        if s.recording() { s.stop_recording() } else { s.start_recording() }
        return true;
    }
    if ev.is_command('m') && s.recording() {
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
    s.seek(greffier_coeur::hms(0, 44, 5));
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
    s.tab = Tab::Speakers;
}
fn scene_info(s: &mut App) {
    scene_meeting(s);
    s.tab = Tab::Info;
}
/// 12 min 34 d'enregistrement (temps accéléré ×20), un repère à 5 min 12.
fn scene_recording(s: &mut App) {
    s.start_recording();
    s.core.avancer(312.0 / fictif::VITESSE_D_ENREGISTREMENT);
    s.add_marker();
    s.core.avancer(442.0 / fictif::VITESSE_D_ENREGISTREMENT);
    s.rec_phase = 1.3;
}
/// La réunion à traiter, transcrite, à 40 % de la relecture.
fn scene_processing(s: &mut App) {
    s.open(1);
    s.process(1);
    s.core.avancer(0.0);
    s.core.avancer(fictif::duree_simulee(Etape::Transcription));
    s.core.avancer(0.4 * fictif::duree_simulee(Etape::Relecture));
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
