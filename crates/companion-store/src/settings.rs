//! Non-secret UI/model/search preferences. API keys belong exclusively in SecretStore.
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

pub const DEFAULT_MODEL_ID: &str = "~anthropic/claude-haiku-latest";
pub const MODEL_CHOICES: &[(&str, &str)] = &[
    (DEFAULT_MODEL_ID, "Claude Haiku – fast"),
    ("~anthropic/claude-sonnet-latest", "Claude Sonnet – smarter"),
    ("openai/gpt-4o-mini", "GPT-4o mini"),
    ("google/gemini-2.5-flash", "Gemini 2.5 Flash"),
];

/// Return the canonical OpenRouter id, without accepting whitespace inside it.
pub fn validate_model_id(value: &str) -> anyhow::Result<&str> {
    let value = value.trim();
    anyhow::ensure!(
        !value.is_empty()
            && value.len() <= 128
            && value
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"~._:/-".contains(&c)),
        "Use 1–128 letters, digits or ~ . _ : / - (no spaces)."
    );
    Ok(value)
}

impl SqliteHub {
    pub fn sidebar_pinned(&self) -> anyhow::Result<bool> {
        self.write(|db| {
            let value: Option<String> = db
                .query_row(
                    "SELECT value_json FROM settings WHERE key='sidebar_pinned'",
                    [],
                    |row| row.get(0),
                )
                .optional()?;
            Ok(value
                .map(|value| serde_json::from_str(&value))
                .transpose()?
                .unwrap_or(false))
        })
    }

    pub fn set_sidebar_pinned(&self, pinned: bool) -> anyhow::Result<()> {
        self.write(|db| {
            db.execute("INSERT INTO settings(key,value_json) VALUES('sidebar_pinned',?1) ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json", [serde_json::to_string(&pinned)?])?;
            Ok(())
        })
    }

    pub fn sidebar_width(&self) -> anyhow::Result<f32> {
        self.write(|db| {
            let value: Option<String> = db
                .query_row(
                    "SELECT value_json FROM settings WHERE key='sidebar_width'",
                    [],
                    |row| row.get(0),
                )
                .optional()?;
            let width: f32 = value
                .map(|value| serde_json::from_str(&value))
                .transpose()?
                .unwrap_or(280.);
            anyhow::ensure!(width.is_finite(), "Invalid sidebar width");
            Ok(width.clamp(220., 360.))
        })
    }

    pub fn set_sidebar_width(&self, width: f32) -> anyhow::Result<()> {
        anyhow::ensure!(width.is_finite(), "Invalid sidebar width");
        self.write(|db| {
            db.execute("INSERT INTO settings(key,value_json) VALUES('sidebar_width',?1) ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json", [serde_json::to_string(&width.clamp(220., 360.))?])?;
            Ok(())
        })
    }

    pub fn model_id(&self) -> anyhow::Result<String> {
        self.write(|db| {
            let value: Option<String> = db
                .query_row(
                    "SELECT value_json FROM settings WHERE key='model_id'",
                    [],
                    |row| row.get(0),
                )
                .optional()?;
            let value = value
                .map(|v| serde_json::from_str::<String>(&v))
                .transpose()?;
            Ok(validate_model_id(value.as_deref().unwrap_or(DEFAULT_MODEL_ID))?.to_owned())
        })
    }

    pub fn set_model_id(&self, value: &str) -> anyhow::Result<()> {
        let value = validate_model_id(value)?;
        self.write(|db| {
            db.execute(
                "INSERT INTO settings(key,value_json) VALUES('model_id',?1) ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json",
                [serde_json::to_string(value)?],
            )?;
            Ok(())
        })
    }

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
    #[test]
    fn sidebar_preferences_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sidebar.sqlite");
        let hub = crate::SqliteHub::open(&path).unwrap();
        assert!(!hub.sidebar_pinned().unwrap());
        assert_eq!(hub.sidebar_width().unwrap(), 280.);
        hub.set_sidebar_pinned(true).unwrap();
        hub.set_sidebar_width(320.).unwrap();
        drop(hub);
        let hub = crate::SqliteHub::open(&path).unwrap();
        assert!(hub.sidebar_pinned().unwrap());
        assert_eq!(hub.sidebar_width().unwrap(), 320.);
        hub.set_sidebar_pinned(false).unwrap();
        hub.set_sidebar_width(999.).unwrap();
        assert!(!hub.sidebar_pinned().unwrap());
        assert_eq!(hub.sidebar_width().unwrap(), 360.);
        assert!(hub.set_sidebar_width(f32::NAN).is_err());
    }

    use crate::SqliteHub;

    #[test]
    fn model_default_round_trip_and_validation() {
        use super::DEFAULT_MODEL_ID;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("model.sqlite");
        let hub = SqliteHub::open(&path).unwrap();
        assert_eq!(hub.model_id().unwrap(), DEFAULT_MODEL_ID);
        hub.set_model_id("  vendor/custom-model:v1  ").unwrap();
        drop(hub);
        let hub = SqliteHub::open(&path).unwrap();
        assert_eq!(hub.model_id().unwrap(), "vendor/custom-model:v1");
        for invalid in [
            "",
            "  ",
            "a b",
            "a\tb",
            "a\nb",
            "a\0b",
            "é/model",
            "a?b",
            &"x".repeat(129),
        ] {
            assert!(hub.set_model_id(invalid).is_err(), "{invalid:?}");
            assert_eq!(hub.model_id().unwrap(), "vendor/custom-model:v1");
        }
        hub.set_model_id(&"x".repeat(128)).unwrap();
        assert_eq!(hub.model_id().unwrap().len(), 128);
    }

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
