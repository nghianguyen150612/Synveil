#![forbid(unsafe_code)]
//! Platform-neutral, in-memory installation lifecycle policy.

mod adapter;
mod engine;
mod journal;
mod lifecycle;
mod model;

pub use adapter::*;
pub use engine::*;
pub use journal::*;
pub use lifecycle::*;
pub use model::*;

pub const ENGINE_SCHEMA_VERSION: u32 = 1;
