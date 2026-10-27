//! Non-secret model preferences. API keys belong exclusively in SecretStore.
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
