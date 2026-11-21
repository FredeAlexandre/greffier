//! L'icône dans la barre des menus de macOS : un `NSStatusItem` et son `NSMenu`, construits et
//! changés depuis le thread principal. Tout `unsafe` de la barre est ici.

use std::cell::RefCell;
use std::time::{Duration, Instant};

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{NSColor, NSImage, NSMenu, NSMenuItem, NSStatusBar, NSStatusItem, NSVariableStatusItemLength};
use objc2_foundation::{NSRunLoop, NSRunLoopCommonModes, NSString, NSTimer};

use crate::{Choix, Etat, Icone, Ligne, menu, texte_de_la_duree};

const CHOIX: [Choix; 4] = [Choix::Demarrer, Choix::Arreter, Choix::Ouvrir, Choix::Quitter];

/// La cible des lignes du menu et de la minuterie de la durée.
struct Cible {
    choisir: Box<dyn Fn(Choix)>,
    /// La ligne de la durée et l'instant où l'enregistrement a commencé, tant qu'il tourne.
    duree: RefCell<Option<(Retained<NSMenuItem>, Instant)>>,
}

define_class!(
    // SAFETY : NSObject n'a pas d'exigence de sous-classe ; la classe ne surcharge pas dealloc.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "GreffierCibleDeLaBarre"]
    #[ivars = Cible]
    struct CibleDeLaBarre;

    impl CibleDeLaBarre {
        #[unsafe(method(choisir:))]
        fn choisir(&self, ligne: &NSMenuItem) {
            if let Some(&c) = CHOIX.get(ligne.tag() as usize) {
                (self.ivars().choisir)(c);
            }
        }

        /// La durée avance même quand l'app ne la redonne pas (menu ouvert, fenêtre fermée).
        #[unsafe(method(battre:))]
        fn battre(&self, _minuterie: &NSTimer) {
            if let Some((ligne, debut)) = &*self.ivars().duree.borrow() {
                ligne.setTitle(&NSString::from_str(&texte_de_la_duree(debut.elapsed().as_secs_f64())));
            }
        }
    }
);

/// L'icône de Greffier dans la barre des menus. Elle disparaît quand on la lâche.
pub struct IconeDeLaBarre {
    mtm: MainThreadMarker,
    item: Retained<NSStatusItem>,
    menu: Retained<NSMenu>,
    cible: Retained<CibleDeLaBarre>,
    minuterie: Option<Retained<NSTimer>>,
    /// La forme du menu affiché (l'état sans la durée) : on ne le reconstruit que si elle change.
    forme: Option<Option<bool>>,
}

impl IconeDeLaBarre {
    /// Pose l'icône. `choisir` reçoit les choix du menu, sur le thread principal. `None` hors du
    /// thread principal.
    pub fn new(choisir: impl Fn(Choix) + 'static) -> Option<Self> {
        let mtm = MainThreadMarker::new()?;
        let item = NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
        let menu = NSMenu::new(mtm);
        menu.setAutoenablesItems(false);
        item.setMenu(Some(&menu));
        let cible = CibleDeLaBarre::alloc(mtm).set_ivars(Cible { choisir: Box::new(choisir), duree: RefCell::new(None) });
        // SAFETY : `init` de NSObject, sur une instance tout juste allouée.
        let cible: Retained<CibleDeLaBarre> = unsafe { msg_send![super(cible), init] };
        Some(IconeDeLaBarre { mtm, item, menu, cible, minuterie: None, forme: None })
    }

    /// Montre l'état de l'app : l'aspect de l'icône, les lignes du menu, la durée.
    pub fn montrer(&mut self, etat: Etat) {
        let forme = match etat {
            Etat::Pret { possible } => Some(possible),
            Etat::Enregistrement { .. } => None,
        };
        let m = menu(etat);
        if self.forme != Some(forme) {
            self.forme = Some(forme);
            self.dessiner_l_icone(m.icone);
            self.remplir(&m.lignes);
            self.minuter(m.icone == Icone::Enregistrement);
        }
        if let Etat::Enregistrement { duree } = etat
            && let Some((ligne, debut)) = &mut *self.cible.ivars().duree.borrow_mut()
        {
            *debut = Instant::now() - Duration::from_secs_f64(duree.max(0.0));
            ligne.setTitle(&NSString::from_str(&texte_de_la_duree(duree)));
        }
    }

