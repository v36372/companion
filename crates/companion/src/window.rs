use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use companion_pdf::CompanionPdf;
use companion_store::{SqliteHub, documents::Document, progress::ReadingProgress};
use gpui::{Context, FocusHandle, Window, div, prelude::*, px, rgb};

pub struct CompanionWindow {
    pdf: Option<CompanionPdf>,
    focus: FocusHandle,
    dialog_open: bool,
    catalog: Option<Arc<SqliteHub>>,
    data_dir: PathBuf,
    recent: Vec<Document>,
    document: Option<Document>,
    page_0: u32,
    navigation: HashMap<String, (ReadingProgress, Arc<AtomicU64>)>,
    error: Option<String>,
}

fn open_catalog() -> anyhow::Result<(PathBuf, Arc<SqliteHub>)> {
    let data_dir = match std::env::var_os("COMPANION_DATA_DIR") {
        Some(path) => PathBuf::from(path),
        None => directories::ProjectDirs::from("", "LearningCompanion", "learning-companion")
            .ok_or_else(|| anyhow::anyhow!("Cannot determine application data directory"))?
            .data_dir()
            .to_path_buf(),
    };
    std::fs::create_dir_all(data_dir.join("docs"))?;
    let data_dir = data_dir.canonicalize()?;
    let catalog = Arc::new(SqliteHub::open(data_dir.join("companion.sqlite"))?);
    Ok((data_dir, catalog))
}

impl CompanionWindow {
    pub fn new(path: Option<PathBuf>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        let weak = cx.weak_entity();
        window.on_window_should_close(cx, move |window, cx| {
            let _ = weak.update(cx, |view, cx| view.close_pdf(window, cx));
            true
        });
        let (data_dir, catalog, error) = match open_catalog() {
            Ok((path, catalog)) => (path, Some(catalog), None),
            Err(error) => (
                PathBuf::new(),
                None,
                Some(format!("Cannot open catalog: {error:#}")),
            ),
        };
        let mut view = Self {
            pdf: None,
            focus,
            dialog_open: false,
            catalog,
            data_dir,
            recent: Vec::new(),
            document: None,
            page_0: 0,
            navigation: HashMap::new(),
            error,
        };
        view.refresh_recent();
        if let Some(path) = path {
            view.open_pdf(path, window, cx);
        }
        view
    }

    /// Flush pending explicit navigation before quitting; background timers may
    /// not run once the application event loop has stopped.
    pub fn close_pdf(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(catalog) = &self.catalog {
            for (document_id, (progress, generation)) in &self.navigation {
                // Check/invalidate timers under the same writer lock used below.
                let result = catalog.write(|connection| {
                    generation.fetch_add(1, Ordering::SeqCst);
                    companion_store::progress::set_on_connection(connection, document_id, progress)
                });
                if let Err(error) = result {
                    tracing::error!("Cannot flush reading progress: {error:#}");
                }
            }
        }
        self.release_pdf(window, cx);
    }

    fn release_pdf(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(pdf) = self.pdf.take() {
            pdf.release(window, cx);
        }
    }

    fn open_pdf(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let _span = tracing::info_span!(target: "companion", "open_pdf").entered();
        let result = self
            .catalog
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Catalog unavailable"))
            .and_then(|catalog| catalog.import_pdf(&path, &self.data_dir));
        match result {
            Ok(document) => self.open_document(document, window, cx),
            Err(error) => {
                self.error = Some(format!("Cannot import PDF: {error:#}"));
                cx.notify();
            }
        }
    }

    fn refresh_recent(&mut self) {
        if let Some(catalog) = &self.catalog {
            match catalog.recent(20) {
                Ok(recent) => self.recent = recent,
                Err(error) => self.error = Some(format!("Cannot read recent files: {error:#}")),
            }
        }
    }

    fn open_document(&mut self, document: Document, window: &mut Window, cx: &mut Context<Self>) {
        let Some(catalog) = self.catalog.clone() else {
            return;
        };
        // A rapid switch/reopen may precede the debounce write. Prefer our
        // latest explicit page over the older durable row in that case.
        let saved = self
            .navigation
            .get(&document.id)
            .map(|(progress, _)| progress.clone());
        let progress = match saved
            .map(Some)
            .map(Ok)
            .unwrap_or_else(|| catalog.progress(&document.id))
        {
            Ok(progress) => progress,
            Err(error) => {
                self.error = Some(format!("Cannot restore progress: {error:#}"));
                cx.notify();
                return;
            }
        };
        if !std::path::Path::new(&document.stored_path).is_file() {
            self.error =
                Some("Managed PDF is missing. Open the original file to restore it.".into());
            cx.notify();
            return;
        }
        // Only release the old viewer once the new managed document is available.
        self.release_pdf(window, cx);
        let pdf = CompanionPdf::open(PathBuf::from(&document.stored_path), cx);
        self.page_0 = progress.as_ref().map_or(0, |progress| progress.page_0);
        pdf.goto_page_0(self.page_0 as usize, cx);
        self.pdf = Some(pdf);
        self.document = Some(document.clone());
        self.error = None;
        if let Err(error) = catalog.touch_last_opened(&document.id) {
            self.error = Some(format!("Cannot update recent files: {error:#}"));
        }
        self.refresh_recent();
        // Reopening is explicit navigation, but preserve any saved excerpt/fraction.
        self.persist_navigation(
            progress.unwrap_or(ReadingProgress {
                page_0: 0,
                scroll_frac: 0.0,
                viewport_excerpt: None,
            }),
            cx,
        );
        cx.notify();
    }

