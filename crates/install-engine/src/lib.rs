#![forbid(unsafe_code)]
//! Platform-neutral, in-memory installation lifecycle policy.

mod adapter;
mod appimage;
mod engine;
mod error_model;
mod journal;
mod lifecycle;
mod linux_package;
mod model;

pub use adapter::*;
pub use appimage::*;
pub use engine::*;
pub use error_model::*;
pub use journal::*;
pub use lifecycle::*;
pub use linux_package::*;
pub use model::*;

pub const ENGINE_SCHEMA_VERSION: u32 = 1;
