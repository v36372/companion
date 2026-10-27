//! App-owned catalog. SQL page indices are always zero-based.
pub mod documents;
pub mod highlights;
mod hub;
pub mod progress;
mod schema;
pub mod settings;

pub use hub::SqliteHub;