    fn navigate(&mut self, forward: bool, cx: &mut Context<Self>) {
        let Some(pdf) = &self.pdf else {
            return;
        };
        let page_0 = if forward {
            self.page_0.saturating_add(1)
        } else {
            self.page_0.saturating_sub(1)
        };
        self.page_0 = match self.document.as_ref().and_then(|doc| doc.page_count) {
            Some(count) => page_0.min(count.saturating_sub(1)),
            None => page_0,
        };
        pdf.goto_page_0(self.page_0 as usize, cx);
        self.persist_navigation(
            ReadingProgress {
                page_0: self.page_0,
                scroll_frac: 0.0,
                viewport_excerpt: None,
            },
            cx,
        );
        cx.notify();
    }

    fn persist_navigation(&mut self, progress: ReadingProgress, cx: &Context<Self>) {
        let (Some(catalog), Some(document)) = (&self.catalog, &self.document) else {
            return;
        };
        let catalog = catalog.clone();
        let document_id = document.id.clone();
        // Tokens are per-document, not per-open: a stale timer must never
        // overwrite newer progress after quickly reopening the same document.
        let pending = self
            .navigation
            .entry(document_id.clone())
            .or_insert_with(|| (progress.clone(), Arc::new(AtomicU64::new(0))));
        pending.0 = progress.clone();
        let generation = pending.1.clone();
        let expected = generation.fetch_add(1, Ordering::SeqCst) + 1;
        let timer = cx.background_executor().timer(Duration::from_millis(500));
        cx.background_executor()
            .spawn(async move {
                timer.await;
                // Check while holding the writer lock, so an old timer cannot
                // overwrite a newer save after waiting for another transaction.
                let result = catalog.write(|connection| {
                    if generation.load(Ordering::SeqCst) == expected {
                        companion_store::progress::set_on_connection(
                            connection,
                            &document_id,
                            &progress,
                        )?;
                    }
                    Ok(())
                });
                if let Err(error) = result {
                    tracing::error!("Cannot save reading progress: {error:#}");
                }
            })
            .detach();
    }

    fn pick_pdf(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.dialog_open {
            return;
        }
        self.dialog_open = true;
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let file = rfd::AsyncFileDialog::new()
                .add_filter("PDF documents", &["pdf"])
                .pick_file()
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.dialog_open = false;
                if let Some(file) = file {
                    this.open_pdf(file.path().to_path_buf(), window, cx);
                }
                cx.notify();
            });
        })
        .detach();
    }
}

impl Render for CompanionWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let center = div().flex_1().min_w_0().h_full();
        let center = match &self.pdf {
            Some(pdf) => center.child(pdf.view()),
            None => center
                .flex()
                .items_center()
                .justify_center()
                .child("Open a PDF to start reading"),
        };

        div()
            .track_focus(&self.focus)
            .capture_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _, cx| {
                match event.keystroke.key.as_str() {
                    "pagedown" => { this.navigate(true, cx); cx.stop_propagation(); }
                    "pageup" => { this.navigate(false, cx); cx.stop_propagation(); }
                    _ => {}
                }
            }))
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(0x18181b))
            .text_color(rgb(0xe4e4e7))
            .child(
                div()
                    .flex()
                    .items_center()
                    .p_2()
                    .border_b_1()
                    .border_color(rgb(0x3f3f46))
                    .child(
                        div()
                            .id("open-pdf")
                            .px_3()
                            .py_1()
                            .rounded_md()
                            .bg(rgb(0x3f3f46))
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, window, cx| this.pick_pdf(window, cx)))
                            .child(if self.dialog_open {
                                "Choosing PDF…"
                            } else {
                                "Open PDF…"
                            }),
                    )
                    .child(div().id("previous-page").px_3().cursor_pointer()
                        .on_click(cx.listener(|this, _, _, cx| this.navigate(false, cx)))
                        .child("Previous"))
                    .child(div().id("next-page").px_3().cursor_pointer()
                        .on_click(cx.listener(|this, _, _, cx| this.navigate(true, cx)))
                        .child("Next"))
                    .child(format!("Last explicit page: {}", self.page_0 as u64 + 1)),
            )
            .children(self.error.clone().map(|error| div().p_2().text_color(rgb(0xfca5a5)).child(error)))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(
                        div()
                            .w(px(220.))
                            .flex_shrink_0()
                            .h_full()
                            .p_4()
                            .border_r_1()
                            .border_color(rgb(0x3f3f46))
                            .id("recent-files")
                            .overflow_y_scroll()
                            .child("Recent")
                            .child(div().text_sm().py_2().child("Progress tracks app Previous/Next and Page Up/Down, not wheel scroll."))
                            .children(self.recent.iter().cloned().map(|document| {
                                div().id(gpui::SharedString::from(document.id.clone()))
                                    .py_2().cursor_pointer().child(document.title.clone())
                                    .on_click(cx.listener(move |this, _, window, cx| this.open_document(document.clone(), window, cx)))
                            })),
                    )
                    .child(center)
                    .child(
                        div()
                            .w(px(360.))
                            .flex_shrink_0()
                            .h_full()
                            .p_4()
                            .border_l_1()
                            .border_color(rgb(0x3f3f46))
                            .child("Agent — coming soon"),
                    ),
            )
    }
}
