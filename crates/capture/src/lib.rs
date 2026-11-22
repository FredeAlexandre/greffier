//! Greffier's real capture, on macOS: ScreenCaptureKit for the screen, the Mac audio and the
//! microphone; AVAssetWriter for writing fragmented MP4; the IOKit sleep assertion
//! ([`sleep::KeepAwake`]) that keeps the Mac awake during a recording or processing; the Mac's
//! Trash ([`MacTrash`]), where deleted meetings go; MP4 muxing of imported media
//! ([`MacMp4Muxer`]).
//! The capture implements
//! [`greffier_core::adapters::Capture`]. It is not tested by the automated suite (spec: the real
//! adapters are covered by use), except the mixing, which is computation.

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
