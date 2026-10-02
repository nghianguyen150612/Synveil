#![forbid(unsafe_code)]
//! Platform-neutral, in-memory installation lifecycle policy.

mod adapter;
// AppImage integration is a Linux-only capability: it owns XDG surfaces, a
// systemd user unit and POSIX ownership/mode checks, all of which are Unix
// APIs. The module is gated off rather than stubbed on other platforms, so no
// non-Linux build can present a fake AppImage implementation that appears to
// succeed while performing no integration. Portable execution is the real
// cross-platform path: it calls nothing from this module.
#[cfg(target_os = "linux")]
mod appimage;
mod engine;
mod error_model;
mod journal;
mod lifecycle;
mod linux_package;
mod model;

pub use adapter::*;
#[cfg(target_os = "linux")]
pub use appimage::*;
pub use engine::*;
pub use error_model::*;
pub use journal::*;
pub use lifecycle::*;
pub use linux_package::*;
pub use model::*;

pub const ENGINE_SCHEMA_VERSION: u32 = 1;
