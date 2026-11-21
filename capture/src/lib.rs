//! La vraie capture de Greffier, sur macOS : ScreenCaptureKit pour l'écran, le son du Mac et le
//! micro ; AVAssetWriter pour l'écriture en MP4 fragmenté ; l'assertion de non-veille d'IOKit
//! ([`veille::PasDeVeille`]) qui garde le Mac éveillé pendant un enregistrement ou un traitement.
//! La capture implémente
//! [`greffier_coeur::adaptateurs::Capture`]. Elle n'est pas testée par la suite automatique
//! (spec : les vrais adaptateurs sont couverts par l'usage), sauf le mixage, qui est du calcul.

pub mod mixage;

#[cfg(target_os = "macos")]
pub mod ecrivain;
#[cfg(target_os = "macos")]
mod mac;
#[cfg(target_os = "macos")]
pub mod veille;
#[cfg(target_os = "macos")]
pub use mac::{CaptureDuMac, taille_des_images};