    fn dessiner_l_icone(&self, icone: Icone) {
        let Some(bouton) = self.item.button(self.mtm) else { return };
        let (symbole, description, teinte) = match icone {
            Icone::Repos => ("waveform", "Greffier", None),
            Icone::Enregistrement => ("record.circle.fill", "Greffier : enregistrement en cours", Some(NSColor::systemRedColor())),
        };
        match NSImage::imageWithSystemSymbolName_accessibilityDescription(&NSString::from_str(symbole), Some(&NSString::from_str(description))) {
            Some(image) => {
                image.setTemplate(true);
                bouton.setImage(Some(&image));
                bouton.setTitle(&NSString::new());
            }
            None => {
                bouton.setImage(None);
                bouton.setTitle(&NSString::from_str(if icone == Icone::Repos { "Greffier" } else { "● Greffier" }));
            }
        }
        bouton.setContentTintColor(teinte.as_deref());
    }

    fn remplir(&self, lignes: &[Ligne]) {
        self.menu.removeAllItems();
        let mut duree = None;
        for l in lignes {
            let item = match l {
                Ligne::Separateur => NSMenuItem::separatorItem(self.mtm),
                Ligne::Info(texte) => {
                    let item = self.ligne(texte, None);
                    item.setEnabled(false);
                    duree = Some(item.clone());
                    item
                }
                Ligne::Choix { texte, choix, active } => {
                    let item = self.ligne(texte, Some(*choix));
                    item.setEnabled(*active);
                    item
                }
            };
            self.menu.addItem(&item);
        }
        *self.cible.ivars().duree.borrow_mut() = duree.map(|l| (l, Instant::now()));
    }

    fn ligne(&self, texte: &str, choix: Option<Choix>) -> Retained<NSMenuItem> {
        let action = choix.map(|_| sel!(choisir:));
        // SAFETY : `choisir:` existe sur la cible et prend l'NSMenuItem qui l'envoie.
        let item = unsafe { NSMenuItem::initWithTitle_action_keyEquivalent(NSMenuItem::alloc(self.mtm), &NSString::from_str(texte), action, &NSString::new()) };
        if let Some(c) = choix {
            item.setTag(CHOIX.iter().position(|&x| x == c).unwrap_or(0) as isize);
            // SAFETY : la cible vit autant que l'icône, qui garde le menu.
            unsafe { item.setTarget(Some(&self.cible as &AnyObject)) };
        }
        item
    }

    /// Une minuterie, dans tous les modes de la boucle (aussi quand le menu est ouvert), le temps
    /// de l'enregistrement.
    fn minuter(&mut self, enregistrement: bool) {
        match (enregistrement, &self.minuterie) {
            (true, None) => {
                // SAFETY : `battre:` existe sur la cible et prend la minuterie ; elle tourne sur
                // la boucle du thread principal, celle de la cible.
                let t = unsafe { NSTimer::timerWithTimeInterval_target_selector_userInfo_repeats(0.5, &self.cible, sel!(battre:), None, true) };
                // SAFETY : ajoutée à la boucle principale, depuis le thread principal.
                unsafe { NSRunLoop::mainRunLoop().addTimer_forMode(&t, NSRunLoopCommonModes) };
                self.minuterie = Some(t);
            }
            (false, Some(t)) => {
                t.invalidate();
                self.minuterie = None;
            }
            _ => {}
        }
    }
}

impl Drop for IconeDeLaBarre {
    fn drop(&mut self) {
        if let Some(t) = self.minuterie.take() {
            t.invalidate();
        }
        NSStatusBar::systemStatusBar().removeStatusItem(&self.item);
    }
}
