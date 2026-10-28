use std::sync::{
    Arc, RwLock,
    atomic::{AtomicUsize, Ordering},
};

use gpui_pdf::{Document, extract_page_text};
use serde::Serialize;

use crate::page_index::page_0_to_1;

#[derive(Clone, Copy, Debug, Serialize)]
pub struct GlyphBox {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Clone, Debug, Serialize)]
pub struct PageJson {
    pub page: u32,
    pub text: Option<String>,
    pub no_text_layer: bool,
    pub boxes: Vec<GlyphBox>,
}

#[derive(Clone, Debug, Serialize)]
pub struct DocMeta {
    pub id: String,
    pub title: String,
    pub source_path: String,
    pub sha256: String,
    pub page_count: u32,
}

#[derive(Clone, Debug, Serialize)]
pub struct Viewport {
    pub page: u32,
    pub page_count: u32,
    pub scroll_frac: f32,
    pub excerpt: String,
    pub source: &'static str,
    pub updated_at_ms: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct LiveSelection {
    pub page: u32,
    pub quote: String,
    pub occurrence: u32,
    pub prefix: String,
    pub suffix: String,
}

pub struct PageSlot {
    pub text: Option<Arc<str>>,
    pub json: Arc<PageJson>,
}

pub struct DocSnapshot {
    pub meta: DocMeta,
    pub outline_md: String,
    /// Prepared off-thread alongside the agent-facing Markdown.
    pub outline: Vec<gpui_pdf::OutlineItem>,
    pub hayro: Arc<Document>,
    pub pages: Vec<PageSlot>,
    pub viewport: RwLock<Viewport>,
    pub selection: RwLock<Option<LiveSelection>>,
}

impl DocSnapshot {
    /// Call on the background executor. At most four workers extract pages from
    /// our parse, independently of PdfView's private parse (design D6).
    pub fn build(bytes: Arc<Vec<u8>>, meta: DocMeta) -> Result<Self, gpui_pdf::LoadError> {
        Self::build_with_password(bytes, meta, "")
    }

    /// Password is borrowed for this parse only, never included in the snapshot.
    pub fn build_with_password(
        bytes: Arc<Vec<u8>>,
        mut meta: DocMeta,
        password: &str,
    ) -> Result<Self, gpui_pdf::LoadError> {
        let hayro = gpui_pdf::parse_with_password(bytes, password)?;
        meta.page_count = hayro.pages().len() as u32;
        meta.title = crate::title::display_title(
            hayro.metadata().title.as_deref(),
            &meta.source_path,
            &meta.title,
        );
        let next = AtomicUsize::new(0);
        let slots = std::sync::Mutex::new((0..meta.page_count).map(|_| None).collect::<Vec<_>>());
        std::thread::scope(|scope| {
            for _ in 0..4.min(meta.page_count) {
                let (doc, next, slots) = (&hayro, &next, &slots);
                scope.spawn(move || {
                    loop {
                        let page_0 = next.fetch_add(1, Ordering::Relaxed);
                        if page_0 >= doc.pages().len() {
                            break;
                        }
                        let extracted =
                            extract_page_text(doc, page_0).filter(|text| !text.is_empty());
                        let text = extracted.as_ref().map(|text| text.text());
                        // gpui-pdf exposes quote/line boxes, not individual glyphs.
                        let boxes = extracted
                            .as_ref()
                            .map(|page| {
                                page.locate(&page.text(), 0)
                                    .into_iter()
                                    .map(|rect| GlyphBox {
                                        x: rect.x,
                                        y: rect.y,
                                        w: rect.w,
                                        h: rect.h,
                                    })
                                    .collect()
                            })
                            .unwrap_or_default();
                        let json = Arc::new(PageJson {
                            page: page_0_to_1(page_0 as u32),
                            no_text_layer: text.is_none(),
                            text: text.clone(),
                            boxes,
                        });
                        slots.lock().unwrap()[page_0] = Some(PageSlot {
                            text: text.map(Arc::from),
                            json,
                        });
                    }
                });
            }
        });
        let outline = crate::destinations::resolved_outline(&hayro);
        let outline_md = outline
            .iter()
            .map(|item| {
                let target = item
                    .page
                    .map(|page_0| format!(" (page {})", page_0_to_1(page_0 as u32)))
                    .unwrap_or_default();
                format!("{}- {}{}\n", "  ".repeat(item.level), item.title, target)
            })
            .collect();
        let viewport = RwLock::new(Viewport {
            page: 1,
            page_count: meta.page_count,
            scroll_frac: 0.0,
            excerpt: String::new(),
            source: "explicit_nav",
            updated_at_ms: 0,
        });
        Ok(Self {
            meta,
            outline_md,
            outline,
            hayro,
            pages: slots
                .into_inner()
                .unwrap()
                .into_iter()
                .map(Option::unwrap)
                .collect(),
            viewport,
            selection: RwLock::new(None),
        })
    }

    pub fn record_navigation(&self, page_0: u32, scroll_frac: f32) {
        let excerpt = self
            .pages
            .get(page_0 as usize)
            .and_then(|page| page.text.as_deref())
            .unwrap_or_default()
            .chars()
            .take(500)
            .collect();
        *self.viewport.write().unwrap() = Viewport {
            page: page_0_to_1(page_0),
            page_count: self.meta.page_count,
            scroll_frac,
            excerpt,
            source: "explicit_nav",
            updated_at_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        };
    }
}
