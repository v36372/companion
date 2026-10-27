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
        let connection = self.open_read_only()?;
        let mut statement = connection.prepare("SELECT id,document_id,page,quote_text,occurrence,color,conversation_entry_id FROM highlights WHERE document_id=?1 ORDER BY page,created_at,rowid")?;
        Ok(statement
            .query_map([document_id], |row| {
                Ok(Highlight {
                    id: row.get(0)?,
                    document_id: row.get(1)?,
                    page_0: row.get(2)?,
                    quote_text: row.get(3)?,
                    occurrence: row.get(4)?,
                    color: row.get(5)?,
                    conversation_entry_id: row.get(6)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?)
    }
}
