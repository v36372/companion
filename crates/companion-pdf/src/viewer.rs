use std::{path::PathBuf, rc::Rc};

use gpui::{App, AppContext, Entity, Window};
use gpui_pdf::{PdfStyle, PdfView};

/// One open viewer. The window owner must call `release` before replacing or closing it.
///
/// PR1 opens the original path directly. Copy-on-open belongs to PR2; PR3 will
/// add our separately parsed `DocSnapshot` (the accepted double parse in D6).
pub struct CompanionPdf {
    inner: Entity<PdfView>,
}

impl CompanionPdf {
    pub fn open(path: PathBuf, cx: &mut App) -> Self {
        let inner =
            cx.new(|cx| PdfView::new(path, Rc::new(PdfStyle::default), Rc::new(|| 1.0), cx));
        Self { inner }
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
