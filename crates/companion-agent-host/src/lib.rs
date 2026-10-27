//! PR4: synchronous, on-stack ComponentizeJS streaming spike.
//! Default builds do not require Node or the generated component.
#[cfg(feature = "agent")]
pub mod http_proxy;
#[cfg(feature = "agent")]
pub mod thread;
#[cfg(feature = "agent")]
pub mod wasm;
