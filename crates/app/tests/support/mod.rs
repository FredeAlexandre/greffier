//! The app's test bench: the real app (its `poll`, `on_key`, `view`) in a Rune runtime with no
//! window, on a core with fake adapters in a temporary folder. A test acts as the user does — a
//! click on an element by its key, a key press, time passing — then looks at the app's state or
//! the core's: the page, the tab, playback, the meetings and their files. Never at pixels or
//! wording.

#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use greffier::App;
use greffier_core::{Greffier, Meeting, demo};
use rune::{Env, Key, KeyEvent, Mods, Runtime};

/// The window's size in the tests: the app's default.
pub const SIZE: (f32, f32) = (1360.0, 860.0);

/// A runtime on the app driven by `core`, its first frame built.
pub fn app_on(core: Greffier) -> Runtime<App> {
    let mut rt = rune::App::new(greffier::spec()).init(move || App::with_core(core)).headless_runtime(SIZE, Env::new(false, 1.0));
    rt.frame(0.0);
    rt
}

/// The prototype's meetings (`greffier_core::demo`) on its fake adapters, in a fresh folder. Tests
/// read what they need from the core (a decision's moment, an utterance's start) instead of
/// copying the demo's values.
pub fn demo_core() -> Greffier {
    Greffier::with_meetings(temp_folder(), demo::adapters(), demo::meetings())
}

/// `meetings` on the prototype's fake adapters, in a fresh folder.
pub fn core_with(meetings: Vec<Meeting>) -> Greffier {
    Greffier::with_meetings(temp_folder(), demo::adapters(), meetings)
}

