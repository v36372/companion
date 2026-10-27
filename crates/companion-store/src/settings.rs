//! Non-secret model/search preferences. API keys belong exclusively in SecretStore.
use crate::SqliteHub;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Openrouter,
    Openai,
    Anthropic,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelSettings {
    pub provider: Provider,
    pub model_id: String,
}

impl SqliteHub {
    /// Search is enabled by default when a host credential exists. An explicit
    /// false persists the rollout opt-out; malformed settings fail closed.
    pub fn websearch_enabled(&self) -> anyhow::Result<bool> {
        self.write(|db| {
            let value: Option<String> = db
                .query_row(
                    "SELECT value_json FROM settings WHERE key='websearch_enabled'",
                    [],
                    |row| row.get(0),
                )
                .optional()?;
            Ok(value
                .map(|v| serde_json::from_str(&v))
                .transpose()?
                .unwrap_or(true))
        })
    }

    pub fn set_websearch_enabled(&self, enabled: bool) -> anyhow::Result<()> {
        self.write(|db| {
            db.execute(
                "INSERT INTO settings(key,value_json) VALUES('websearch_enabled',?1) ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json",
                [serde_json::to_string(&enabled)?],
            )?;
            Ok(())
        })
    }
    /// No guessed model default: caller chooses an id from the logged catalog.
    pub fn model_settings(&self) -> anyhow::Result<Option<ModelSettings>> {
        self.write(|db| {
            let provider: Option<String> = db
                .query_row(
                    "SELECT value_json FROM settings WHERE key='provider'",
                    [],
                    |row| row.get(0),
                )
                .optional()?;
            let model: Option<String> = db
                .query_row(
                    "SELECT value_json FROM settings WHERE key='modelId'",
                    [],
                    |row| row.get(0),
                )
                .optional()?;
            match (provider, model) {
                (Some(provider), Some(model)) => Ok(Some(ModelSettings {
                    provider: serde_json::from_str(&provider)?,
                    model_id: serde_json::from_str(&model)?,
                })),
                _ => Ok(None),
            }
        })
    }
    pub fn set_model_settings(&self, settings: &ModelSettings) -> anyhow::Result<()> {
        anyhow::ensure!(
            !settings.model_id.trim().is_empty() && settings.model_id.len() <= 256,
            "invalid model id"
        );
        self.write(|db| {
            let tx = db.transaction()?;
            for (key, value) in [("provider", serde_json::to_string(&settings.provider)?), ("modelId", serde_json::to_string(&settings.model_id)?)] {
                tx.execute("INSERT INTO settings(key,value_json) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json", rusqlite::params![key, value])?;
            }
            tx.commit()?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod search_tests {
    use crate::SqliteHub;

    #[test]
    fn search_opt_out_persists_and_invalid_values_fail_closed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.sqlite");
        let hub = SqliteHub::open(&path).unwrap();
        assert!(hub.websearch_enabled().unwrap());
        hub.set_websearch_enabled(false).unwrap();
        drop(hub);
        let hub = SqliteHub::open(&path).unwrap();
        assert!(!hub.websearch_enabled().unwrap());
        hub.set_websearch_enabled(true).unwrap();
        assert!(hub.websearch_enabled().unwrap());
        hub.write(|db| {
            db.execute(
                "UPDATE settings SET value_json='\"not a boolean\"' WHERE key='websearch_enabled'",
                [],
            )?;
            Ok(())
        })
        .unwrap();
        assert!(hub.websearch_enabled().is_err());
    }
}
