//! [`PasDeVeille`] : une assertion de non-veille d'IOKit (`PreventUserIdleSystemSleep`), tenue
//! le temps d'un enregistrement ou d'un traitement. Le Mac ne se met plus en veille quand on ne
//! le touche plus ; l'écran, lui, peut encore s'éteindre, et fermer le capot met toujours le Mac
//! en veille. L'assertion est relâchée quand la valeur est lâchée (et par macOS si l'app meurt).
//! Elle se voit dans `pmset -g assertions`, au nom de la raison donnée.

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
}

impl PasDeVeille {
    /// Empêche la veille automatique ; `raison` est le nom de l'assertion (« Greffier enregistre
    /// une réunion »). `Err` si macOS la refuse.
    pub fn new(raison: &str) -> Result<PasDeVeille, String> {
        let genre = CFString::from_str("PreventUserIdleSystemSleep");
        let nom = CFString::from_str(raison);
        let mut id = 0u32;
        // SAFETY : deux CFString valides le temps de l'appel, `id` est un u32 à nous.
        let code = unsafe { IOPMAssertionCreateWithName(&genre, ACTIVE, &nom, &mut id) };
        if code != 0 {
            return Err(format!("assertion de non-veille refusée (IOKit {code:#x})"));
        }
        Ok(PasDeVeille { id, raison: raison.into() })
    }

    pub fn raison(&self) -> &str {
        &self.raison
    }
}

impl Drop for PasDeVeille {
    fn drop(&mut self) {
        // SAFETY : `id` vient d'IOPMAssertionCreateWithName et n'est relâché qu'ici.
        unsafe { IOPMAssertionRelease(self.id) };
    }
}
