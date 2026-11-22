//! [`PasDeVeille`] : une assertion de non-veille d'IOKit, tenue le temps d'un enregistrement ou
//! d'un traitement. Le Mac ne se met plus en veille quand on ne le touche plus
//! (`PreventUserIdleSystemSleep`) ; avec `ecran`, l'écran ne s'éteint pas non plus
//! (`PreventUserIdleDisplaySleep`, qui garde aussi le Mac éveillé) : il le faut pendant un
//! enregistrement, ScreenCaptureKit ne capture plus rien une fois l'écran éteint. Fermer le capot
//! met toujours le Mac en veille. L'assertion est relâchée quand la valeur est lâchée (et par
//! macOS si l'app meurt). Elle se voit dans `pmset -g assertions`, au nom de la raison donnée.

use objc2_core_foundation::CFString;

#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IOPMAssertionCreateWithName(kind: &CFString, level: u32, name: &CFString, id: *mut u32) -> i32;
    fn IOPMAssertionRelease(id: u32) -> i32;
}

/// `kIOPMAssertionLevelOn`.
const ACTIVE: u32 = 255;

/// Le Mac reste éveillé tant que cette valeur vit.
#[derive(Debug)]
pub struct KeepAwake {
    id: u32,
    reason: String,
    screen: bool,
}

impl KeepAwake {
    /// Empêche la veille automatique du Mac, et de l'écran avec `ecran` ; `raison` est le nom de
    /// l'assertion (« Greffier enregistre une réunion »). `Err` si macOS la refuse.
    pub fn new(reason: &str, screen: bool) -> Result<KeepAwake, String> {
        let kind = CFString::from_str(if screen { "PreventUserIdleDisplaySleep" } else { "PreventUserIdleSystemSleep" });
        let name = CFString::from_str(reason);
        let mut id = 0u32;
        // SAFETY : deux CFString valides le temps de l'appel, `id` est un u32 à nous.
        let code = unsafe { IOPMAssertionCreateWithName(&kind, ACTIVE, &name, &mut id) };
        if code != 0 {
            return Err(format!("assertion de non-veille refusée (IOKit {code:#x})"));
        }
        Ok(KeepAwake { id, reason: reason.into(), screen })
    }

    pub fn reason(&self) -> &str {
        &self.reason
    }

    /// L'écran reste allumé aussi.
    pub fn screen(&self) -> bool {
        self.screen
    }
}

impl Drop for KeepAwake {
    fn drop(&mut self) {
        // SAFETY : `id` vient d'IOPMAssertionCreateWithName et n'est relâché qu'ici.
        unsafe { IOPMAssertionRelease(self.id) };
    }
}
