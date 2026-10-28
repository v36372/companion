use std::{path::PathBuf, rc::Rc, sync::Arc};

use crate::snapshot::DocSnapshot;

use gpui::{App, AppContext, Entity, Window};
use gpui_pdf::{PdfStyle, PdfView};

/// One open viewer. The window owner must call `release` before replacing or closing it.
///
/// The app supplies a managed copy-on-open path and asynchronously attaches our
/// separately parsed snapshot (the accepted double parse in D6).
pub struct CompanionPdf {
    inner: Entity<PdfView>,
    pub snapshot: Option<Arc<DocSnapshot>>,
}

impl CompanionPdf {
    pub fn open(path: PathBuf, cx: &mut App) -> Self {
        let inner =
            cx.new(|cx| PdfView::new(path, Rc::new(PdfStyle::default), Rc::new(|| 1.0), cx));
        Self {
            inner,
            snapshot: None,
        }
    }

    pub fn set_on_create_highlight(&self, callback: gpui_pdf::CreateHighlightFn, cx: &mut App) {
        self.inner.update(cx, |view, cx| {
            view.set_on_create_highlight(callback);
            view.set_highlight_palette(
                [
                    ("yellow", 0xfde047),
                    ("green", 0x86efac),
                    ("blue", 0x93c5fd),
                    ("pink", 0xf9a8d4),
                    ("purple", 0xc4b5fd),
                ]
                .into_iter()
                .map(|(name, color)| (name.into(), gpui::Hsla::from(gpui::rgb(color))))
                .collect(),
                cx,
            );
            // Dragging text produces a pending selection, not a durable highlight.
            view.toggle_select_mode(cx);
        });
    }

    pub fn set_highlights(&self, highlights: Vec<gpui_pdf::Highlight>, cx: &mut App) {
        self.inner
            .update(cx, |view, cx| view.set_highlights(highlights, cx));
    }

    pub fn toggle_search(&self, cx: &mut App) {
        self.inner.update(cx, |view, cx| view.toggle_search(cx));
    }

    pub fn unlock(&self, password: String, cx: &mut App) {
        self.inner.update(cx, |view, cx| view.unlock(password, cx));
    }

    pub fn view(&self) -> Entity<PdfView> {
        self.inner.clone()
    }

    /// Navigate using a zero-based page index, including while the PDF is loading.
    pub fn goto_page_0(&self, page_0: usize, cx: &mut App) {
        self.inner.update(cx, |view, cx| {
            view.go_to_page(page_0, cx);
            // Unlike go_to_page, reveal_highlight queues navigation during load.
            view.reveal_highlight(page_0, cx);
        });
    }

    pub fn release(&self, window: &mut Window, cx: &mut App) {
        self.inner.update(cx, |view, cx| view.release(window, cx));
    }
}
