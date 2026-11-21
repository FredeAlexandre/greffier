//! Greffier — l'interface (Rune). Elle lit l'état du cœur (`greffier_coeur`) et lui envoie des
//! commandes. Le cœur lit les réunions du dossier de réunions choisi au premier lancement et les
//! traite avec les vrais adaptateurs (whisper.cpp, la CLI `claude`…). Les ressources (modèles,
//! Python privé, CLI `claude`) sont dans `~/Library/Application Support/Greffier`.
//!
//! `cargo run` ouvre la fenêtre. `cargo run -- --snap out.png --scene meeting` rend une scène
//! sans fenêtre (`--list-scenes` pour la liste) : sans fenêtre, l'app part des données fictives
//! du prototype, jamais du dossier de réunions. `GREFFIER_DOSSIER=… cargo run` ouvre un autre
//! dossier de réunions le temps d'un lancement.

#![forbid(unsafe_code)]

mod import;
mod meeting;
mod overlays;
mod pages;
mod refaire;
mod reglages;
mod sidebar;
mod surveillance;
mod timeline;
mod titre;
mod ui;
mod verifier;

use std::path::{Path, PathBuf};
use std::time::Duration;

use rune::{AppHandle, AppSpec, Chrome, Cx, El, FileDialogOptions, Key, KeyEvent, Player, PointerPress, Shortcut, ShortcutError, ShortcutId, Spring, TextEdit, Tx, Waker};
use rune_kit::shell::{self, PaneMotion, PaneState, SidebarToggle, SplitView};
use rune_kit::{ControlSize, Kit, ShortcutRecorderState};

use greffier_barre::{Choix, IconeDeLaBarre};
use greffier_capture::veille::PasDeVeille;
use greffier_coeur::adaptateurs::EtatDeRessource;
use greffier_coeur::faux::FaussesRessources;
use greffier_coeur::{Commande, Etape, Evenement, Greffier, IdReunion, Refus, Reunion, Role, fictif, lexique};

pub const SPEEDS: [f64; 3] = [1.0, 1.5, 2.0];

#[derive(Clone, Copy, PartialEq)]
pub enum Page {
    /// Premier lancement : choisir le dossier de réunions.
    Welcome,
    Overview,
    Recording,
    Meeting(IdReunion),
    Settings,
    /// Le premier lancement : télécharger et vérifier les ressources.
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
    pub collapsed: [bool; 5],
    /// Recherche ⌘K : ouverte quand `Some`.
    pub search: Option<TextEdit>,
    /// Menu du clic droit sur une réunion de la barre latérale.
    pub menu: Option<(IdReunion, PointerPress)>,
    pub confirm_delete: Option<IdReunion>,
    /// Vidéo en plein écran (la fenêtre passe aussi en plein écran macOS).
    pub fullscreen: bool,
    /// L'enregistrement de la réunion ouverte, quand son dossier en a un. Sans lui (données
    /// fictives), la lecture est simulée.
    pub video: Option<(IdReunion, Player)>,
    /// Réveille l'interface quand la vidéo charge, finit ou échoue.
    pub waker: Option<Waker>,
    /// Le dossier de réunions choisi (`None` pour les données fictives du prototype).
    pub folder: Option<PathBuf>,
    /// Premier lancement : rien d'autre que le choix du dossier de réunions tant qu'il n'est
    /// pas fait (aucun enregistrement ne doit partir dans un dossier non choisi).
    pub needs_folder: bool,
    /// Le dernier problème avec le dossier de réunions, affiché dans les réglages.
    pub folder_error: Option<String>,
    /// Pour (dé)poser les raccourcis globaux une fois l'app lancée.
    pub handle: Option<AppHandle<App>>,
    /// Le raccourci global qui démarre et arrête l'enregistrement (réglable).
    pub shortcut: ShortcutRecorderState,
    pub shortcut_id: Option<ShortcutId>,
    /// ⌘M pose un repère de n'importe où, le temps de l'enregistrement.
    pub marker_id: Option<ShortcutId>,
    /// Pourquoi l'enregistrement n'a pas démarré (autorisation, source…).
    pub record_error: Option<String>,
    /// Un enregistrement de moins d'une minute, proposé à la suppression.
    pub short_recording: Option<IdReunion>,
    /// Ce que compte le lexique, pour les réglages.
    pub lexique: lexique::Resume,
    /// Signale les changements faits dans le dossier de réunions hors de l'app.
    pub watch: Option<surveillance::Surveillance>,
    /// La fenêtre d'import, ouverte quand `Some`.
    pub import: Option<import::Draft>,
    /// La réunion qui vient d'être importée : on demande quand lancer son traitement.
    pub process_imported: Option<IdReunion>,
    /// L'icône de la barre des menus (fenêtre seulement, posée au lancement).
    pub menu_bar: Option<IconeDeLaBarre>,
    /// La fenêtre « Changer le titre », ouverte quand `Some`.
    pub rename: Option<titre::Renaming>,
    /// La fenêtre « Refaire la transcription », ouverte quand `Some`.
    pub redo_transcription: Option<refaire::RedoTranscription>,
    /// Le menu « Refaire… » de l'onglet Transcription, ouvert.
    pub redo_menu: bool,
    /// L'assertion de non-veille, tenue pendant un enregistrement ou un traitement (fenêtre
    /// seulement).
    pub awake: Option<PasDeVeille>,
    /// Le passage à vérifier sélectionné au clavier (voir `verifier::selected`).
    pub doubt: usize,
    /// La correction à la main en cours (passage à vérifier ou réplique).
    pub correction: Option<verifier::Correction>,
    /// « Écouter » un passage : la lecture s'arrête à la fin de son extrait.
    pub listen_until: Option<f64>,
}

