#![forbid(unsafe_code)]
//! Platform-neutral, in-memory installation lifecycle policy.

mod adapter;
mod engine;
mod journal;
mod model;

pub use adapter::*;
pub use engine::*;
pub use journal::*;
pub use model::*;

pub const ENGINE_SCHEMA_VERSION: u32 = 1;
