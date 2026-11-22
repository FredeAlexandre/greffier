// The pinned whisper.cpp, read by build.rs and by `cargo xtask vendor-whisper` (`include!`).
// To change version: update the three values, then run `cargo xtask vendor-whisper`.

/// The whisper.cpp release whose sources are vendored into `whisper.cpp/`.
pub const WHISPER_VERSION: &str = "1.9.5";
/// The sha256 of that release's archive on GitHub.
pub const WHISPER_ARCHIVE_SHA256: &str = "ff1a9053feb509ff9d7729703355541ae9690073a6b1c40eb692c962e0dc1720";
/// The ggml version that release ships (`ggml/CMakeLists.txt`).
pub const GGML_VERSION: &str = "0.26.0";
