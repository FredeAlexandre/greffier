//! The icon in the macOS menu bar: an `NSStatusItem` and its `NSMenu`, built and changed from the
//! main thread. All of the menu bar's `unsafe` is here.

use std::cell::RefCell;
use std::time::{Duration, Instant};

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{NSColor, NSImage, NSMenu, NSMenuItem, NSStatusBar, NSStatusItem, NSVariableStatusItemLength};
use objc2_foundation::{NSRunLoop, NSRunLoopCommonModes, NSString, NSTimer};

use crate::{Choice, State, Icon, Line, menu, duration_text};

const CHOICES: [Choice; 4] = [Choice::Start, Choice::Stop, Choice::Open, Choice::Quit];

/// The target of the menu lines and of the duration timer.
struct Target {
    choose: Box<dyn Fn(Choice)>,
    /// The duration line and the time the recording started, while it is running.
    duration: RefCell<Option<(Retained<NSMenuItem>, Instant)>>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements; the class doesn't override dealloc.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "GreffierMenubarTarget"]
    #[ivars = Target]
    struct MenubarTarget;

    impl MenubarTarget {
        #[unsafe(method(choose:))]
        fn choose(&self, line: &NSMenuItem) {
            if let Some(&c) = CHOICES.get(line.tag() as usize) {
                (self.ivars().choose)(c);
            }
        }

        /// The duration moves on even when the app doesn't send it again (menu open, window closed).
        #[unsafe(method(heartbeat:))]
        fn heartbeat(&self, _timer: &NSTimer) {
            if let Some((line, start)) = &*self.ivars().duration.borrow() {
                line.setTitle(&NSString::from_str(&duration_text(start.elapsed().as_secs_f64())));
            }
        }
    }
);

/// Greffier's icon in the menu bar. It disappears when dropped.
pub struct MenubarIcon {
    mtm: MainThreadMarker,
    item: Retained<NSStatusItem>,
    menu: Retained<NSMenu>,
    target: Retained<MenubarTarget>,
    timer: Option<Retained<NSTimer>>,
    /// The shape of the menu shown (the state without the duration): it is rebuilt only when this changes.
    shape: Option<Option<bool>>,
}

impl MenubarIcon {
    /// Puts up the icon. `choose` receives the menu choices, on the main thread. `None` off the
    /// main thread.
    pub fn new(choose: impl Fn(Choice) + 'static) -> Option<Self> {
        let mtm = MainThreadMarker::new()?;
        let item = NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
        let menu = NSMenu::new(mtm);
        menu.setAutoenablesItems(false);
        item.setMenu(Some(&menu));
        let target = MenubarTarget::alloc(mtm).set_ivars(Target { choose: Box::new(choose), duration: RefCell::new(None) });
        // SAFETY: NSObject's `init`, on a freshly allocated instance.
        let target: Retained<MenubarTarget> = unsafe { msg_send![super(target), init] };
        Some(MenubarIcon { mtm, item, menu, target, timer: None, shape: None })
    }

    /// Shows the app's state: the icon's look, the menu lines, the duration.
    pub fn show(&mut self, state: State) {
        let shape = match state {
            State::Ready { possible } => Some(possible),
            State::Recording { .. } => None,
        };
        let m = menu(state);
        if self.shape != Some(shape) {
            self.shape = Some(shape);
            self.draw_icon(m.icon);
            self.fill(&m.lines);
            self.schedule_timer(m.icon == Icon::Recording);
        }
        if let State::Recording { duration } = state
            && let Some((line, start)) = &mut *self.target.ivars().duration.borrow_mut()
        {
            *start = Instant::now() - Duration::from_secs_f64(duration.max(0.0));
            line.setTitle(&NSString::from_str(&duration_text(duration)));
        }
    }

    fn draw_icon(&self, icon: Icon) {
        let Some(button) = self.item.button(self.mtm) else { return };
        let (symbol, description, tint) = match icon {
            Icon::Idle => ("waveform", "Greffier", None),
            Icon::Recording => ("record.circle.fill", "Greffier : enregistrement en cours", Some(NSColor::systemRedColor())),
        };
        match NSImage::imageWithSystemSymbolName_accessibilityDescription(&NSString::from_str(symbol), Some(&NSString::from_str(description))) {
            Some(image) => {
                image.setTemplate(true);
                button.setImage(Some(&image));
                button.setTitle(&NSString::new());
            }
            None => {
                button.setImage(None);
                button.setTitle(&NSString::from_str(if icon == Icon::Idle { "Greffier" } else { "● Greffier" }));
            }
        }
        button.setContentTintColor(tint.as_deref());
    }

    fn fill(&self, lines: &[Line]) {
        self.menu.removeAllItems();
        let mut duration = None;
        for l in lines {
            let item = match l {
                Line::Separator => NSMenuItem::separatorItem(self.mtm),
                Line::Info(text) => {
                    let item = self.line(text, None);
                    item.setEnabled(false);
                    duration = Some(item.clone());
                    item
                }
                Line::Choice { text, choice, active } => {
                    let item = self.line(text, Some(*choice));
                    item.setEnabled(*active);
                    item
                }
            };
            self.menu.addItem(&item);
        }
        *self.target.ivars().duration.borrow_mut() = duration.map(|l| (l, Instant::now()));
    }

    fn line(&self, text: &str, choice: Option<Choice>) -> Retained<NSMenuItem> {
        let action = choice.map(|_| sel!(choose:));
        // SAFETY: `choose:` exists on the target and takes the NSMenuItem that sends it.
        let item = unsafe { NSMenuItem::initWithTitle_action_keyEquivalent(NSMenuItem::alloc(self.mtm), &NSString::from_str(text), action, &NSString::new()) };
        if let Some(c) = choice {
            item.setTag(CHOICES.iter().position(|&x| x == c).unwrap_or(0) as isize);
            // SAFETY: the target lives as long as the icon, which holds the menu.
            unsafe { item.setTarget(Some(&self.target as &AnyObject)) };
        }
        item
    }

    /// A timer, in all run loop modes (also while the menu is open), for the length of the
    /// recording.
    fn schedule_timer(&mut self, recording: bool) {
        match (recording, &self.timer) {
            (true, None) => {
                // SAFETY: `heartbeat:` exists on the target and takes the timer; it runs on the
                // main thread's run loop, the target's.
                let t = unsafe { NSTimer::timerWithTimeInterval_target_selector_userInfo_repeats(0.5, &self.target, sel!(heartbeat:), None, true) };
                // SAFETY: added to the main run loop, from the main thread.
                unsafe { NSRunLoop::mainRunLoop().addTimer_forMode(&t, NSRunLoopCommonModes) };
                self.timer = Some(t);
            }
            (false, Some(t)) => {
                t.invalidate();
                self.timer = None;
            }
            _ => {}
        }
    }
}

impl Drop for MenubarIcon {
    fn drop(&mut self) {
        if let Some(t) = self.timer.take() {
            t.invalidate();
        }
        NSStatusBar::systemStatusBar().removeStatusItem(&self.item);
    }
}
