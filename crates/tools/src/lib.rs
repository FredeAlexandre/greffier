//! Greffier's real adapters that drive Mac programs. The core only knows their interfaces
//! ([`greffier_core::adapters`]):
//!
//! - [`claude`]: Greffier's [`Agent`](greffier_core::adapters::Agent), the `claude` CLI
//!   (two-pass review with relisten, summary written by the agent).
//! - [`mac`]: the resources' [`Tools`](greffier_core::resources::Tools) (curl, sha256, bundled
//!   uv, `claude` check) and the private Python's environment.
//! - [`subprocess`]: what they share (and `greffier-engines` with them) to run a job on a thread
//!   and wait for a program that can be stopped.

pub mod claude;
pub mod mac;
pub mod subprocess;
