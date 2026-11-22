//! L'icône dans la barre des menus de macOS : un `NSStatusItem` et son `NSMenu`, construits et
//! changés depuis le thread principal. Tout `unsafe` de la barre est ici.

use std::cell::RefCell;
use std::time::{Duration, Instant};

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{NSColor, NSImage, NSMenu, NSMenuItem, NSStatusBar, NSStatusItem, NSVariableStatusItemLength};
use objc2_foundation::{NSRunLoop, NSRunLoopCommonModes, NSString, NSTimer};

use crate::{Choice, State, Icon, Line, menu, duration_text};

const CHOICES: [Choice; 4] = [Choice::Start, Choice::Stop, Choice::Open, Choice::Quit];

/// La cible des lignes du menu et de la minuterie de la durée.
struct Target {
    choose: Box<dyn Fn(Choice)>,
    /// La ligne de la durée et l'instant où l'enregistrement a commencé, tant qu'il tourne.
    duration: RefCell<Option<(Retained<NSMenuItem>, Instant)>>,
}

define_class!(
    // SAFETY : NSObject n'a pas d'exigence de sous-classe ; la classe ne surcharge pas dealloc.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "GreffierCibleDeLaBarre"]
    #[ivars = Target]
    struct MenubarTarget;

    impl MenubarTarget {
        #[unsafe(method(choose:))]
        fn choose(&self, line: &NSMenuItem) {
            if let Some(&c) = CHOICES.get(line.tag() as usize) {
                (self.ivars().choose)(c);
            }
        }

        /// La durée avance même quand l'app ne la redonne pas (menu ouvert, fenêtre fermée).
        #[unsafe(method(heartbeat:))]
        fn heartbeat(&self, _timer: &NSTimer) {
            if let Some((line, start)) = &*self.ivars().duration.borrow() {
                line.setTitle(&NSString::from_str(&duration_text(start.elapsed().as_secs_f64())));
            }
        }
    }
);

/// L'icône de Greffier dans la barre des menus. Elle disparaît quand on la lâche.
pub struct MenubarIcon {
    mtm: MainThreadMarker,
    item: Retained<NSStatusItem>,
    menu: Retained<NSMenu>,
    target: Retained<MenubarTarget>,
    timer: Option<Retained<NSTimer>>,
    /// La forme du menu affiché (l'état sans la durée) : on ne le reconstruit que si elle change.
    shape: Option<Option<bool>>,
}

impl MenubarIcon {
    /// Pose l'icône. `choisir` reçoit les choix du menu, sur le thread principal. `None` hors du
    /// thread principal.
    pub fn new(choose: impl Fn(Choice) + 'static) -> Option<Self> {
        let mtm = MainThreadMarker::new()?;
        let item = NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
        let menu = NSMenu::new(mtm);
        menu.setAutoenablesItems(false);
        item.setMenu(Some(&menu));
        let target = MenubarTarget::alloc(mtm).set_ivars(Target { choose: Box::new(choose), duration: RefCell::new(None) });
        // SAFETY : `init` de NSObject, sur une instance tout juste allouée.
        let target: Retained<MenubarTarget> = unsafe { msg_send![super(target), init] };
        Some(MenubarIcon { mtm, item, menu, target, timer: None, shape: None })
    }

    /// Montre l'état de l'app : l'aspect de l'icône, les lignes du menu, la durée.
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
        // SAFETY : `choisir:` existe sur la cible et prend l'NSMenuItem qui l'envoie.
        let item = unsafe { NSMenuItem::initWithTitle_action_keyEquivalent(NSMenuItem::alloc(self.mtm), &NSString::from_str(text), action, &NSString::new()) };
        if let Some(c) = choice {
            item.setTag(CHOICES.iter().position(|&x| x == c).unwrap_or(0) as isize);
            // SAFETY : la cible vit autant que l'icône, qui garde le menu.
            unsafe { item.setTarget(Some(&self.target as &AnyObject)) };
        }
        item
    }

    /// Une minuterie, dans tous les modes de la boucle (aussi quand le menu est ouvert), le temps
    /// de l'enregistrement.
    fn schedule_timer(&mut self, recording: bool) {
        match (recording, &self.timer) {
            (true, None) => {
                // SAFETY : `battre:` existe sur la cible et prend la minuterie ; elle tourne sur
                // la boucle du thread principal, celle de la cible.
                let t = unsafe { NSTimer::timerWithTimeInterval_target_selector_userInfo_repeats(0.5, &self.target, sel!(heartbeat:), None, true) };
                // SAFETY : ajoutée à la boucle principale, depuis le thread principal.
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
