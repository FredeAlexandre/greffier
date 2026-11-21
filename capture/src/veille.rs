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
    fn IOPMAssertionCreateWithName(genre: &CFString, niveau: u32, nom: &CFString, id: *mut u32) -> i32;
    fn IOPMAssertionRelease(id: u32) -> i32;
}

/// `kIOPMAssertionLevelOn`.
const ACTIVE: u32 = 255;

/// Le Mac reste éveillé tant que cette valeur vit.
#[derive(Debug)]
pub struct PasDeVeille {
    id: u32,
    raison: String,
    ecran: bool,
}

impl PasDeVeille {
    /// Empêche la veille automatique du Mac, et de l'écran avec `ecran` ; `raison` est le nom de
    /// l'assertion (« Greffier enregistre une réunion »). `Err` si macOS la refuse.
    pub fn new(raison: &str, ecran: bool) -> Result<PasDeVeille, String> {
        let genre = CFString::from_str(if ecran { "PreventUserIdleDisplaySleep" } else { "PreventUserIdleSystemSleep" });
        let nom = CFString::from_str(raison);
        let mut id = 0u32;
        // SAFETY : deux CFString valides le temps de l'appel, `id` est un u32 à nous.
        let code = unsafe { IOPMAssertionCreateWithName(&genre, ACTIVE, &nom, &mut id) };
        if code != 0 {
            return Err(format!("assertion de non-veille refusée (IOKit {code:#x})"));
        }
        Ok(PasDeVeille { id, raison: raison.into(), ecran })
    }

    pub fn raison(&self) -> &str {
        &self.raison
    }

    /// L'écran reste allumé aussi.
    pub fn ecran(&self) -> bool {
        self.ecran
    }
}

impl Drop for PasDeVeille {
    fn drop(&mut self) {
        // SAFETY : `id` vient d'IOPMAssertionCreateWithName et n'est relâché qu'ici.
        unsafe { IOPMAssertionRelease(self.id) };
    }
}
