//! The global shortcut that starts and stops recording. It is registered system-wide, except
//! while the settings field captures a new one: the chord the user types must reach the field,
//! even when it is the active shortcut, instead of starting a recording.

use rune::{Shortcut, ShortcutError};

/// Registers and releases the recording shortcut (the app handle; a fake in the tests).
pub trait Registrar {
    type Id;
    fn register(&self, shortcut: &Shortcut) -> Result<Self::Id, ShortcutError>;
    fn unregister(&self, id: Self::Id);
}

/// What is registered of the recording shortcut. Holds at most one registration.
#[derive(Debug)]
pub struct RecordShortcut<Id> {
    registered: Option<Id>,
    /// The settings field is capturing a shortcut: the registration was released.
    suspended: bool,
}

impl<Id> Default for RecordShortcut<Id> {
    fn default() -> Self {
        RecordShortcut { registered: None, suspended: false }
    }
}

impl<Id> RecordShortcut<Id> {
    pub fn is_registered(&self) -> bool {
        self.registered.is_some()
    }

    /// Registers `next` (`None`: no shortcut) in place of the current registration. If `next` is
    /// refused, the current registration stays.
    pub fn change(&mut self, r: &impl Registrar<Id = Id>, next: Option<&Shortcut>) -> Result<(), ShortcutError> {
        let new = next.map(|sc| r.register(sc)).transpose()?;
        if let Some(old) = std::mem::replace(&mut self.registered, new) {
            r.unregister(old);
        }
        Ok(())
    }

