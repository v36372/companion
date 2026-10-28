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
    fn lookup_error(provider: &str, error: &keyring::Error) -> String {
        let store = if cfg!(target_os = "macos") {
            "macOS Keychain"
        } else if cfg!(target_os = "windows") {
            "Windows Credential Manager"
        } else {
            "Secret Service (D-Bus)"
        };
        // Describe the category, never backend metadata or credential bytes.
        let cause = match error {
            keyring::Error::NoStorageAccess(_) => "access denied or store locked",
            keyring::Error::BadEncoding(_) => "stored credential is not valid UTF-8",
            keyring::Error::Ambiguous(_) => "multiple matching credentials",
            keyring::Error::Invalid(_, _) => "invalid credential entry",
            _ => "secure storage unavailable",
        };
        format!(
            "{store} lookup for learning-companion/{provider} failed: {cause}. Unlock/allow access to the store (on Linux, start an unlocked Secret Service session), or set COMPANION_{}_API_KEY in the launching environment and restart. See Set up key for storage commands.",
            provider.to_ascii_uppercase()
        )
    }

    fn entry(provider: &str) -> anyhow::Result<keyring::Entry> {
        anyhow::ensure!(PROVIDERS.contains(&provider), "unknown credential provider");
        Ok(keyring::Entry::new("learning-companion", provider)?)
    }
}
impl SecretStore for KeychainStore {
    fn read(&self, provider: &str) -> anyhow::Result<Option<String>> {
        anyhow::ensure!(PROVIDERS.contains(&provider), "unknown credential provider");
        let result = keyring::Entry::new("learning-companion", provider)
            .and_then(|entry| entry.get_password());
        match result {
            Ok(value) => Ok(Some(value)),
            Err(error) => {
                let name = format!("COMPANION_{}_API_KEY", provider.to_ascii_uppercase());
                if let Ok(value) = std::env::var(name) {
                    return Ok(Some(value));
                }
                if matches!(error, keyring::Error::NoEntry) {
                    Ok(None)
                } else {
                    anyhow::bail!(Self::lookup_error(provider, &error))
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
    fn keychain_diagnostic_names_store_and_remedy_without_metadata() {
        let error = keyring::Error::BadEncoding(b"private credential bytes".to_vec());
        let message = KeychainStore::lookup_error("openrouter", &error);
        assert!(message.contains("learning-companion/openrouter"));
        assert!(message.contains("COMPANION_OPENROUTER_API_KEY"));
        assert!(message.contains("restart"));
        assert!(message.contains("not valid UTF-8"));
        assert!(!message.contains("private credential bytes"));
    }

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