/// A fresh, empty temporary folder.
pub fn temp_folder() -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let folder = std::env::temp_dir().join(format!("greffier-app-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).expect("temporary folder");
    folder
}

/// Reads the app's state.
pub fn state<T>(rt: &mut Runtime<App>, f: impl FnOnce(&App) -> T) -> T {
    let mut out = None;
    rt.update(|s, _| out = Some(f(s)));
    out.expect("update ran")
}

/// Lets `seconds` pass, a frame every 1/30 s, as the window does.
pub fn wait(rt: &mut Runtime<App>, seconds: f64) {
    let step = 1.0 / 30.0;
    let mut t = 0.0;
    while t < seconds {
        rt.frame(step);
        t += step;
    }
}

/// Builds a frame without moving time: what a click or key changed is drawn.
pub fn settle(rt: &mut Runtime<App>) {
    rt.frame(0.0);
    rt.frame(0.0);
}

/// Whether the element keyed `key` is on screen.
pub fn shown(rt: &Runtime<App>, key: &str) -> bool {
    rt.find(key).is_some()
}

/// Where the user can click the element keyed `key`: its centre, inside the window. Panics if it
/// is not there: a test clicks what the user sees, and they cannot click what is scrolled away.
pub fn on_screen(rt: &Runtime<App>, key: &str) -> (f32, f32) {
    let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no element keyed {key:?}"));
    assert!((0.0..SIZE.0).contains(&x) && (0.0..SIZE.1).contains(&y), "{key:?} is outside the window, at ({x}, {y}): scroll to it first");
    (x, y)
}

/// A click on the element keyed `key` (see [`on_screen`]).
pub fn tap(rt: &mut Runtime<App>, key: &str) {
    let (x, y) = on_screen(rt, key);
    rt.pointer_move(x, y);
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    settle(rt);
}

/// A right click on the element keyed `key`.
pub fn secondary_tap(rt: &mut Runtime<App>, key: &str) {
    let (x, y) = on_screen(rt, key);
    rt.pointer_move(x, y);
    rt.secondary_down(x, y);
    rt.secondary_up(x, y);
    settle(rt);
}

/// Presses `ev`.
pub fn press(rt: &mut Runtime<App>, ev: KeyEvent) {
    rt.key(ev);
    settle(rt);
}

/// Types `text`, one character at a time.
pub fn type_text(rt: &mut Runtime<App>, text: &str) {
    for c in text.chars() {
        rt.key(KeyEvent { key: Key::Text(c.to_string()), mods: Mods::default() });
    }
    settle(rt);
}

/// ⌘ + `c`.
pub fn cmd(c: char) -> KeyEvent {
    KeyEvent { key: Key::Command(c), mods: Mods { cmd: true, ..Mods::default() } }
}

/// ⇧⌘ + `c`.
pub fn shift_cmd(c: char) -> KeyEvent {
    KeyEvent { key: Key::Command(c), mods: Mods { cmd: true, shift: true, ..Mods::default() } }
}

/// A character key without modifier (`' '` is the space bar).
pub fn key(c: char) -> KeyEvent {
    KeyEvent { key: Key::Text(c.to_string()), mods: Mods::default() }
}

/// A named key without modifier (`Key::Enter`, `Key::Left`, `Key::Escape`…).
pub fn named(k: Key) -> KeyEvent {
    KeyEvent { key: k, mods: Mods::default() }
}

/// The drawn rectangle `[x, y, w, h]` of the element keyed `key`, as of the last frame. `a/b` is
/// the element keyed `b` inside the one keyed `a` (e.g. `summary-md/i1`, the summary's second
/// checkbox).
pub fn rect(rt: &Runtime<App>, key: &str) -> [f32; 4] {
    find_rect(rt, key).unwrap_or_else(|| panic!("no element keyed {key:?}"))
}

fn find_rect(rt: &Runtime<App>, key: &str) -> Option<[f32; 4]> {
    let (within, own) = key.rsplit_once('/').map_or((None, key), |(a, b)| (Some(format!("/{a}/")), b));
    let e = rt.elements().into_iter().find(|e| !e.exiting && e.key.as_deref() == Some(own) && within.as_ref().is_none_or(|w| e.path.contains(w.as_str())))?;
    Some(e.rect)
}

/// A click at a point of the window.
pub fn click_at(rt: &mut Runtime<App>, x: f32, y: f32) {
    assert!((0.0..SIZE.0).contains(&x) && (0.0..SIZE.1).contains(&y), "({x}, {y}) is outside the window");
    rt.pointer_move(x, y);
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    settle(rt);
}

/// Scrolls the meeting's page with the wheel, the pointer in its left margin (outside the
/// transcript, which scrolls on its own), until the element keyed `key` is well inside the window.
pub fn scroll_page_to(rt: &mut Runtime<App>, key: &str) {
    scroll_page_until(rt, key, |rt| rt.find(key).map(|(_, y)| y).or_else(|| find_rect(rt, key).map(|r| r[1] + r[3] / 2.0)));
}

/// [`scroll_page_to`] for an element found otherwise than by its key: `y_of` gives its centre's
/// height in the window, `what` names it in the failure.
pub fn scroll_page_until(rt: &mut Runtime<App>, what: &str, y_of: impl Fn(&Runtime<App>) -> Option<f32>) {
    rt.set_wheel_glide(false);
    let (x, _) = rt.find("meeting").expect("a meeting's page");
    let margin_x = x - rect(rt, "meeting")[2] / 2.0 + 10.0;
    let mut last = None;
    for _ in 0..200 {
        let y = y_of(rt).unwrap_or_else(|| panic!("no {what}"));
        // Well inside, or as far as the page scrolls (the bottom of the page) and inside.
        if (80.0..SIZE.1 - 80.0).contains(&y) || (last == Some(y) && (20.0..SIZE.1 - 20.0).contains(&y)) {
            return;
        }
        last = Some(y);
        rt.wheel(margin_x, SIZE.1 / 2.0, if y < 80.0 { 120.0 } else { -120.0 });
        rt.frame(1.0 / 30.0);
    }
    panic!("could not scroll {what} into the window");
}

// --- Library, recording, processing, import and settings -----------------------------------

/// Picks the item `label` in the open menu keyed `menu` (`menu{id}` for a meeting's right-click
/// menu, `redo-menu` for the Transcript tab's « Refaire… »). rune-kit keys a menu's items by
/// their position only, which changes with the items the meeting offers: the item is found by
/// its label, as the user finds it. Panics if the menu has no such item.
pub fn choose(rt: &mut Runtime<App>, menu: &str, label: &str) {
    let prefix = format!("{menu}-item");
    let key = rt
        .elements()
        .into_iter()
        .filter(|e| !e.exiting && e.label.as_deref() == Some(label))
        .filter_map(|e| e.key)
        .find(|k| k.starts_with(&prefix))
        .unwrap_or_else(|| panic!("no item {label:?} in the menu {menu:?}"));
    tap(rt, &key);
}

/// The stage a meeting is queued at (waiting or running), if it is in the queue.
pub fn queued_at(status: &greffier_core::Status) -> Option<greffier_core::Stage> {
    use greffier_core::Status;
    match status {
        Status::Pending(stage) | Status::Running { stage, .. } => Some(*stage),
        _ => None,
    }
}

/// The core tests' sample meetings folder (`crates/core/tests/fixtures/meetings-folder`), copied
/// to a fresh folder: meetings that exist on disk, to delete or rename.
pub fn fixtures_folder() -> PathBuf {
    let folder = temp_folder();
    copy_dir(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../core/tests/fixtures/meetings-folder"), &folder);
    folder
}

fn copy_dir(from: &std::path::Path, to: &std::path::Path) {
    std::fs::create_dir_all(to).expect("folder");
    for e in std::fs::read_dir(from).expect("fixtures").flatten() {
        let target = to.join(e.file_name());
        if e.file_type().is_ok_and(|t| t.is_dir()) {
            copy_dir(&e.path(), &target);
        } else {
            std::fs::copy(e.path(), target).expect("copy");
        }
    }
}

/// The prototype's fake adapters with this capture, this trash or this diarizer instead.
pub fn adapters_with(change: impl FnOnce(&mut greffier_core::adapters::Adapters)) -> greffier_core::adapters::Adapters {
    let mut adapters = demo::adapters();
    change(&mut adapters);
    adapters
}

/// The macOS permissions as the test sets them, standing for macOS: a request shows its prompt
/// and changes nothing; the test answers it (or changes System Settings) with [`MacPermissions::set`],
/// and the capture adapter reads the new state at once.
#[derive(Clone)]
pub struct MacPermissions(std::rc::Rc<std::cell::Cell<greffier_core::adapters::Permissions>>, std::rc::Rc<std::cell::RefCell<Vec<greffier_core::adapters::Permission>>>);

impl MacPermissions {
    pub fn new(permissions: greffier_core::adapters::Permissions) -> MacPermissions {
        MacPermissions(std::rc::Rc::new(std::cell::Cell::new(permissions)), Default::default())
    }
    /// The permissions the app asked macOS for, in order.
    pub fn asked(&self) -> Vec<greffier_core::adapters::Permission> {
        self.1.borrow().clone()
    }
    pub fn get(&self) -> greffier_core::adapters::Permissions {
        self.0.get()
    }
    pub fn set(&self, permission: greffier_core::adapters::Permission, state: greffier_core::adapters::PermissionState) {
        let mut p = self.0.get();
        *p.state_mut(permission) = state;
        self.0.set(p);
    }
    /// The prototype's fake capture, its permissions read from these.
    pub fn capture(&self) -> Box<dyn greffier_core::adapters::Capture> {
        Box::new(CaptureWithPermissions { permissions: self.clone(), capture: greffier_core::fake::FakeCapture::new(demo::RECORDING_SPEED, 60.0) })
    }
}

struct CaptureWithPermissions {
    permissions: MacPermissions,
    capture: greffier_core::fake::FakeCapture,
}

impl greffier_core::adapters::Capture for CaptureWithPermissions {
    fn permissions(&self) -> greffier_core::adapters::Permissions {
        self.permissions.get()
    }
    fn request_permission(&mut self, permission: greffier_core::adapters::Permission) {
        self.permissions.1.borrow_mut().push(permission);
    }
    fn start(&mut self, request: greffier_core::adapters::CaptureRequest) -> Result<(), String> {
        self.capture.start(request)
    }
    fn poll(&mut self, elapsed: f64) -> greffier_core::adapters::CaptureState {
        self.capture.poll(elapsed)
    }
    fn stop(&mut self) -> Result<f64, String> {
        self.capture.stop()
    }
}

/// The notification permission as the test sets it, standing for macOS: a request shows its
/// prompt and changes nothing; the test answers it with [`FakeNotifications::set`].
#[derive(Clone)]
pub struct FakeNotifications(std::rc::Rc<std::cell::Cell<Option<greffier_core::adapters::PermissionState>>>, std::rc::Rc<std::cell::Cell<u32>>);

impl FakeNotifications {
    pub fn new(state: greffier_core::adapters::PermissionState) -> FakeNotifications {
        FakeNotifications(std::rc::Rc::new(std::cell::Cell::new(Some(state))), Default::default())
    }
    pub fn set(&self, state: greffier_core::adapters::PermissionState) {
        self.0.set(Some(state));
    }
    /// How many times the app asked macOS.
    pub fn asked(&self) -> u32 {
        self.1.get()
    }
}

impl greffier::NotificationPermission for FakeNotifications {
    fn state(&self) -> Option<greffier_core::adapters::PermissionState> {
        self.0.get()
    }
    fn request(&self) {
        self.1.set(self.1.get() + 1);
    }
}

/// The system's global shortcuts, standing for macOS: what the app registered, by chord.
#[derive(Clone, Default)]
pub struct FakeShortcuts(std::rc::Rc<std::cell::RefCell<Vec<(u32, rune::Shortcut)>>>, std::rc::Rc<std::cell::Cell<u32>>);

impl FakeShortcuts {
    /// The chords registered now.
    pub fn active(&self) -> Vec<rune::Shortcut> {
        self.0.borrow().iter().map(|(_, s)| s.clone()).collect()
    }
}

impl greffier::Registrar for FakeShortcuts {
    type Id = greffier::Registration;
    fn register(&self, shortcut: &rune::Shortcut) -> Result<greffier::Registration, rune::ShortcutError> {
        if self.0.borrow().iter().any(|(_, s)| s == shortcut) {
            return Err(rune::ShortcutError::Conflict);
        }
        let id = self.1.get();
        self.1.set(id + 1);
        self.0.borrow_mut().push((id, shortcut.clone()));
        Ok(Box::new(id))
    }
    fn unregister(&self, id: greffier::Registration) {
        let id = *id.downcast::<u32>().expect("one of ours");
        self.0.borrow_mut().retain(|(i, _)| *i != id);
    }
}

/// Lets a page or window that just opened finish coming in (half a second), so a click lands
/// where the user sees its buttons: mid-motion, a click can fall outside a window and close it.
pub fn wait_for_motion(rt: &mut Runtime<App>) {
    wait(rt, 0.5);
}

/// Scrolls the scroll area keyed `scroller` with the wheel, the pointer at its left edge, until
/// the element keyed `key` is well inside the window.
pub fn scroll_to(rt: &mut Runtime<App>, scroller: &str, key: &str) {
    rt.set_wheel_glide(false);
    let r = rect(rt, scroller);
    let x = r[0] + 10.0;
    for _ in 0..200 {
        let (_, y) = rt.find(key).unwrap_or_else(|| panic!("no element keyed {key:?}"));
        if (80.0..SIZE.1 - 80.0).contains(&y) {
            return;
        }
        rt.wheel(x, SIZE.1 / 2.0, if y < 80.0 { 120.0 } else { -120.0 });
        settle(rt);
    }
    panic!("could not scroll {key:?} into the window");
}
