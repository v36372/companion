use rusqlite::{Connection, OptionalExtension};

pub(crate) fn migrate(connection: &mut Connection) -> anyhow::Result<()> {
    let transaction = connection.transaction()?;
    let exists: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='schema_migrations')",
        [], |row| row.get(0),
    )?;
    let applied = exists
        && transaction
            .query_row(
                "SELECT version FROM schema_migrations WHERE version=1",
                [],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .is_some();
    if !applied {
        transaction.execute_batch(include_str!("migrations/0001_init.sql"))?;
        transaction.execute("INSERT INTO schema_migrations(version) VALUES (1)", [])?;
    }
    transaction.commit()?;
    Ok(())
}