    /// Follows the settings field: releases the registration when it starts capturing, and
    /// registers `chosen` (its value) when it stops, unless a change made during the capture is
    /// already registered. Does nothing while the field stays in the same phase. An error: `chosen`
    /// was refused when coming back, and nothing is registered.
    pub fn follow_capture(&mut self, r: &impl Registrar<Id = Id>, capturing: bool, chosen: Option<&Shortcut>) -> Result<(), ShortcutError> {
        if capturing == self.suspended {
            return Ok(());
        }
        self.suspended = capturing;
        if capturing {
            if let Some(id) = self.registered.take() {
                r.unregister(id);
            }
            Ok(())
        } else if self.registered.is_none() {
            self.change(r, chosen)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// The system's registrations: chords by id; `refused` chords cannot be registered.
    #[derive(Default)]
    struct System {
        active: RefCell<Vec<(u32, Shortcut)>>,
        next: RefCell<u32>,
        refused: Vec<Shortcut>,
    }

    impl Registrar for System {
        type Id = u32;
        fn register(&self, shortcut: &Shortcut) -> Result<u32, ShortcutError> {
            if self.refused.contains(shortcut) || self.active.borrow().iter().any(|(_, s)| s == shortcut) {
                return Err(ShortcutError::Conflict);
            }
            let id = *self.next.borrow();
            *self.next.borrow_mut() += 1;
            self.active.borrow_mut().push((id, shortcut.clone()));
            Ok(id)
        }
        fn unregister(&self, id: u32) {
            let mut active = self.active.borrow_mut();
            let before = active.len();
            active.retain(|(i, _)| *i != id);
            assert_eq!(active.len() + 1, before, "released once, after being registered");
        }
    }

    impl System {
        fn active(&self) -> Vec<String> {
            self.active.borrow().iter().map(|(_, s)| s.to_string()).collect()
        }
    }

    fn sc(s: &str) -> Shortcut {
        s.parse().expect("shortcut")
    }

    #[test]
    fn the_active_shortcut_is_released_while_the_field_captures_and_comes_back_after() {
        let system = System::default();
        let mut rec = RecordShortcut::default();
        rec.change(&system, Some(&sc("cmd+shift+r"))).unwrap();
        assert_eq!(system.active(), ["shift+super+r"]);

        rec.follow_capture(&system, true, Some(&sc("cmd+shift+r"))).unwrap();
        assert!(system.active().is_empty(), "⇧⌘R reaches the field");
        rec.follow_capture(&system, true, Some(&sc("cmd+shift+r"))).unwrap();
        assert!(system.active().is_empty());

        // Escape, or the window losing focus: the same shortcut again.
        rec.follow_capture(&system, false, Some(&sc("cmd+shift+r"))).unwrap();
        assert_eq!(system.active(), ["shift+super+r"]);
        rec.follow_capture(&system, false, Some(&sc("cmd+shift+r"))).unwrap();
        assert_eq!(system.active(), ["shift+super+r"], "registered once");
    }

    #[test]
    fn a_shortcut_chosen_during_the_capture_is_the_one_registered_after() {
        let system = System::default();
        let mut rec = RecordShortcut::default();
        rec.change(&system, Some(&sc("ctrl+alt+cmd+r"))).unwrap();
        rec.follow_capture(&system, true, Some(&sc("ctrl+alt+cmd+r"))).unwrap();
        // Back to ⇧⌘R: the field calls `change`, then stops capturing with ⇧⌘R as its value.
        rec.change(&system, Some(&sc("cmd+shift+r"))).unwrap();
        rec.follow_capture(&system, false, Some(&sc("cmd+shift+r"))).unwrap();
        assert_eq!(system.active(), ["shift+super+r"]);
        assert!(rec.is_registered());
    }

    #[test]
    fn a_cleared_shortcut_stays_unregistered_after_the_capture() {
        let system = System::default();
        let mut rec = RecordShortcut::default();
        rec.change(&system, Some(&sc("cmd+shift+r"))).unwrap();
        rec.follow_capture(&system, true, Some(&sc("cmd+shift+r"))).unwrap();
        rec.change(&system, None).unwrap();
        rec.follow_capture(&system, false, None).unwrap();
        assert!(system.active().is_empty());
        assert!(!rec.is_registered());
    }

    #[test]
    fn a_refused_shortcut_keeps_the_old_one() {
        let system = System { refused: vec![sc("cmd+space")], ..System::default() };
        let mut rec = RecordShortcut::default();
        rec.change(&system, Some(&sc("cmd+shift+r"))).unwrap();
        // Outside a capture: the old registration stays.
        assert_eq!(rec.change(&system, Some(&sc("cmd+space"))), Err(ShortcutError::Conflict));
        assert_eq!(system.active(), ["shift+super+r"]);

        // During a capture: refused, the field keeps capturing; cancelled, the old value is back.
        rec.follow_capture(&system, true, Some(&sc("cmd+shift+r"))).unwrap();
        assert_eq!(rec.change(&system, Some(&sc("cmd+space"))), Err(ShortcutError::Conflict));
        assert!(system.active().is_empty());
        rec.follow_capture(&system, false, Some(&sc("cmd+shift+r"))).unwrap();
        assert_eq!(system.active(), ["shift+super+r"]);
    }

    #[test]
    fn a_shortcut_refused_when_coming_back_leaves_nothing_registered() {
        let system = System::default();
        let mut rec = RecordShortcut::default();
        rec.change(&system, Some(&sc("cmd+shift+r"))).unwrap();
        rec.follow_capture(&system, true, Some(&sc("cmd+shift+r"))).unwrap();
        // Another app took ⇧⌘R in the meantime.
        let other = system.register(&sc("cmd+shift+r")).unwrap();
        assert!(rec.follow_capture(&system, false, Some(&sc("cmd+shift+r"))).is_err());
        assert!(!rec.is_registered());
        system.unregister(other);
        // The next capture and cancel try again.
        rec.follow_capture(&system, true, Some(&sc("cmd+shift+r"))).unwrap();
        rec.follow_capture(&system, false, Some(&sc("cmd+shift+r"))).unwrap();
        assert_eq!(system.active(), ["shift+super+r"]);
    }
}
