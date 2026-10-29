use std::{cell::Cell, path::PathBuf, rc::Rc, sync::Arc};

use crate::snapshot::DocSnapshot;

use gpui::{App, AppContext, Entity, Window};
use gpui_pdf::{PdfStyle, PdfView};

/// Stable document-annotation colours, shared by new selections and restored
/// persisted highlights. These are content colours, not application chrome.
pub fn highlight_color(name: &str) -> gpui::Hsla {
    gpui::rgb(match name {
        "green" => 0x86efac,
        "blue" => 0x93c5fd,
        "pink" => 0xf9a8d4,
        "purple" => 0xc4b5fd,
        _ => 0xfde047,
    })
    .into()
}

/// One open viewer. The window owner must call `release` before replacing or closing it.
///
/// The app supplies a managed copy-on-open path and asynchronously attaches our
/// separately parsed snapshot (the accepted double parse in D6).
pub struct CompanionPdf {
    inner: Entity<PdfView>,
    style: Rc<Cell<PdfStyle>>,
    pub snapshot: Option<Arc<DocSnapshot>>,
}

impl CompanionPdf {
    pub fn open(path: PathBuf, cx: &mut App) -> Self {
        let style = Rc::new(Cell::new(PdfStyle::default()));
        let current_style = style.clone();
        let inner = cx.new(|cx| {
            PdfView::new(
                path,
                Rc::new(move || current_style.get()),
                Rc::new(|| 1.0),
                cx,
            )
        });
        Self {
            inner,
            style,
            snapshot: None,
        }
    }

    /// Theme changes update chrome and select cached page pixels; never reload
    /// the document or change page width, zoom or raster quality.
    pub fn set_style(&self, style: PdfStyle, cx: &mut App) {
        let previous = self.style.replace(style);
        if previous.page_colors != style.page_colors
            || previous.bg != style.bg
            || previous.border != style.border
            || previous.placeholder_bg != style.placeholder_bg
            || previous.placeholder_fg != style.placeholder_fg
            || previous.header_fg != style.header_fg
            || previous.header_muted != style.header_muted
        {
            self.inner.update(cx, |_, cx| cx.notify());
        }
    }

    pub fn set_on_create_highlight(&self, callback: gpui_pdf::CreateHighlightFn, cx: &mut App) {
        self.inner.update(cx, |view, cx| {
            view.set_on_create_highlight(callback);
            view.set_highlight_palette(
                ["yellow", "green", "blue", "pink", "purple"]
                    .into_iter()
                    .map(|name| (name.into(), highlight_color(name)))
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

    pub fn focus_search(&self, cx: &mut App) {
        self.inner.update(cx, |view, cx| view.focus_search(cx));
    }

    /// Let viewer inputs consume Escape before the application's global abort.
    pub fn dismiss_input(&self, window: &mut Window, cx: &mut App) -> bool {
        self.inner
            .update(cx, |view, cx| view.dismiss_input(window, cx))
    }

    pub fn clear_selection(&self, cx: &mut App) {
        self.inner.update(cx, |view, cx| view.clear_selection(cx));
    }

    pub fn has_selection(&self, cx: &App) -> bool {
        self.inner.read(cx).has_selection()
    }

    pub fn selection_geometry(
        &self,
        cx: &App,
    ) -> (
        Option<gpui::Bounds<gpui::Pixels>>,
        gpui::Bounds<gpui::Pixels>,
    ) {
        let view = self.inner.read(cx);
        (view.selection_anchor(), view.selection_viewport())
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
