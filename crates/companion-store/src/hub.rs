use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use parking_lot::Mutex;
use rusqlite::{Connection, OpenFlags};

/// One writer for app and (eventually) guest tables in this process.
///
/// Never hold this lock across a HostToGuest send-and-wait. Commit first.
/// PR6 must add txn_depth/txn_owner + a Condvar before exposing multi-call
/// guest transactions: unrelated writers must wait outside an active guest tx.
pub struct SqliteHub {
    path: PathBuf,
    writer: Mutex<Connection>,
}

impl SqliteHub {
    pub fn open(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = path.as_ref().to_path_buf();
        let mut connection = Connection::open(&path)?;
        connection.busy_timeout(Duration::from_millis(5000))?;
        connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA foreign_keys=ON; PRAGMA wal_autocheckpoint=1000;")?;
        crate::schema::migrate(&mut connection)?;
        Ok(Self {
            path,
            writer: Mutex::new(connection),
        })
    }

    /// A short, serialized writer operation. Do not retain the connection.
    pub fn write<T>(
        &self,
        operation: impl FnOnce(&mut Connection) -> anyhow::Result<T>,
    ) -> anyhow::Result<T> {
        operation(&mut self.writer.lock())
    }

    pub fn open_read_only(&self) -> anyhow::Result<Connection> {
        let connection = Connection::open_with_flags(
            &self.path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        connection.busy_timeout(Duration::from_millis(5000))?;
        connection.execute_batch("PRAGMA query_only=ON; PRAGMA foreign_keys=ON;")?;
        Ok(connection)
    }
}

impl Drop for SqliteHub {
    fn drop(&mut self) {
        let _ = self
            .writer
            .get_mut()
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
    }
}