impl App {
    pub fn meeting(&self) -> Option<&Reunion> {
        match self.page {
            Page::Meeting(id) => self.core.reunion(id),
            _ => None,
        }
    }
    /// La vidéo de la réunion ouverte.
    pub fn player(&self) -> Option<&Player> {
        match (self.page, &self.video) {
            (Page::Meeting(id), Some((vid, p))) if id == *vid => Some(p),
            _ => None,
        }
    }
    /// La durée de la réunion ouverte : celle de ses fichiers, sinon celle de la vidéo.
    pub fn duration(&self) -> f64 {
        let d = self.meeting().map_or(0.0, |m| m.duree);
        if d > 0.0 { d } else { self.player().and_then(Player::duration).unwrap_or(0.0) }
    }
    /// Envoie une commande au cœur et donne suite à ses événements.
    pub fn command(&mut self, c: Commande) {
        if let Err(refus) = self.try_command(c) {
            eprintln!("greffier : {refus}");
        }
    }
    fn try_command(&mut self, c: Commande) -> Result<(), Refus> {
        let events = self.core.executer(c);
        self.follow_core();
        self.handle(events?);
        Ok(())
    }
    fn handle(&mut self, events: Vec<Evenement>) {
        for e in events {
            match e {
                Evenement::ReunionEnregistree(id) => self.open(id),
                Evenement::EnregistrementInterrompu { reunion, message } => {
                    eprintln!("greffier : {message}");
                    self.register_marker_shortcut(false);
                    self.open(reunion);
                    self.toast = Some(Toast { text: "Enregistrement interrompu : ce qui a été capturé est gardé.".into(), meeting: Some(reunion), until: self.clock + 8.0 });
                    if let Some(h) = &self.handle {
                        h.open();
                    }
                }
                Evenement::SuppressionProposee(id) => self.short_recording = Some(id),
                Evenement::ReunionImportee(id) => self.imported(id),
                Evenement::BilanPret { reunion, titre } => {
                    if self.page == Page::Meeting(reunion) {
                        self.command(Commande::MarquerLue(reunion));
                    }
                    self.toast = Some(Toast { text: format!("Bilan prêt : {titre}"), meeting: Some(reunion), until: self.clock + 6.0 });
                    self.notify_bilan(reunion);
                }
                Evenement::EtapeTerminee { etape: Etape::Relecture, .. } if !rune::is_headless() => {
                    // La relecture a pu enrichir le lexique.
                    self.lexique = reglages::lexique();
                }
                Evenement::EtapeEnEchec { message, .. } | Evenement::EcritureImpossible { message, .. } => eprintln!("greffier : {message}"),
                Evenement::RessourcesInstallees => {
                    self.toast = Some(Toast { text: "Ressources installées : Greffier est prêt.".into(), meeting: None, until: self.clock + 6.0 });
                }
                Evenement::InstallationEnEchec(message) => eprintln!("greffier : installation des ressources : {message}"),
                Evenement::ReunionDisparue(id) => self.forget(id),
                Evenement::ReunionRelue(id) if self.page == Page::Meeting(id) => {
                    // Les répliques et les locuteurs ont pu changer.
                    self.picking = None;
                    self.current = self.meeting().and_then(|m| m.replique_a(self.pos));
                    if self.video.is_none() {
                        self.load_video(id);
                    }
                }
                _ => {}
            }
        }
    }
    /// Ouvre le lexique dans l'éditeur de texte par défaut (sans fenêtre, rien).
    pub fn open_lexique(&mut self) {
        if rune::is_headless() {
            return;
        }
        let Some(chemin) = reglages::chemin_du_lexique() else { return };
        if let Err(e) = std::process::Command::new("/usr/bin/open").arg("-t").arg(&chemin).spawn() {
            eprintln!("greffier : ouvrir le lexique : {e}");
        }
    }
    pub fn open(&mut self, id: IdReunion) {
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
        if self.core.reunion(id).is_some_and(|m| m.nouvelle) {
            self.command(Commande::MarquerLue(id));
        }
    }
    /// La notification de macOS « Bilan prêt », quand l'app n'est pas devant (elle a déjà son
    /// bandeau). Un clic ouvre la réunion (`open_folder_named`), retrouvée par son dossier :
    /// l'identifiant ne survit pas à un redémarrage de l'app. Seule l'app installée peut poster.
    fn notify_bilan(&self, id: IdReunion) {
        if rune::is_headless() || rune_notify::app_focused() {
            return;
        }
        let Some(m) = self.core.reunion(id) else { return };
        let n = rune_notify::Notification::new(format!("bilan:{}", m.nom_du_dossier), "Bilan prêt")
            .body(meeting::display_title(m, true))
            .info("dossier", m.nom_du_dossier.clone());
        rune_notify::post(&n);
    }
    /// Ouvre la réunion dont le dossier porte ce nom (clic sur une notification).
    pub fn open_folder_named(&mut self, nom: &str) {
        if let Some(id) = self.core.reunions().iter().find(|r| r.nom_du_dossier == nom).map(|r| r.id) {
            self.open(id);
        }
    }
    pub fn open_at(&mut self, id: IdReunion, t: f64) {
        self.open(id);
        self.seek(t);
    }
    /// Ouvre l'enregistrement de la réunion, son activé, s'il existe.
    fn load_video(&mut self, id: IdReunion) {
        self.video = None;
        let Some(m) = self.core.reunion(id) else { return };
        if !m.fichiers.contains(&Role::Enregistrement) {
            return;
        }
        // Un média importé garde son extension (`… - enregistrement.m4a`).
        let Some(file) = self.core.dossier().enregistrement(&m.nom_du_dossier) else { return };
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
        self.current = self.meeting().and_then(|m| m.replique_a(self.pos));
    }
    pub fn seek_and_play(&mut self, t: f64) {
        self.seek(t);
        self.play();
    }
    fn play(&mut self) {
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
    /// Lit les réunions d'un dossier de réunions et le retient pour les lancements suivants.
    pub fn use_folder(&mut self, dir: PathBuf) {
        if self.core.occupe() {
            self.folder_error = Some("Impossible de changer de dossier pendant un enregistrement ou un traitement.".into());
            return;
        }
        if let Err(e) = reglages::choisir_dossier_de_reunions(&dir) {
            self.folder_error = Some(e);
            return;
        }
        let settings = self.core.reglages().clone();
        self.core = real_core(&dir, true);
        self.command(Commande::ChangerReglages(settings));
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
    /// Le sélecteur de dossier de macOS, pour choisir le dossier de réunions.
    pub fn pick_folder(&mut self, tx: &mut Tx) {
        let directory = self.folder.clone().or_else(|| reglages::dossier_propose().parent().map(Path::to_path_buf));
        let options = FileDialogOptions { title: "Dossier de réunions".into(), directory, ..Default::default() };
        tx.open_folder(options, |s: &mut App, answer, _| match answer {
            Ok(Some(dir)) => s.use_folder(dir),
            Ok(None) => {}
            Err(e) => s.folder_error = Some(e.to_string()),
        });
    }
    pub fn recording(&self) -> bool {
        self.core.enregistrement().is_some()
    }
    /// Pose l'icône de la barre des menus. Ses choix font les mêmes gestes que le raccourci
    /// global, sur le thread de l'interface (`AppHandle::update`), comme lui.
    fn install_menu_bar(&mut self) {
        let Some(h) = self.handle.clone() else { return };
        self.menu_bar = IconeDeLaBarre::new(move |choix| match choix {
            Choix::Demarrer => h.update(|s: &mut App, _| if !s.recording() { s.toggle_recording() }),
            Choix::Arreter => h.update(|s: &mut App, _| if s.recording() { s.toggle_recording() }),
            Choix::Ouvrir => h.open(),
            Choix::Quitter => h.quit(),
        });
        self.show_menu_bar();
    }
    /// Ce qui suit l'état du cœur hors de la fenêtre : l'icône de la barre des menus et
    /// l'assertion de non-veille.
    fn follow_core(&mut self) {
        self.show_menu_bar();
        self.keep_awake();
    }
    /// Le Mac ne se met pas en veille pendant un enregistrement ou un traitement : l'assertion
    /// suit [`Greffier::garder_le_mac_eveille`] (renommée quand la raison change).
    fn keep_awake(&mut self) {
        let reason = if rune::is_headless() { None } else { self.core.garder_le_mac_eveille() };
        if self.awake.as_ref().map(PasDeVeille::raison) == reason {
            return;
        }
        self.awake = None;
        if let Some(reason) = reason {
            match PasDeVeille::new(reason) {
                Ok(a) => self.awake = Some(a),
                Err(e) => eprintln!("greffier : {e}"),
            }
        }
    }
    /// Montre l'état de l'enregistrement dans la barre des menus.
    fn show_menu_bar(&mut self) {
        let etat = match self.core.enregistrement() {
            Some(r) => greffier_barre::Etat::Enregistrement { duree: r.duree },
            None => greffier_barre::Etat::Pret { possible: !self.needs_folder },
        };
        if let Some(b) = &mut self.menu_bar {
            b.montrer(etat);
        }
    }
    pub fn start_recording(&mut self) {
        if self.needs_folder {
            return;
        }
        if !self.recording() {
            self.rec_phase = 0.0;
            self.record_error = self.try_command(Commande::DemarrerEnregistrement).err().map(|r| r.to_string());
            if self.recording() {
                self.register_marker_shortcut(true);
            } else if let Some(h) = &self.handle {
                // Parti de la barre des menus ou du raccourci, fenêtre cachée : on montre pourquoi.
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
        if self.recording() {
            self.command(Commande::ArreterEnregistrement);
        }
        self.register_marker_shortcut(false);
    }
    pub fn toggle_recording(&mut self) {
        if self.recording() { self.stop_recording() } else { self.start_recording() }
    }
    /// ⌘M de n'importe où pendant l'enregistrement (rendu aux autres apps à l'arrêt).
    fn register_marker_shortcut(&mut self, on: bool) {
        let Some(h) = &self.handle else { return };
        match (on, self.marker_id) {
            (true, None) => match h.register_shortcut("cmd+m".parse().expect("cmd+m"), |s: &mut App, _| s.add_marker()) {
                Ok(id) => self.marker_id = Some(id),
                Err(e) => eprintln!("greffier : ⌘M pour les repères : {e}"),
            },
            (false, Some(id)) => {
                let _ = h.unregister_shortcut(id);
                self.marker_id = None;
            }
            _ => {}
        }
    }
    /// Pose le raccourci global d'enregistrement retenu, au lancement.
    pub fn register_record_shortcut(&mut self) {
        let shortcut = self.shortcut.shortcut().cloned();
        if let Err(e) = self.change_shortcut(shortcut.as_ref()) {
            eprintln!("greffier : raccourci {} : {e}", shortcut.map(|s| s.to_string()).unwrap_or_default());
            self.shortcut.mark_unregistered();
        }
    }
    /// Remplace le raccourci global d'enregistrement ; garde l'ancien si le nouveau est refusé.
    pub fn change_shortcut(&mut self, next: Option<&Shortcut>) -> Result<(), ShortcutError> {
        let h = self.handle.as_ref().ok_or(ShortcutError::NotRunning)?;
        let new = next.map(|sc| h.register_shortcut(sc.clone(), |s: &mut App, _| s.toggle_recording())).transpose()?;
        if let Some(old) = self.shortcut_id.take() {
            let _ = h.unregister_shortcut(old);
        }
        self.shortcut_id = new;
        self.save_settings();
        Ok(())
    }
    /// Retient sources, démarrage automatique et raccourci (pas pour les données fictives).
    fn save_settings(&mut self) {
        if rune::is_headless() || self.folder.is_none() && !self.needs_folder {
            return;
        }
        let raccourci = self.shortcut.shortcut().map(|s| s.to_string()).unwrap_or_default();
        if let Err(e) = reglages::retenir_enregistrement(self.core.reglages(), &raccourci) {
            eprintln!("greffier : réglages : {e}");
        }
    }
    pub fn process(&mut self, id: IdReunion) {
        self.command(Commande::Traiter(id));
    }
    pub fn stop_processing(&mut self, id: IdReunion) {
        self.command(Commande::ArreterTraitement(id));
    }
    pub fn delete(&mut self, id: IdReunion) {
        self.command(Commande::Supprimer(id));
        self.forget(id);
        self.confirm_delete = None;
    }
    /// La réunion n'existe plus (supprimée dans l'app ou hors de l'app) : on quitte sa page.
    fn forget(&mut self, id: IdReunion) {
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
    }
    /// Surveille le dossier de réunions choisi, une fois l'interface prête à être réveillée.
    fn watch_folder(&mut self) {
        self.watch = None;
        let (Some(dir), Some(waker)) = (&self.folder, &self.waker) else { return };
        match surveillance::Surveillance::new(dir, waker.clone()) {
            Ok(w) => self.watch = Some(w),
            Err(e) => eprintln!("greffier : surveillance du dossier de réunions impossible ({e})"),
        }
    }
    pub fn add_marker(&mut self) {
        if self.recording() {
            self.command(Commande::PoserRepere);
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
        self.record_error = None;
        self.save_settings();
    }
    pub fn toggle_auto_process(&mut self) {
        let mut r = self.core.reglages().clone();
        r.traiter_a_l_arret = !r.traiter_a_l_arret;
        self.command(Commande::ChangerReglages(r));
        self.save_settings();
    }
    /// Le sélecteur de dossier de macOS, pour ouvrir un dossier de contexte à l'agent.
    pub fn pick_context_folder(&mut self, tx: &mut Tx) {
        let options = FileDialogOptions { title: "Dossier de contexte de l'agent".into(), directory: self.folder.clone(), ..Default::default() };
        tx.open_folder(options, |s: &mut App, answer, _| {
            if let Ok(Some(dir)) = answer {
                let mut r = s.core.reglages().clone();
                if !r.dossiers_de_contexte.contains(&dir) {
                    r.dossiers_de_contexte.push(dir);
                    s.command(Commande::ChangerReglages(r));
                    s.save_settings();
                }
            }
        });
    }
    pub fn remove_context_folder(&mut self, i: usize) {
        let mut r = self.core.reglages().clone();
        if i < r.dossiers_de_contexte.len() {
            r.dossiers_de_contexte.remove(i);
            self.command(Commande::ChangerReglages(r));
            self.save_settings();
        }
    }
    pub fn toggle_fullscreen(&mut self, tx: &mut Tx) {
        self.fullscreen = !self.fullscreen && self.meeting().is_some_and(meeting::playable);
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

/// Le cœur du prototype : données fictives, dossier temporaire.
fn prototype() -> Greffier {
    Greffier::avec_reunions(prototype_folder(), fictif::adaptateurs(), fictif::reunions())
}

/// Le cœur de la fenêtre sur un dossier de réunions, avec les vraies ressources, dont il lance
/// tout de suite les vérifications.
fn real_core(folder: &Path, reunions: bool) -> Greffier {
    let mut core = if reunions { Greffier::ouvrir(folder, reglages::adaptateurs()) } else { Greffier::avec_reunions(folder, reglages::adaptateurs(), vec![]) };
    let lexique = reglages::chemin_du_lexique();
    for c in [Commande::ChangerReglages(greffier_coeur::Reglages { lexique, ..core.reglages().clone() }), Commande::VerifierRessources] {
        if let Err(e) = core.executer(c) {
            eprintln!("greffier : {e}");
        }
    }
    core
}

/// L'écran du premier lancement des ressources, tant que la transcription ne peut pas tourner.
fn resources_missing(core: &Greffier) -> bool {
    core.ressources().iter().any(|r| r.etapes.contains(&Etape::Transcription) && r.etat != EtatDeRessource::Prete)
}

fn init() -> App {
    // Sans fenêtre (scènes, captures), l'app part des données fictives : rendu déterministe, et
    // jamais d'écriture dans le vrai dossier de réunions ni dans le dossier de l'app.
    let folder = if rune::is_headless() { None } else { reglages::dossier_de_reunions() };
    let (mut core, page) = match &folder {
        _ if rune::is_headless() => (prototype(), Page::Overview),
        Some(dir) => {
            let core = real_core(dir, true);
            let page = if resources_missing(&core) { Page::Installation } else { Page::Overview };
            (core, page)
        }
        None => (real_core(&reglages::dossier_propose(), false), Page::Welcome),
    };
    // Sans fenêtre, les réglages par défaut : rendu déterministe.
    let (settings, shortcut) = if rune::is_headless() { (greffier_coeur::Reglages::default(), reglages::RACCOURCI_PAR_DEFAUT.to_string()) } else { reglages::enregistrement() };
    let _ = core.executer(Commande::ChangerReglages(settings));
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
        shortcut_id: None,
        marker_id: None,
        record_error: None,
        short_recording: None,
        lexique: reglages::lexique(),
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

/// Avance la lecture et fait passer le temps du cœur (enregistrement, file de traitement).
fn poll(s: &mut App, clock: f64, tx: &mut Tx) -> bool {
    let dt = s.last.map_or(0.0, |l| (clock - l).clamp(0.0, 0.25));
    s.clock = clock;
    let mut active = false;
    let mut changed = false;

    // La vidéo ne joue que sur la page de sa réunion.
    if s.video.is_some() && s.player().is_none() {
        s.video = None;
        s.playing = false;
    }
    if let Some(p) = s.player() {
        // La vraie vidéo donne la position ; on la relit tant qu'elle joue.
        let (pos, playing) = (p.time(), p.is_playing());
        if playing || s.playing != playing || (pos - s.pos).abs() > 0.01 {
            changed = true;
        }
        s.playing = playing;
        s.pos = pos;
        if playing {
            active = true;
        }
        s.current = s.meeting().and_then(|m| m.replique_a(s.pos));
    } else if s.playing {
        active = true;
        changed = true;
        let speed = SPEEDS[s.speed];
        let dur = s.duration();
        s.pos = (s.pos + dt * speed).min(dur);
        if s.pos >= dur {
            s.playing = false;
        }
        s.current = s.meeting().and_then(|m| m.replique_a(s.pos));
    }
    // « Écouter » un passage à vérifier s'arrête à la fin de son extrait.
    if s.listen_until.is_some_and(|t| s.playing && s.pos >= t) {
        s.pause();
        changed = true;
    }
    // Une correction qui n'est plus affichée (autre onglet, autre page) est abandonnée.
    if s.correction.is_some() && !verifier::correction_shown(s) {
        s.correction = None;
        changed = true;
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
    // Un fichier a changé dans le dossier de réunions : le cœur relit ce qui a changé hors de l'app.
    if s.watch.as_ref().is_some_and(surveillance::Surveillance::a_change) {
        let events = s.core.relire_le_dossier();
        changed |= !events.is_empty();
        s.handle(events);
    }
    let events = s.core.avancer(dt);
    if s.core.occupe() || !events.is_empty() {
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
    if s.search.is_some() || s.confirm_delete.is_some() || s.import.is_some() || s.process_imported.is_some() || s.short_recording.is_some() || s.redo_transcription.is_some() || s.shortcut.is_recording() || s.correction.is_some() {
        return false;
    }
    // Le raccourci global prend ⇧⌘R quand il est posé ; sinon (sans fenêtre, refusé), l'app le garde.
    if ev.is_command('r') && ev.mods.shift && s.shortcut_id.is_none() {
        s.toggle_recording();
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
    if s.tab == Tab::Doubts && verifier::on_key(s, ev, tx) {
        return true;
    }
    if s.tab == Tab::Transcript
        && matches!(&ev.key, Key::Text(t) if t.eq_ignore_ascii_case("c"))
        && let Some(i) = s.current
    {
        s.start_correction(verifier::Cible::Replique(i));
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
    overlays::short_recording(s, &k, &mut layer);
    titre::window(s, &k, &mut layer);
    refaire::window(s, &k, &mut layer);
    import::window(s, &k, &mut layer);
    import::process_window(s, &k, &mut layer);
    // Des fichiers déposés n'importe où sur la fenêtre ouvrent l'import.
    rune_kit::root(shell, layer).on_drop_files(|s: &mut App, files, _| s.propose_import(files))
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
/// Un passage tranché (hypothèse acceptée), et le suivant en cours de correction.
fn scene_doubt_fix(s: &mut App) {
    scene_doubts(s);
    s.resolve_doubt(0, greffier_coeur::Tranche::AccepterHypothese);
    if let Some(i) = s.meeting().and_then(|m| verifier::selected(s, m)) {
        s.start_correction(verifier::Cible::Passage(i));
    }
}
/// La correction d'une réplique dans l'onglet Transcription.
fn scene_transcript_fix(s: &mut App) {
    scene_transcript(s);
    if let Some(i) = s.current {
        s.start_correction(verifier::Cible::Replique(i));
    }
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
/// La page d'enregistrement avant le démarrage : sources et autorisations.
fn scene_record_ready(s: &mut App) {
    s.page = Page::Recording;
}
/// À l'arrêt d'un enregistrement de 12 s (capture au temps réel) : proposé à la suppression.
fn scene_short_recording(s: &mut App) {
    let adaptateurs = greffier_coeur::adaptateurs::Adaptateurs { capture: Box::new(greffier_coeur::faux::FausseCapture::new(1.0, 0.0)), ..fictif::adaptateurs() };
    s.core = Greffier::avec_reunions(prototype_folder(), adaptateurs, fictif::reunions());
    s.start_recording();
    s.core.avancer(12.0);
    s.stop_recording();
}
/// La réunion à traiter, transcrite, à 40 % de la relecture.
fn scene_processing(s: &mut App) {
    s.open(1);
    s.process(1);
    s.core.avancer(0.0);
    s.core.avancer(fictif::duree_simulee(Etape::Transcription));
    s.core.avancer(0.4 * fictif::duree_simulee(Etape::Relecture));
}
/// La file : la réunion du jour en transcription, la revue de release en attente derrière elle.
fn scene_queue(s: &mut App) {
    let second = s.core.reunions()[1].id;
    s.process(1);
    s.core.avancer(0.0);
    s.core.avancer(0.5 * fictif::duree_simulee(Etape::Transcription));
    s.command(Commande::Refaire { reunion: second, etape: Etape::Bilan });
    s.open(second);
}
/// La transcription a échoué : le message et « Réessayer cette étape ».
fn scene_failed(s: &mut App) {
    let transcripteur = greffier_coeur::faux::FauxTranscripteur::new(1.0, |_| Err("Le média n'a pas de son.".into()));
    let adaptateurs = greffier_coeur::adaptateurs::Adaptateurs { transcripteur: Box::new(transcripteur), ..fictif::adaptateurs() };
    s.core = Greffier::avec_reunions(prototype_folder(), adaptateurs, fictif::reunions());
    s.open(1);
    s.process(1);
    s.core.avancer(0.0);
    s.core.avancer(1.0);
}
/// Le menu « Refaire… » de l'onglet Transcription.
fn scene_redo(s: &mut App) {
    scene_transcript(s);
    s.redo_menu = true;
}
/// « Refaire la transcription… » : le nombre de locuteurs avant de relancer.
fn scene_redo_transcription(s: &mut App) {
    scene_transcript(s);
    s.start_redo_transcription(2);
    if let Some(r) = &mut s.redo_transcription {
        r.nombre = Some(4);
    }
}
fn scene_unprocessed(s: &mut App) {
    s.open(1);
}
fn scene_settings(s: &mut App) {
    s.page = Page::Settings;
}
/// La fenêtre « Changer le titre » sur la réunion ouverte.
fn scene_rename(s: &mut App) {
    scene_meeting(s);
    s.start_rename(2);
}
/// Les réglages avec deux dossiers de contexte ouverts à l'agent.
fn scene_context(s: &mut App) {
    let dossiers = ["/Users/odile/Documents/Notes", "/Users/odile/Projets/Atelier"].map(PathBuf::from).to_vec();
    s.command(Commande::ChangerReglages(greffier_coeur::Reglages { dossiers_de_contexte: dossiers, ..s.core.reglages().clone() }));
    s.page = Page::Settings;
}
/// Le premier lancement : un téléchargement, une installation, un sha256 faux, `claude` absente.
fn scene_installation(s: &mut App) {
    let adaptateurs = greffier_coeur::adaptateurs::Adaptateurs { ressources: Box::new(FaussesRessources::fixes(fictif::ressources_en_installation())), ..fictif::adaptateurs() };
    s.core = Greffier::avec_reunions(prototype_folder(), adaptateurs, fictif::reunions());
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
    s.core = Greffier::avec_reunions(prototype_folder(), fictif::adaptateurs(), vec![]);
    s.page = Page::Welcome;
    s.needs_folder = true;
}
/// Les réunions d'exemple des tests du cœur (une copie), la réunion complète ouverte à 0:00:05.
fn scene_folder(s: &mut App) {
    let dir = prototype_folder();
    copy_dir(&Path::new(env!("CARGO_MANIFEST_DIR")).join("coeur/tests/fixtures/dossier-de-reunions"), &dir);
    s.core = Greffier::ouvrir(&dir, fictif::adaptateurs());
    s.folder = Some(dir);
    if let Some(id) = s.core.reunions().first().map(|m| m.id) {
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
/// Les réunions d'exemple : celle qui n'a que son enregistrement.
fn scene_video_only(s: &mut App) {
    scene_folder(s);
    if let Some(id) = s.core.reunions().get(1).map(|m| m.id) {
        s.open(id);
    }
}

/// La fenêtre d'import sur la vidéo et la transcription d'une réunion d'exemple.
fn scene_import(s: &mut App) {
    scene_folder(s);
    s.page = Page::Overview;
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("coeur/tests/fixtures/dossier-de-reunions/2026-11-18 09h15 - Réunion");
    let dir = std::env::temp_dir().join("greffier-prototype-import");
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::create_dir_all(&dir);
    let media = dir.join("Point banc de test.mp4");
    let transcription = dir.join("Point banc de test.json");
    let _ = std::fs::copy(fixture.join("2026-11-18 09h15 - Réunion - enregistrement.mp4"), &media);
    let _ = std::fs::copy(fixture.join("2026-11-18 09h15 - Réunion - transcription.json"), &transcription);
    s.propose_import(&[media, transcription]);
    // La date du fichier copié change à chaque rendu : on la fixe.
    if let Some(d) = &mut s.import {
        d.date = TextEdit::new("2026-11-18");
        d.heure = TextEdit::new("09:15");
    }
}
/// Après l'import : la question « maintenant ou plus tard ».
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
            ("info", scene_info),
            ("recording", scene_recording),
            ("record-ready", scene_record_ready),
            ("short-recording", scene_short_recording),
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
    let importer = rune::MenuItem::new("Importer…", |s: &mut App, tx: &mut rune::Tx| s.pick_import(tx)).id("importer").menu("Fichier").key_equivalent("cmd+i".parse().expect("raccourci"));
    // Fermer la fenêtre garde l'app (et l'icône de la barre des menus) : ⌘Q la quitte.
    let app = rune::App::new(spec).chrome(chrome).menu_item(importer).keep_running();
    let handle = app.handle();
    let shared = handle.clone();
    let shared_for_clicks = handle.clone();
    handle.update(move |s, _| {
        s.handle = Some(shared);
        s.register_record_shortcut();
        if !rune::is_headless() {
            s.install_menu_bar();
            // Un clic sur « Bilan prêt » ouvre la réunion, même si la fenêtre était fermée.
            let h = shared_for_clicks.clone();
            rune_notify::on_click(move |click| {
                let Some(nom) = click.get("dossier").map(str::to_owned) else { return };
                h.open();
                h.update(move |s: &mut App, _| {
                    s.open_folder_named(&nom);
                    rune_notify::bring_to_front();
                });
            });
        }
    });
    app.run();
}

