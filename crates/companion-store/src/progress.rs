use crate::SqliteHub;
use rusqlite::{OptionalExtension, params};

#[derive(Clone, Debug, PartialEq)]
pub struct ReadingProgress {
    pub page_0: u32,
    pub scroll_frac: f64,
    pub viewport_excerpt: Option<String>,
}

impl SqliteHub {
    pub fn progress(&self, document_id: &str) -> anyhow::Result<Option<ReadingProgress>> {
        Ok(self.open_read_only()?.query_row("SELECT page,scroll_frac,viewport_excerpt FROM reading_progress WHERE document_id=?1", [document_id], |row| Ok(ReadingProgress { page_0: row.get(0)?, scroll_frac: row.get(1)?, viewport_excerpt: row.get(2)? })).optional()?)
    }

    pub fn set_progress(
        &self,
        document_id: &str,
        progress: &ReadingProgress,
    ) -> anyhow::Result<()> {
        self.write(|connection| set_on_connection(connection, document_id, progress))
    }
}

/// Save under an already-held hub writer lock. This lets debounced callers
/// validate their generation token atomically with the write. Do not call a
/// hub method from inside `SqliteHub::write` (the writer is not reentrant).
pub fn set_on_connection(
    connection: &rusqlite::Connection,
    document_id: &str,
    progress: &ReadingProgress,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        progress.scroll_frac.is_finite() && (0.0..=1.0).contains(&progress.scroll_frac),
        "scroll fraction must be between 0 and 1"
    );
    connection.execute("INSERT INTO reading_progress(document_id,page,scroll_frac,viewport_excerpt) VALUES (?1,?2,?3,?4) ON CONFLICT(document_id) DO UPDATE SET page=excluded.page,scroll_frac=excluded.scroll_frac,viewport_excerpt=excluded.viewport_excerpt,updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now')", params![document_id, progress.page_0, progress.scroll_frac, progress.viewport_excerpt])?;
    Ok(())
}
