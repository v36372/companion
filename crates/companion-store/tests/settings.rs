use companion_store::{
    SqliteHub,
    settings::{ModelSettings, Provider},
};

#[test]
fn model_preferences_are_atomic_non_secret_rows() {
    let directory = tempfile::tempdir().unwrap();
    let hub = SqliteHub::open(directory.path().join("settings.sqlite")).unwrap();
    assert_eq!(hub.model_settings().unwrap(), None);
    let settings = ModelSettings {
        provider: Provider::Openrouter,
        model_id: "chosen/catalog-model".into(),
    };
    hub.set_model_settings(&settings).unwrap();
    assert_eq!(hub.model_settings().unwrap(), Some(settings.clone()));
    assert!(
        hub.set_model_settings(&ModelSettings {
            model_id: "".into(),
            ..settings
        })
        .is_err()
    );
    hub.write(|db| {
        let count: i64 = db.query_row(
            "SELECT COUNT(*) FROM settings WHERE key IN ('provider','modelId')",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(count, 2);
        Ok(())
    })
    .unwrap();
}
