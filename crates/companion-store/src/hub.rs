use parking_lot::{Condvar, Mutex};
use rusqlite::{Connection, OpenFlags};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

struct Writer {
    connection: Connection,
    txn_depth: usize,
    txn_owner: Option<u64>,
}

/// One writer for app and guest tables. Never hold this lock across a
/// HostToGuest send-and-wait: commit the app operation first.
pub struct SqliteHub {
    path: PathBuf,
    writer: Mutex<Writer>,
    available: Condvar,
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
            writer: Mutex::new(Writer {
                connection,
                txn_depth: 0,
                txn_owner: None,
            }),
            available: Condvar::new(),
        })
    }

    /// App writes cannot accidentally join an on-stack guest transaction.
    pub fn write<T>(
        &self,
        operation: impl FnOnce(&mut Connection) -> anyhow::Result<T>,
    ) -> anyhow::Result<T> {
        self.access(None, operation)
    }

    fn access<T>(
        &self,
        owner: Option<u64>,
        operation: impl FnOnce(&mut Connection) -> anyhow::Result<T>,
    ) -> anyhow::Result<T> {
        let mut writer = self.writer.lock();
        while writer.txn_depth > 0 && writer.txn_owner != owner {
            self.available.wait(&mut writer);
        }
        operation(&mut writer.connection)
    }

    /// The owner identifies a Store, not a thread (Stores can be replaced).
    pub fn guest<T>(
        &self,
        owner: u64,
        operation: impl FnOnce(&mut Connection) -> anyhow::Result<T>,
    ) -> anyhow::Result<T> {
        self.access(Some(owner), operation)
    }

    pub fn begin(&self, owner: u64) -> anyhow::Result<()> {
        let mut writer = self.writer.lock();
        while writer.txn_depth > 0 && writer.txn_owner != Some(owner) {
            self.available.wait(&mut writer);
        }
        anyhow::ensure!(writer.txn_depth == 0, "nested guest transaction");
        writer.connection.execute_batch("BEGIN IMMEDIATE")?;
        writer.txn_owner = Some(owner);
        writer.txn_depth = 1;
        Ok(())
    }

    pub fn end(&self, owner: u64, commit: bool) -> anyhow::Result<()> {
        let mut writer = self.writer.lock();
        anyhow::ensure!(writer.txn_owner == Some(owner), "not transaction owner");
        writer
            .connection
            .execute_batch(if commit { "COMMIT" } else { "ROLLBACK" })?;
        writer.txn_depth = 0;
        writer.txn_owner = None;
        self.available.notify_all();
        Ok(())
    }

    /// Roll back an abandoned transaction before a trapped Store is replaced.
    pub fn release(&self, owner: u64) {
        let mut writer = self.writer.lock();
        if writer.txn_owner == Some(owner) {
            let _ = writer.connection.execute_batch("ROLLBACK");
            writer.txn_depth = 0;
            writer.txn_owner = None;
            self.available.notify_all();
        }
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
        let writer = self.writer.get_mut();
        if writer.txn_depth > 0 {
            let _ = writer.connection.execute_batch("ROLLBACK");
        }
        let _ = writer
            .connection
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
    }
}
