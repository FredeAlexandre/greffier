//! La vraie capture de Greffier, sur macOS : ScreenCaptureKit pour l'écran, le son du Mac et le
//! micro ; AVAssetWriter pour l'écriture en MP4 fragmenté ; l'assertion de non-veille d'IOKit
//! ([`veille::PasDeVeille`]) qui garde le Mac éveillé pendant un enregistrement ou un traitement ;
//! la corbeille du Mac ([`CorbeilleDuMac`]), où partent les réunions supprimées ; la mise en MP4
//! des médias importés ([`MiseEnMp4DuMac`]).
//! La capture implémente
//! [`greffier_coeur::adaptateurs::Capture`]. Elle n'est pas testée par la suite automatique
//! (spec : les vrais adaptateurs sont couverts par l'usage), sauf le mixage, qui est du calcul.

pub mod mixing;

#[cfg(target_os = "macos")]
mod trash;
#[cfg(target_os = "macos")]
pub mod writer;
#[cfg(target_os = "macos")]
mod mac;
#[cfg(target_os = "macos")]
mod mp4_muxing;
#[cfg(target_os = "macos")]
pub mod sleep;
#[cfg(target_os = "macos")]
pub use trash::MacTrash;
#[cfg(target_os = "macos")]
pub use mp4_muxing::MacMp4Muxer;
#[cfg(target_os = "macos")]
pub use mac::{MacCapture, image_size};
