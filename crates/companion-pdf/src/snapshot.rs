use std::sync::{
    Arc, OnceLock, RwLock,
    atomic::{AtomicBool, AtomicUsize, Ordering},
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
    pub json: Arc<PageJson>,
}

/// Owner-side lifetime of a prefill. Dropping it stops workers between pages.
#[derive(Default)]
pub struct TextPrefill(Arc<AtomicBool>);

impl TextPrefill {
    pub fn cancellation(&self) -> Arc<AtomicBool> {
        self.0.clone()
    }
}

impl Drop for TextPrefill {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

pub struct DocSnapshot {
    pub meta: DocMeta,
    pub outline_md: String,
    /// Prepared off-thread alongside the agent-facing Markdown.
    pub outline: Vec<gpui_pdf::OutlineItem>,
    pub hayro: Arc<Document>,
    pages: Vec<OnceLock<PageSlot>>,
    pub viewport: RwLock<Viewport>,
    pub selection: RwLock<Option<LiveSelection>>,
}

impl DocSnapshot {
    /// Parse metadata and outline only; text is cached on demand.
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
        let pages = (0..meta.page_count).map(|_| OnceLock::new()).collect();
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
            pages,
            viewport,
            selection: RwLock::new(None),
        })
    }

    /// Blocking agent-side access. Concurrent requests extract each page once.
    pub fn page(&self, page_0: usize) -> Option<&PageSlot> {
        let slot = self.pages.get(page_0)?.get_or_init(|| {
            let extracted = extract_page_text(&self.hayro, page_0).filter(|text| !text.is_empty());
            let text = extracted.as_ref().map(|page| page.text());
            let boxes = extracted
                .as_ref()
                .map(|page| {
                    page.locate(text.as_deref().unwrap_or_default(), 0)
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
            PageSlot {
                json: Arc::new(PageJson {
                    page: page_0_to_1(page_0 as u32),
                    no_text_layer: text.is_none(),
                    text,
                    boxes,
                }),
            }
        });
        let mut viewport = self.viewport.write().unwrap();
        if viewport.page == page_0_to_1(page_0 as u32) {
            viewport.excerpt = slot
                .json
                .text
                .as_deref()
                .unwrap_or_default()
                .chars()
                .take(500)
                .collect();
        }
        Some(slot)
    }

    /// UI-side access: never initiates or waits for extraction.
    pub fn page_if_ready(&self, page_0: usize) -> Option<&PageSlot> {
        self.pages.get(page_0)?.get()
    }

    /// Run off-thread after publication. The current page completes first, then
    /// two workers fill outward. Cancellation bounds remaining work to two pages.
    pub fn prefill(
        &self,
        current: usize,
        cancelled: &AtomicBool,
        ready: impl Fn(usize) + Sync,
    ) -> bool {
        let count = self.pages.len();
        if count == 0 || cancelled.load(Ordering::Relaxed) {
            return !cancelled.load(Ordering::Relaxed);
        }
        let current = current.min(count - 1);
        self.page(current);
        ready(current);
        let order = prefill_order(count, current);
        let next = AtomicUsize::new(0);
        std::thread::scope(|scope| {
            for _ in 0..2.min(order.len()) {
                let (order, next, ready) = (&order, &next, &ready);
                scope.spawn(move || {
                    while !cancelled.load(Ordering::Relaxed) {
                        let Some(&page) = order.get(next.fetch_add(1, Ordering::Relaxed)) else {
                            break;
                        };
                        self.page(page);
                        ready(page);
                    }
                });
            }
        });
        !cancelled.load(Ordering::Relaxed)
    }

    /// Fixture hook: exclusive access only, never invalidates a published slot.
    #[doc(hidden)]
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_page_for_test(&mut self, page_0: usize, slot: PageSlot) {
        self.pages[page_0] = OnceLock::from(slot);
    }

    pub fn record_navigation(&self, page_0: u32, scroll_frac: f32) {
        self.record_position(page_0, scroll_frac, "explicit_nav");
    }

    /// Live scroll observations feed the same viewport used by Follow and PDF tools.
    pub fn record_scroll_navigation(&self, page_0: u32, scroll_frac: f32) {
        self.record_position(page_0, scroll_frac, "live_scroll");
    }

    #[cfg(test)]
    fn fixture() -> Self {
        Self::build(
            Arc::new(include_bytes!("../tests/fixtures/mixed.pdf").to_vec()),
            DocMeta {
                id: "scroll-test".into(),
                title: "Fixture".into(),
                source_path: "mixed.pdf".into(),
                sha256: "fixture".into(),
                page_count: 0,
            },
        )
        .unwrap()
    }

    fn record_position(&self, page_0: u32, scroll_frac: f32, source: &'static str) {
        // Read readiness under the viewport lock so a concurrent fill cannot
        // refresh the excerpt and then have it overwritten by an empty sample.
        let mut viewport = self.viewport.write().unwrap();
        let excerpt = self
            .page_if_ready(page_0 as usize)
            .and_then(|page| page.json.text.as_deref())
            .unwrap_or_default()
            .chars()
            .take(500)
            .collect();
        *viewport = Viewport {
            page: page_0_to_1(page_0),
            page_count: self.meta.page_count,
            scroll_frac: scroll_frac.clamp(0.0, 1.0),
            excerpt,
            source,
            updated_at_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        };
    }
}

fn prefill_order(count: usize, current: usize) -> Vec<usize> {
    (1..count)
        .flat_map(|distance| {
            [
                current.checked_add(distance).filter(|&page| page < count),
                current.checked_sub(distance),
            ]
            .into_iter()
            .flatten()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outward_order_covers_every_other_page_once() {
        assert_eq!(prefill_order(7, 3), vec![4, 2, 5, 1, 6, 0]);
        assert_eq!(prefill_order(4, 0), vec![1, 2, 3]);
        assert_eq!(prefill_order(4, 3), vec![2, 1, 0]);
        assert!(prefill_order(1, 0).is_empty());
    }

    #[test]
    fn ui_read_does_not_wait_for_an_in_progress_slot() {
        let snapshot = DocSnapshot::fixture();
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let result = std::thread::scope(|scope| {
            let snapshot = &snapshot;
            scope.spawn(move || {
                snapshot.pages[0].get_or_init(|| {
                    started_tx.send(()).unwrap();
                    release_rx.recv().unwrap();
                    PageSlot {
                        json: Arc::new(PageJson {
                            page: 1,
                            text: Some("ready".into()),
                            no_text_layer: false,
                            boxes: vec![],
                        }),
                    }
                });
            });
            started_rx.recv().unwrap();
            scope.spawn(move || ready_tx.send(snapshot.page_if_ready(0).is_none()).unwrap());
            let result = ready_rx.recv_timeout(std::time::Duration::from_secs(1));
            release_tx.send(()).unwrap();
            result
        });
        assert!(
            result.unwrap(),
            "UI read initiated or waited for extraction"
        );
        assert_eq!(
            snapshot.page_if_ready(0).unwrap().json.text.as_deref(),
            Some("ready")
        );
    }

    #[test]
    fn scroll_updates_follow_viewport_and_scanned_page() {
        let snapshot = DocSnapshot::fixture();
        snapshot.page(0);
        snapshot.record_scroll_navigation(0, 0.35);
        {
            let viewport = snapshot.viewport.read().unwrap();
            assert_eq!(viewport.page, 1);
            assert_eq!(viewport.source, "live_scroll");
            assert_eq!(viewport.scroll_frac, 0.35);
            assert!(!viewport.excerpt.is_empty());
        }
        snapshot.record_scroll_navigation(1, 0.5);
        let viewport = snapshot.viewport.read().unwrap();
        assert_eq!(viewport.page, 2);
        assert!(viewport.excerpt.is_empty());
    }
}
