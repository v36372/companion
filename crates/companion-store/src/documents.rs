use std::path::Path;

use rusqlite::{OptionalExtension, Row, params};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::SqliteHub;

#[derive(Clone, Debug, PartialEq)]
pub struct Document {
    pub id: String,
    pub title: String,
    pub source_path: String,
    pub stored_path: String,
    pub sha256: String,
    pub page_count: Option<u32>,
}

fn from_row(row: &Row<'_>) -> rusqlite::Result<Document> {
    Ok(Document {
        id: row.get(0)?,
        title: row.get(1)?,
        source_path: row.get(2)?,
        stored_path: row.get(3)?,
        sha256: row.get(4)?,
        page_count: row.get(5)?,
    })
}

pub fn sha256(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

impl SqliteHub {
    pub fn document_by_sha256(&self, sha256: &str) -> anyhow::Result<Option<Document>> {
        Ok(self.open_read_only()?.query_row("SELECT id,title,source_path,stored_path,sha256,page_count FROM documents WHERE sha256=?1", [sha256], from_row).optional()?)
    }

    /// Identity conflicts return the original row, without changing its UUID/path.
    pub fn upsert_document(&self, document: &Document) -> anyhow::Result<Document> {
        self.write(|connection| {
            connection.execute("INSERT INTO documents(id,title,source_path,stored_path,sha256,page_count) VALUES (?1,?2,?3,?4,?5,?6) ON CONFLICT(sha256) DO NOTHING", params![document.id, document.title, document.source_path, document.stored_path, document.sha256, document.page_count])?;
            Ok(connection.query_row("SELECT id,title,source_path,stored_path,sha256,page_count FROM documents WHERE sha256=?1", [&document.sha256], from_row)?)
        })
    }

    /// Snapshot the exact bytes hashed, before any viewer parses them. Failed DB
    /// insertion removes the new copy; an existing identity reuses its stored path.
    pub fn import_pdf(&self, source: &Path, data_dir: &Path) -> anyhow::Result<Document> {
        let bytes = std::fs::read(source)?;
        let hash = sha256(&bytes);
        if let Some(document) = self.document_by_sha256(&hash)? {
            // Repair a missing managed copy using the verified source bytes.
            if !Path::new(&document.stored_path).exists() {
                std::fs::write(&document.stored_path, &bytes)?;
            }
            return Ok(document);
        }
        let id = Uuid::new_v4().to_string();
        let docs = data_dir.join("docs");
        std::fs::create_dir_all(&docs)?;
        let stored = docs.join(format!("{id}.pdf"));
        std::fs::write(&stored, bytes)?;
        let candidate = Document {
            id,
            title: source
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            source_path: source.to_string_lossy().into_owned(),
            stored_path: stored.to_string_lossy().into_owned(),
            sha256: hash,
            page_count: None,
        };
        match self.upsert_document(&candidate) {
            Ok(document) => {
                if document.id != candidate.id {
                    let _ = std::fs::remove_file(&stored);
                }
                Ok(document)
            }
            Err(error) => {
                let _ = std::fs::remove_file(&stored);
                Err(error)
            }
        }
    }

    pub fn set_page_count(&self, document_id: &str, page_count: u32) -> anyhow::Result<()> {
        self.write(|connection| {
            connection.execute(
                "UPDATE documents SET page_count=?2 WHERE id=?1",
                params![document_id, page_count],
            )?;
            Ok(())
        })
    }

    pub fn touch_last_opened(&self, document_id: &str) -> anyhow::Result<()> {
        self.write(|connection| {
            connection.execute("UPDATE documents SET last_opened_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?1", [document_id])?;
            Ok(())
        })
    }

    pub fn recent(&self, limit: u32) -> anyhow::Result<Vec<Document>> {
        let connection = self.open_read_only()?;
        let mut statement = connection.prepare("SELECT id,title,source_path,stored_path,sha256,page_count FROM documents WHERE last_opened_at IS NOT NULL ORDER BY last_opened_at DESC, rowid DESC LIMIT ?1")?;
        Ok(statement
            .query_map([limit], from_row)?
            .collect::<rusqlite::Result<_>>()?)
    }
}
