//! Host-only credentials. Never pass this store or its values to WASI.
use std::{collections::HashMap, sync::Mutex};

pub const PROVIDERS: &[&str] = &["openrouter", "openai", "anthropic", "brave", "google"];
pub trait SecretStore: Send + Sync {
    fn read(&self, provider: &str) -> anyhow::Result<Option<String>>;
    fn write(&self, provider: &str, key: &str) -> anyhow::Result<()>;
    fn delete(&self, provider: &str) -> anyhow::Result<()>;
}

/// Native keychain with a host-only env fallback for headless deployments.
pub struct KeychainStore;
impl KeychainStore {
    fn entry(provider: &str) -> anyhow::Result<keyring::Entry> {
        anyhow::ensure!(PROVIDERS.contains(&provider), "unknown credential provider");
        Ok(keyring::Entry::new("learning-companion", provider)?)
    }
}
impl SecretStore for KeychainStore {
    fn read(&self, provider: &str) -> anyhow::Result<Option<String>> {
        let entry = Self::entry(provider)?;
        match entry.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(error) => {
                let name = format!("COMPANION_{}_API_KEY", provider.to_ascii_uppercase());
                if let Ok(value) = std::env::var(name) {
                    return Ok(Some(value));
                }
                if matches!(error, keyring::Error::NoEntry) {
                    Ok(None)
                } else {
                    // No service is normal on headless Linux. Do not expose backend
                    // diagnostics (which may contain identifying keychain metadata).
                    anyhow::bail!("keychain unavailable and no host credential fallback")
                }
            }
        }
    }
    fn write(&self, provider: &str, key: &str) -> anyhow::Result<()> {
        Self::entry(provider)?.set_password(key)?;
        Ok(())
    }
    fn delete(&self, provider: &str) -> anyhow::Result<()> {
        match Self::entry(provider)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => anyhow::bail!("keychain deletion failed"),
        }
    }
}

/// Explicitly supplied in tests; never consults the machine's credentials.
#[derive(Default)]
pub struct MemorySecretStore(Mutex<HashMap<String, String>>);
impl SecretStore for MemorySecretStore {
    fn read(&self, provider: &str) -> anyhow::Result<Option<String>> {
        Ok(self.0.lock().unwrap().get(provider).cloned())
    }
    fn write(&self, provider: &str, key: &str) -> anyhow::Result<()> {
        self.0.lock().unwrap().insert(provider.into(), key.into());
        Ok(())
    }
    fn delete(&self, provider: &str) -> anyhow::Result<()> {
        self.0.lock().unwrap().remove(provider);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn memory_store_is_explicit_and_deletable() {
        let store = MemorySecretStore::default();
        assert_eq!(store.read("openrouter").unwrap(), None);
        store.write("openrouter", "local-test-only").unwrap();
        assert_eq!(
            store.read("openrouter").unwrap().as_deref(),
            Some("local-test-only")
        );
        store.delete("openrouter").unwrap();
        assert_eq!(store.read("openrouter").unwrap(), None);
    }
}
