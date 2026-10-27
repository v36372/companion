use std::{
    collections::HashMap,
    path::PathBuf,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use crate::{
    agent_bridge::{Attachment, Bridge},
    ask_bar::{AskBar, AskEvent},
};
use companion_pdf::{
    CompanionPdf,
    page_index::{page_0_to_1, page_1_to_0},
    snapshot::{DocMeta, DocSnapshot, LiveSelection},
    text_layer::live_selection,
};
use companion_store::{SqliteHub, documents::Document, progress::ReadingProgress};
use gpui::{Context, Entity, FocusHandle, Window, div, prelude::*, px, rgb};

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
    selection: Option<(LiveSelection, String)>,
    bridge: Bridge,
    composer: Entity<AskBar>,
    chat_scroll: gpui::ScrollHandle,
    open_generation: u64,
    pub(crate) doc_env: Option<Arc<companion_env::VirtualDocFs>>,
    pub(crate) pdf_commands: (
        crossbeam_channel::Sender<companion_env::PdfCommand>,
        crossbeam_channel::Receiver<companion_env::PdfCommand>,
    ),
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
        let composer = cx.new(AskBar::new);
        cx.subscribe(&composer, |this, _, event: &AskEvent, cx| {
            match event {
                AskEvent::Submit(text) => {
                    if this.bridge.submit(text.clone(), None) {
                        this.composer.update(cx, |bar, cx| {
                            bar.text.clear();
                            cx.notify();
                        });
                    }
                }
                AskEvent::Abort => this.bridge.abort(),
            }
            cx.notify();
        })
        .detach();
        #[cfg(feature = "agent")]
        let bridge = if let Some(hub) = catalog.clone() {
            let (bridge, events) = Bridge::start(hub);
            cx.spawn(async move |this, cx| {
                while let Ok(event) = events.recv().await {
                    if this
                        .update(cx, |this, cx| {
                            this.bridge.event(event);
                            if this.bridge.running {
                                this.chat_scroll.scroll_to_bottom();
                            }
                            cx.notify();
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .detach();
            bridge
        } else {
            Bridge::off()
        };
        #[cfg(not(feature = "agent"))]
        let bridge = Bridge::off();
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
            selection: None,
            bridge,
            composer,
            chat_scroll: gpui::ScrollHandle::new(),
            open_generation: 0,
            doc_env: None,
            pdf_commands: crossbeam_channel::unbounded(),
        };
        // Command imports never block the UI thread. Drain here, not while holding SQLite.
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(30))
                    .await;
                if this
                    .update(cx, |this, cx| this.drain_pdf_commands(cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        view.refresh_recent();
        if let Some(path) = path {
            view.open_pdf(path, window, cx);
        }
        view
    }

    pub fn agent_shutdown(&self) -> Option<async_channel::Receiver<()>> {
        self.bridge.shutdown()
    }

    /// Flush pending explicit navigation before quitting; background timers may
    /// not run once the application event loop has stopped.
    pub fn close_pdf(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.bridge.close();
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

    fn drain_pdf_commands(&mut self, cx: &mut Context<Self>) {
        while let Ok(command) = self.pdf_commands.1.try_recv() {
            match command {
                companion_env::PdfCommand::Goto {
                    document_id,
                    page_0,
                    ..
                } => {
                    if self
                        .document
                        .as_ref()
                        .is_some_and(|doc| doc.id == document_id)
                    {
                        self.goto_explicit(page_0, cx);
                        self.bridge
                            .notices
                            .push(format!("went to p. {}", page_0_to_1(page_0)));
                    }
                }
                companion_env::PdfCommand::Highlight { document_id, .. } => {
                    if self
                        .document
                        .as_ref()
                        .is_some_and(|doc| doc.id == document_id)
                    {
                        self.restore_highlights(cx);
                        self.bridge.notices.push("highlighted a passage".into());
                        cx.notify();
                    }
                }
            }
        }
    }

    fn release_pdf(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.doc_env.take();
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
        // Invalidate chat immediately; the bridge waits for old submit + abort
        // before publishing any replacement SetDocument/OpenDocument.
        self.bridge.begin_switch();
        // Only release the old viewer once the new managed document is available.
        self.release_pdf(window, cx);
        let pdf = CompanionPdf::open(PathBuf::from(&document.stored_path), cx);
        self.page_0 = progress.as_ref().map_or(0, |progress| progress.page_0);
        pdf.goto_page_0(self.page_0 as usize, cx);
        let weak = cx.weak_entity();
        pdf.set_on_create_highlight(
            Rc::new(move |page_0, quote, occurrence, color, _, cx| {
                let _ = weak.update(cx, |this, cx| {
                    this.select_quote(
                        page_0 as u32,
                        quote,
                        occurrence as u32,
                        color.to_string(),
                        cx,
                    )
                });
            }),
            cx,
        );
        self.pdf = Some(pdf);
        self.document = Some(document.clone());
        self.selection = None;
        self.open_generation += 1;
        let generation = self.open_generation;
        let copied = document.stored_path.clone();
        let meta = DocMeta {
            id: document.id.clone(),
            title: document.title.clone(),
            source_path: document.source_path.clone(),
            sha256: document.sha256.clone(),
            page_count: 0,
        };
        let snapshot_catalog = catalog.clone();
        let task = cx.background_executor().spawn(async move {
            let bytes = Arc::new(std::fs::read(copied)?);
            let snapshot =
                DocSnapshot::build(bytes, meta).map_err(|error| anyhow::anyhow!("{error:?}"))?;
            snapshot_catalog.set_page_count(&snapshot.meta.id, snapshot.meta.page_count)?;
            Ok::<_, anyhow::Error>(Arc::new(snapshot))
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                // Ignore extraction from a document since replaced.
                if generation != this.open_generation {
                    return;
                }
                match result {
                    Ok(snapshot) => {
                        match companion_env::VirtualDocFs::new(
                            Some(snapshot.clone()),
                            this.catalog.as_ref().unwrap().clone(),
                        ) {
                            Ok(fs) => this.doc_env = Some(Arc::new(fs)),
                            Err(error) => {
                                this.error = Some(format!(
                                    "Cannot open document environment: {}",
                                    error.message
                                ))
                            }
                        }
                        if let Some(document) = &mut this.document {
                            document.page_count = Some(snapshot.meta.page_count);
                        }
                        if let Some(pdf) = &mut this.pdf {
                            pdf.snapshot = Some(snapshot);
                        }
                        if let Some((selection, _)) = &mut this.selection {
                            let text = this
                                .pdf
                                .as_ref()
                                .and_then(|pdf| pdf.snapshot.as_ref())
                                .and_then(|snapshot| {
                                    snapshot
                                        .pages
                                        .get(page_1_to_0(selection.page).unwrap() as usize)
                                })
                                .and_then(|page| page.text.as_deref())
                                .unwrap_or_default();
                            *selection = live_selection(
                                page_1_to_0(selection.page).unwrap(),
                                text,
                                selection.quote.clone(),
                                selection.occurrence,
                            );
                        }
                        let page_0 = this.page_0.min(
                            this.document
                                .as_ref()
                                .and_then(|doc| doc.page_count)
                                .unwrap_or(1)
                                .saturating_sub(1),
                        );
                        if page_0 != this.page_0 {
                            this.goto_explicit(page_0, cx);
                        } else if let Some(progress) = this
                            .document
                            .as_ref()
                            .and_then(|document| this.navigation.get(&document.id))
                            .map(|(progress, _)| progress.clone())
                        {
                            // Attaching extraction must not scroll or discard restored progress.
                            this.persist_navigation(progress, cx);
                        }
                        if let (Some(fs), Some(doc)) = (&this.doc_env, &this.document) {
                            this.bridge.switch_document(Attachment {
                                fs: fs.clone(),
                                pdf_tx: this.pdf_commands.0.clone(),
                                document_id: doc.id.clone(),
                                title: doc.title.clone(),
                                page_count: doc.page_count.unwrap_or(0),
                            });
                        }
                        this.refresh_recent();
                    }
                    Err(error) => this.error = Some(format!("Cannot extract PDF text: {error:#}")),
                }
                cx.notify();
            });
        })
        .detach();
        self.error = None;
        self.restore_highlights(cx);
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
        let page_0 = if forward {
            self.page_0.saturating_add(1)
        } else {
            self.page_0.saturating_sub(1)
        };
        let page_0 = match self.document.as_ref().and_then(|doc| doc.page_count) {
            Some(count) => page_0.min(count.saturating_sub(1)),
            None => page_0,
        };
        self.goto_explicit(page_0, cx);
    }

    fn goto_explicit(&mut self, page_0: u32, cx: &mut Context<Self>) {
        let Some(pdf) = &self.pdf else {
            return;
        };
        self.page_0 = page_0;
        pdf.goto_page_0(page_0 as usize, cx);
        self.persist_navigation(
            ReadingProgress {
                page_0,
                scroll_frac: 0.0,
                viewport_excerpt: None,
            },
            cx,
        );
        cx.notify();
    }

    fn persist_navigation(&mut self, mut progress: ReadingProgress, cx: &Context<Self>) {
        if let Some(snapshot) = self.pdf.as_ref().and_then(|pdf| pdf.snapshot.as_ref()) {
            snapshot.record_navigation(progress.page_0, progress.scroll_frac as f32);
            if progress.viewport_excerpt.is_none() {
                progress.viewport_excerpt = Some(snapshot.viewport.read().unwrap().excerpt.clone());
            }
            *snapshot.selection.write().unwrap() = self
                .selection
                .as_ref()
                .map(|(selection, _)| selection.clone());
        }
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

    fn select_quote(
        &mut self,
        page_0: u32,
        quote: String,
        occurrence: u32,
        color: String,
        cx: &mut Context<Self>,
    ) {
        let text = self
            .pdf
            .as_ref()
            .and_then(|pdf| pdf.snapshot.as_ref())
            .and_then(|snapshot| snapshot.pages.get(page_0 as usize))
            .and_then(|page| page.text.as_deref())
            .unwrap_or_default();
        self.selection = Some((live_selection(page_0, text, quote, occurrence), color));
        // Selection identifies the page without moving the viewport under the drag.
        self.page_0 = page_0;
        self.persist_navigation(
            ReadingProgress {
                page_0,
                scroll_frac: 0.0,
                viewport_excerpt: None,
            },
            cx,
        );
        cx.notify();
    }

    fn ask_selection(&mut self, cx: &mut Context<Self>) {
        if let Some((selection, _)) = &self.selection {
            self.bridge.submit("Explain this passage.".into(), Some(serde_json::json!({
                "page": selection.page, "quote": selection.quote, "occurrence": selection.occurrence
            })));
            cx.notify();
        }
    }

    fn highlight_selection(&mut self, cx: &mut Context<Self>) {
        let (Some((selection, color)), Some(document), Some(catalog)) =
            (&self.selection, &self.document, &self.catalog)
        else {
            return;
        };
        let highlight = companion_store::highlights::Highlight {
            id: uuid::Uuid::new_v4().to_string(),
            document_id: document.id.clone(),
            page_0: page_1_to_0(selection.page).expect("selection is 1-based"),
            quote_text: selection.quote.clone(),
            occurrence: selection.occurrence,
            color: color.clone(),
            conversation_entry_id: None,
        };
        match catalog.insert_highlight(&highlight) {
            Ok(_) => self.restore_highlights(cx),
            Err(error) => self.error = Some(format!("Cannot save highlight: {error:#}")),
        }
        cx.notify();
    }

    fn restore_highlights(&mut self, cx: &mut Context<Self>) {
        let (Some(catalog), Some(document), Some(pdf)) = (&self.catalog, &self.document, &self.pdf)
        else {
            return;
        };
        let result = (|| -> anyhow::Result<_> {
            Ok(catalog
                .highlights_with_rowids(&document.id)?
                .into_iter()
                .map(|(rowid, highlight)| gpui_pdf::Highlight {
                    id: rowid,
                    page: highlight.page_0 as usize,
                    quote: highlight.quote_text,
                    occurrence: highlight.occurrence as usize,
                    color: gpui::Hsla::from(rgb(match highlight.color.as_str() {
                        "green" => 0x86efac,
                        "blue" => 0x93c5fd,
                        "pink" => 0xf9a8d4,
                        "purple" => 0xc4b5fd,
                        _ => 0xfde047,
                    })),
                    region: None,
                })
                .collect())
        })();
        match result {
            Ok(highlights) => pdf.set_highlights(highlights, cx),
            Err(error) => self.error = Some(format!("Cannot restore highlights: {error:#}")),
        }
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
        let snapshot = self.pdf.as_ref().and_then(|pdf| pdf.snapshot.as_ref());
        let scanned = snapshot
            .and_then(|snapshot| snapshot.pages.get(self.page_0 as usize))
            .is_some_and(|page| page.json.no_text_layer);
        let outline = snapshot
            .map(|snapshot| snapshot.outline.clone())
            .unwrap_or_default();
        let center = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .h_full()
            .children(scanned.then(|| {
                div().p_2().bg(rgb(0x713f12)).child(format!(
                    "Page {}: This page has no text. (Last explicit navigation)",
                    page_0_to_1(self.page_0)
                ))
            }))
            .children(self.selection.as_ref().map(|(selection, _)| {
                div()
                    .flex()
                    .p_2()
                    .gap_3()
                    .child(format!("Selection · page {}", selection.page))
                    .child(
                        div()
                            .id("ask-selection")
                            .cursor_pointer()
                            .px_2()
                            .bg(rgb(0x3f3f46))
                            .on_click(cx.listener(|this, _, _, cx| this.ask_selection(cx)))
                            .child("Ask"),
                    )
                    .child(
                        div()
                            .id("highlight-selection")
                            .cursor_pointer()
                            .px_2()
                            .bg(rgb(0x3f3f46))
                            .on_click(cx.listener(|this, _, _, cx| this.highlight_selection(cx)))
                            .child("Highlight"),
                    )
            }));
        let center = match &self.pdf {
            Some(pdf) => center.child(div().flex_1().min_h_0().child(pdf.view())),
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
                    "escape" => { this.bridge.abort(); cx.notify(); cx.stop_propagation(); }
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
                    .child(format!("Last explicit page: {}", page_0_to_1(self.page_0))),
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
                            .child("Outline")
                            .children(outline.into_iter().enumerate().map(|(index, item)| {
                                div().id(gpui::SharedString::from(format!("outline-{index}"))).py_1()
                                    .pl(px(item.level as f32 * 10.0)).cursor_pointer().child(item.title)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if let Some(page_0) = item.page { this.goto_explicit(page_0 as u32, cx); }
                                    }))
                            }))
                            .child(div().py_2().child("Recent"))
                            .child(div().text_sm().py_2().child("Progress tracks app Previous/Next and Page Up/Down, not wheel scroll."))
                            .children(self.recent.iter().cloned().map(|document| {
                                div().id(gpui::SharedString::from(document.id.clone()))
                                    .py_2().cursor_pointer().child(document.title.clone())
                                    .on_click(cx.listener(move |this, _, window, cx| this.open_document(document.clone(), window, cx)))
                            })),
                    )
                    .child(center)
                    .child(div().flex().flex_col().h_full()
                        .child(div().id("follow-mode").px_4().py_2().cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| { this.bridge.toggle_follow(); cx.notify(); }))
                            .child(if self.bridge.follow { "Follow: on" } else { "Follow: off" }))
                        .child(div().flex_1().min_h_0().child(crate::chat::pane(&self.bridge, self.composer.clone(), &self.chat_scroll)))),
            )
    }
}
