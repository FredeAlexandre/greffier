//! Greffier's real capture, on macOS: ScreenCaptureKit for the screen, the Mac audio and the
//! microphone; AVAssetWriter for writing fragmented MP4; the IOKit sleep assertion
//! ([`sleep::KeepAwake`]) that keeps the Mac awake during a recording or processing; the Mac's
//! Trash ([`MacTrash`]), where deleted meetings go; MP4 muxing of imported media
//! ([`MacMp4Muxer`]); the notification permission ([`notifications`]).
//! The capture implements
//! [`greffier_core::adapters::Capture`]. `cargo test` only covers the mixing, which is
//! computation; the rest is covered by the smoke tests (`tests/real_capture.rs`, run on purpose
//! on the Mac with `cargo xtask smoke`).

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
pub mod notifications;
#[cfg(target_os = "macos")]
pub use trash::MacTrash;
#[cfg(target_os = "macos")]
pub use mp4_muxing::MacMp4Muxer;
#[cfg(target_os = "macos")]
pub use mac::{MacCapture, image_size};
