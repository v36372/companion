use crate::SqliteHub;
use rusqlite::params;

#[derive(Clone, Debug, PartialEq)]
pub struct Highlight {
    pub id: String,
    pub document_id: String,
    pub page_0: u32,
    pub quote_text: String,
    pub occurrence: u32,
    pub color: String,
    pub conversation_entry_id: Option<i64>,
}

impl SqliteHub {
    /// Returns false when this quote locator already exists (one color per quote).
    pub fn insert_highlight(&self, highlight: &Highlight) -> anyhow::Result<bool> {
        self.write(|connection| {
            Ok(connection.execute("INSERT OR IGNORE INTO highlights(id,document_id,page,quote_text,occurrence,color,conversation_entry_id) VALUES (?1,?2,?3,?4,?5,?6,?7)", params![highlight.id, highlight.document_id, highlight.page_0, highlight.quote_text, highlight.occurrence, highlight.color, highlight.conversation_entry_id])? != 0)
        })
    }

    pub fn highlights(&self, document_id: &str) -> anyhow::Result<Vec<Highlight>> {
        Ok(self
            .highlights_with_rowids(document_id)?
            .into_iter()
            .map(|(_, highlight)| highlight)
            .collect())
    }

    /// SQLite rowid is the viewer's numeric identity; the UUID remains the app
    /// identity. Read both together so concurrent inserts cannot break the mapping.
    pub fn highlights_with_rowids(
        &self,
        document_id: &str,
    ) -> anyhow::Result<Vec<(u64, Highlight)>> {
        let connection = self.open_read_only()?;
        let mut statement = connection.prepare("SELECT rowid,id,document_id,page,quote_text,occurrence,color,conversation_entry_id FROM highlights WHERE document_id=?1 ORDER BY page,created_at,rowid")?;
        Ok(statement
            .query_map([document_id], |row| {
                Ok((
                    row.get::<_, i64>(0)? as u64,
                    Highlight {
                        id: row.get(1)?,
                        document_id: row.get(2)?,
                        page_0: row.get(3)?,
                        quote_text: row.get(4)?,
                        occurrence: row.get(5)?,
                        color: row.get(6)?,
                        conversation_entry_id: row.get(7)?,
                    },
                ))
            })?
            .collect::<rusqlite::Result<_>>()?)
    }
}
