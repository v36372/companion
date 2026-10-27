//! Synchronous, on-stack ComponentizeJS agent runtime.
//! Default builds do not require Node or the generated component.
#[cfg(feature = "agent")]
pub mod http_proxy;
pub mod secrets;

/// WASM is the only shipping runtime; `off` prevents loading the component.
pub fn agent_enabled() -> anyhow::Result<bool> {
    match std::env::var("COMPANION_AGENT").as_deref() {
        Err(std::env::VarError::NotPresent) | Ok("wasm") => Ok(true),
        Ok("off") => Ok(false),
        _ => anyhow::bail!("COMPANION_AGENT must be wasm or off"),
    }
}
#[cfg(feature = "agent")]
pub mod thread;
#[cfg(feature = "agent")]
pub mod wasm;
