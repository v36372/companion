use crate::{FsError, FsResult};
use companion_store::SqliteHub;
use std::collections::BTreeMap;

pub(crate) fn load(hub: &SqliteHub, document: &str) -> FsResult<BTreeMap<String, Vec<u8>>> {
    let connection = hub.open_read_only().map_err(db_error)?;
    let mut statement = connection
        .prepare("SELECT path,content FROM notes WHERE document_id=?1")
        .map_err(db_error)?;
    statement
        .query_map([document], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(db_error)?
        .collect::<Result<_, _>>()
        .map_err(db_error)
}

// One transaction for rename/remove as well as writes; blobs are never encoded as text.
pub(crate) fn save(
    hub: &SqliteHub,
    document: &str,
    files: &BTreeMap<String, Vec<u8>>,
) -> FsResult<()> {
    hub.write(|connection| {
        let tx = connection.transaction()?;
        tx.execute("DELETE FROM notes WHERE document_id=?1", [document])?;
        for (path, bytes) in files.iter().filter(|(path, _)| path.starts_with("/notes/")) {
            tx.execute("INSERT INTO notes(document_id,path,content,mtime_ms) VALUES (?1,?2,?3,CAST(unixepoch('subsec')*1000 AS INTEGER))", rusqlite::params![document, path, bytes])?;
        }
        tx.commit()?;
        Ok(())
    }).map_err(db_error)
}

fn db_error(error: impl std::fmt::Display) -> FsError {
    FsError::new("unknown", error.to_string(), None)
}
