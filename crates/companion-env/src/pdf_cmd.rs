use crate::VirtualDocFs;
use companion_store::{SqliteHub, highlights::Highlight};
use crossbeam_channel::Sender;
use std::sync::Arc;

/// Pages have already crossed the 1-based WIT boundary here. Document identity
/// lets the UI discard queued commands from a document that has since closed.
#[derive(Clone, Debug, PartialEq)]
pub enum PdfCommand {
    Goto {
        document_id: String,
        page_0: u32,
        quote: Option<String>,
        occurrence: u32,
    },
    Highlight {
        document_id: String,
        id: u64,
    },
}
pub struct PdfCommands {
    pub fs: Arc<VirtualDocFs>,
    pub hub: Arc<SqliteHub>,
    pub tx: Sender<PdfCommand>,
}
impl PdfCommands {
    fn page(&self, page_1: u32) -> Result<u32, String> {
        let doc = self.fs.snapshot.as_ref().ok_or("no document")?;
        if page_1 == 0 || page_1 > doc.meta.page_count {
            return Err("invalid page".into());
        }
        Ok(page_1 - 1)
    }
    pub fn goto(&self, page_1: u32, quote: Option<String>, occurrence: u32) -> Result<(), String> {
        let page_0 = self.page(page_1)?;
        self.tx
            .send(PdfCommand::Goto {
                document_id: self.fs.snapshot.as_ref().unwrap().meta.id.clone(),
                page_0,
                quote,
                occurrence,
            })
            .map_err(|_| "viewer unavailable".into())
    }
    pub fn highlight(
        &self,
        page_1: u32,
        quote: String,
        occurrence: u32,
        color: Option<String>,
    ) -> Result<u64, String> {
        let page_0 = self.page(page_1)?;
        if quote.is_empty() {
            return Err("empty quote".into());
        }
        let doc = self.fs.snapshot.as_ref().unwrap();
        let highlight = Highlight {
            id: uuid::Uuid::new_v4().to_string(),
            document_id: doc.meta.id.clone(),
            page_0,
            quote_text: quote.clone(),
            occurrence,
            color: color.unwrap_or_else(|| "yellow".into()),
            conversation_entry_id: None,
        };
        let inserted = self
            .hub
            .insert_highlight(&highlight)
            .map_err(|e| e.to_string())?;
        let id = self
            .hub
            .highlights_with_rowids(&doc.meta.id)
            .map_err(|e| e.to_string())?
            .into_iter()
            .find(|(_, h)| {
                h.page_0 == page_0 && h.quote_text == quote && h.occurrence == occurrence
            })
            .ok_or("highlight disappeared")?
            .0;
        // Replays return the existing rowid without repainting or flashing.
        if inserted {
            let _ = self.tx.send(PdfCommand::Highlight {
                document_id: doc.meta.id.clone(),
                id,
            });
        }
        Ok(id)
    }
    pub fn search(&self, query: &str, max: u32) -> Result<String, String> {
        if query.is_empty() || max == 0 || max > 50 {
            return Err("invalid search".into());
        }
        let doc = self.fs.snapshot.as_ref().ok_or("no document")?;
        let mut hits = Vec::new();
        for (page_0, slot) in doc.pages.iter().enumerate() {
            if let Some(text) = &slot.text {
                for (occurrence, (offset, _)) in text.match_indices(query).enumerate() {
                    let excerpt: String = text[offset..].chars().take(300).collect();
                    hits.push(serde_json::json!({"page":page_0+1,"quote":query,"occurrence":occurrence,"excerpt":excerpt}));
                    if hits.len() >= max as usize {
                        return Ok(serde_json::json!(hits).to_string());
                    }
                }
            }
        }
        Ok(serde_json::json!(hits).to_string())
    }
}
