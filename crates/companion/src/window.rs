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
use gpui::{Context, Entity, FocusHandle, Window, div, prelude::*, px};
use gpui_component::{
    ActiveTheme, Disableable, IconName, WindowExt,
    button::{Button, ButtonVariants},
    notification::Notification,
    resizable::{h_resizable, resizable_panel},
};

#[path = "sidebar.rs"]
mod sidebar;

pub struct CompanionWindow {
    pdf: Option<CompanionPdf>,
    focus: FocusHandle,
    window_title: String,
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
    password_bar: Entity<AskBar>,
    model_picker: Entity<crate::model_picker::ModelPicker>,
    password_open: bool,
    password_focus_pending: bool,
    chat_scroll: gpui::ScrollHandle,
    open_generation: u64,
    import: ImportState,
    pub(crate) doc_env: Option<Arc<companion_env::VirtualDocFs>>,
    pub(crate) pdf_commands: (
        crossbeam_channel::Sender<companion_env::PdfCommand>,
        crossbeam_channel::Receiver<companion_env::PdfCommand>,
    ),
}

/// Last requested file wins, even if an older background import finishes later.
/// Stale imports may populate the catalog, but never replace the viewer or errors.
#[derive(Default)]
struct ImportState {
    generation: u64,
    pending: bool,
}

impl ImportState {
    fn start(&mut self) -> u64 {
        self.invalidate();
        self.pending = true;
        self.generation
    }

    fn invalidate(&mut self) {
        self.generation += 1;
        self.pending = false;
    }

    fn finish(&mut self, generation: u64) -> bool {
        if generation != self.generation || !self.pending {
            return false;
        }
        self.pending = false;
        true
    }
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
        let model_picker = cx.new(|cx| crate::model_picker::ModelPicker::new(catalog.clone(), cx));
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
        let password_bar = cx.new(|cx| {
            let mut bar = AskBar::new(cx);
            bar.secret = true;
            bar
        });
        cx.subscribe(&password_bar, |this, _, event: &AskEvent, cx| {
            if let AskEvent::Submit(password) = event
                && let (Some(pdf), Some(document)) = (&this.pdf, &this.document)
            {
                pdf.unlock(password.clone(), cx);
                this.load_snapshot(document.clone(), password.clone(), cx);
                this.password_bar.update(cx, |bar, cx| {
                    bar.text.clear();
                    cx.notify();
                });
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
                            crate::chat::follow_tail(&this.chat_scroll);
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
            model_picker,
            focus,
            window_title: String::new(),
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
            password_bar,
            password_open: false,
            password_focus_pending: false,
            chat_scroll: gpui::ScrollHandle::new(),
            open_generation: 0,
            import: ImportState::default(),
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
        } else if let Some(document) = view.recent.first().cloned() {
            // Reattach the last document on launch so an interrupted conversation
            // reaches open-document + resume-wait without a manual reopen.
            view.open_document(document, window, cx);
        }
        view
    }

    pub fn agent_shutdown(&self) -> Option<async_channel::Receiver<()>> {
        self.bridge.shutdown()
    }

    /// Flush pending explicit navigation before quitting; background timers may
    /// not run once the application event loop has stopped.
    pub fn close_pdf(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.import.invalidate();
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
        self.password_open = false;
        self.password_focus_pending = false;
        self.password_bar.update(cx, |bar, cx| {
            bar.text.clear();
            cx.notify();
        });
        if let Some(pdf) = self.pdf.take() {
            pdf.release(window, cx);
        }
    }

    fn open_pdf(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let generation = self.import.start();
        let catalog = self.catalog.clone();
        let data_dir = self.data_dir.clone();
        self.error = None;
        // Reading, hashing, copying, and catalog I/O all run off the UI thread.
        // Keep the current document usable until the replacement is ready.
        let task = cx.background_executor().spawn(async move {
            let _span = tracing::info_span!(target: "companion", "import_pdf").entered();
            catalog
                .ok_or_else(|| anyhow::anyhow!("Catalog unavailable"))?
                .import_pdf(&path, &data_dir)
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if !this.import.finish(generation) {
                    return;
                }
                match result {
                    Ok(document) => this.open_document(document, window, cx),
                    Err(error) => this.error = Some(format!("Cannot import PDF: {error:#}")),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn refresh_recent(&mut self) {
        if let Some(catalog) = &self.catalog {
            match catalog.recent(20) {
                Ok(recent) => self.recent = recent,
                Err(error) => self.error = Some(format!("Cannot read recent files: {error:#}")),
            }
        }
    }

    fn open_document(
        &mut self,
        mut document: Document,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Metadata parsing and title-named link creation never run on the UI
        // thread. The previous viewer stays usable until this preflight finishes.
        let generation = self.import.start();
        let display_dir = self.data_dir.join("display").join(&document.id);
        let task = cx.background_executor().spawn(async move {
            let bytes = Arc::new(std::fs::read(&document.stored_path)?);
            let parsed = gpui_pdf::parse(bytes).ok();
            document.title = companion_pdf::title::display_title(
                parsed
                    .as_ref()
                    .and_then(|pdf| pdf.metadata().title.as_deref()),
                &document.source_path,
                &document.title,
            );
            let display_path = companion_pdf::title::display_path(
                std::path::Path::new(&document.stored_path),
                &display_dir,
                &document.title,
            )?;
            Ok::<_, anyhow::Error>((document, display_path))
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if !this.import.finish(generation) {
                    return;
                }
                match result {
                    Ok((document, path)) => this.attach_document(document, path, window, cx),
                    Err(error) => this.error = Some(format!("Cannot prepare PDF: {error:#}")),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn attach_document(
        &mut self,
        document: Document,
        display_path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
        let pdf = CompanionPdf::open(display_path, cx);
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
        cx.subscribe(
            &pdf.view(),
            |this, viewer, event: &gpui_pdf::PdfEvent, cx| {
                if let gpui_pdf::PdfEvent::LinkClicked(target) = event {
                    let destination = match target {
                        gpui_pdf::LinkTarget::Page(page) => Some(gpui_pdf::PageDestination {
                            page: *page,
                            y: None,
                        }),
                        gpui_pdf::LinkTarget::Destination(destination) => Some(*destination),
                        gpui_pdf::LinkTarget::Uri(uri) => {
                            if gpui_pdf::is_safe_external_uri(uri) {
                                cx.open_url(uri);
                            }
                            None
                        }
                        gpui_pdf::LinkTarget::Named(name) => {
                            if let Some(doc) = viewer.read(cx).document() {
                                let name = name.clone();
                                let source_doc = doc.clone();
                                let viewer_id = viewer.entity_id();
                                let task = cx.background_executor().spawn(async move {
                                    companion_pdf::destinations::resolve_named(&doc, &name)
                                });
                                cx.spawn(async move |this, cx| {
                                    let destination = task.await;
                                    let _ = this.update(cx, |this, cx| {
                                        if this
                                            .pdf
                                            .as_ref()
                                            .map(|pdf| pdf.view())
                                            .filter(|current| current.entity_id() == viewer_id)
                                            .and_then(|current| current.read(cx).document())
                                            .is_some_and(|current| {
                                                Arc::ptr_eq(&current, &source_doc)
                                            })
                                            && let Some(destination) = destination
                                        {
                                            this.goto_explicit(destination.page as u32, cx);
                                            if let Some(pdf) = &this.pdf {
                                                pdf.view().update(cx, |view, cx| {
                                                    view.go_to_destination(destination, cx)
                                                });
                                            }
                                        }
                                    });
                                })
                                .detach();
                            }
                            None
                        }
                    };
                    if let Some(destination) = destination {
                        this.goto_explicit(destination.page as u32, cx);
                        viewer.update(cx, |view, cx| view.go_to_destination(destination, cx));
                    }
                } else if matches!(event, gpui_pdf::PdfEvent::LockChanged) {
                    this.password_open = viewer.read(cx).is_locked();
                    this.password_focus_pending = this.password_open;
                    if this.password_open {
                        this.error = None;
                    }
                    cx.notify();
                } else if matches!(event, gpui_pdf::PdfEvent::LoadFailed) {
                    this.password_open = false;
                    this.error = viewer
                        .read(cx)
                        .load_error()
                        .map(|message| message.to_string());
                    cx.notify();
                }
            },
        )
        .detach();
        self.pdf = Some(pdf);
        self.document = Some(document.clone());
        self.selection = None;
        self.load_snapshot(document.clone(), String::new(), cx);
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

    fn load_snapshot(&mut self, document: Document, password: String, cx: &mut Context<Self>) {
        let Some(catalog) = self.catalog.clone() else {
            return;
        };
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
            let snapshot = match DocSnapshot::build_with_password(bytes, meta, &password) {
                Ok(snapshot) => snapshot,
                Err(gpui_pdf::LoadError::Locked) => return Ok(None),
                Err(error) => return Err(anyhow::anyhow!("{error:?}")),
            };
            snapshot_catalog.set_page_count(&snapshot.meta.id, snapshot.meta.page_count)?;
            snapshot_catalog.write(|connection| {
                connection.execute(
                    "UPDATE documents SET title=?1 WHERE id=?2",
                    [&snapshot.meta.title, &snapshot.meta.id],
                )?;
                Ok(())
            })?;
            Ok::<_, anyhow::Error>(Some(Arc::new(snapshot)))
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                // Ignore extraction from a document since replaced.
                if generation != this.open_generation {
                    return;
                }
                match result {
                    Ok(Some(snapshot)) => {
                        this.password_open = false;
                        this.error = None;
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
                            document.title = snapshot.meta.title.clone();
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
                    Ok(None) => {
                        this.password_open = true;
                        this.password_focus_pending = true;
                    }
                    Err(error) => this.error = Some(format!("Cannot extract PDF text: {error:#}")),
                }
                cx.notify();
            });
        })
        .detach();
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
            if !self.bridge.submit("Explain this passage.".into(), Some(serde_json::json!({
                "page": selection.page, "quote": selection.quote, "occurrence": selection.occurrence
            }))) {
                // The bridge supplies the visible rejection reason; retain selection.
                cx.notify();
                return;
            }
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
                    color: companion_pdf::highlight_color(&highlight.color),
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let spacing = cx.theme().spacing_tokens();
        let typography = cx.theme().typography_tokens();
        let radius = cx.theme().radius;
        let title = self
            .document
            .as_ref()
            .map_or("Learning Companion".to_owned(), |doc| {
                format!("{} — Learning Companion", doc.title)
            });
        if title != self.window_title {
            window.set_window_title(&title);
            self.window_title = title;
        }
        if let Some(error) = self.error.take() {
            window.push_notification(Notification::error(error).autohide(false), cx);
        }
        if self.password_open && self.password_focus_pending {
            self.password_focus_pending = false;
            self.password_bar
                .update(cx, |bar, cx| bar.focus(window, cx));
        }
        let disabled_reason = self.bridge.unavailable_reason();
        let can_chat = disabled_reason.is_none();
        self.composer.update(cx, |bar, cx| {
            if bar.disabled_reason != disabled_reason {
                bar.disabled_reason = disabled_reason.clone();
                cx.notify();
            }
        });
        self.model_picker.update(cx, |picker, cx| {
            if picker.disabled_reason != disabled_reason {
                picker.disabled_reason = disabled_reason.clone();
                cx.notify();
            }
        });
        let weak = cx.weak_entity();
        let navigate_quote = move |page_0, _: &mut Window, cx: &mut gpui::App| {
            let _ = weak.update(cx, |this, cx| this.goto_explicit(page_0, cx));
        };
        let retry_weak = cx.weak_entity();
        let retry = move |_: &mut Window, cx: &mut gpui::App| {
            let _ = retry_weak.update(cx, |this, cx| {
                this.bridge.retry_parked();
                cx.notify();
            });
        };
        if let Some(pdf) = &self.pdf {
            let theme = cx.theme();
            pdf.set_style(
                gpui_pdf::PdfStyle {
                    bg: theme.muted,
                    border: theme.border,
                    placeholder_bg: theme.secondary,
                    placeholder_fg: theme.muted_foreground,
                    header_fg: theme.foreground,
                    header_muted: theme.muted_foreground,
                },
                cx,
            );
        }
        let snapshot = self.pdf.as_ref().and_then(|pdf| pdf.snapshot.as_ref());
        let scanned = snapshot
            .and_then(|snapshot| snapshot.pages.get(self.page_0 as usize))
            .is_some_and(|page| page.json.no_text_layer);
        let center = div()
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .min_w_0()
            .overflow_hidden();
        let center = match &self.pdf {
            Some(pdf) => center.child(
                div()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .overflow_hidden()
                    .child(pdf.view()),
            ),
            None => center
                .flex()
                .items_center()
                .justify_center()
                .gap(spacing.md)
                .p(spacing.lg)
                .child(
                    div()
                        .text_size(typography.xl.size)
                        .child("Your next chapter starts here"),
                )
                .child(
                    div()
                        .text_color(cx.theme().muted_foreground)
                        .child("Open a PDF to read, highlight and ask questions."),
                )
                .child(
                    Button::new("empty-open-pdf")
                        .primary()
                        .icon(IconName::FolderOpen)
                        .label("Open PDF…")
                        .disabled(self.dialog_open || self.import.pending)
                        .on_click(cx.listener(|this, _, window, cx| this.pick_pdf(window, cx))),
                ),
        };
        // All transient reader UI is absolute: selections and scans never change
        // the viewer's available height or invalidate its page layout.
        let center = center.child(
            div()
                .absolute()
                .top(px(52.))
                .left(spacing.md)
                .right(spacing.md)
                .flex()
                .flex_col()
                .gap(spacing.sm)
                .children(scanned.then(|| {
                    div()
                        .p(spacing.sm)
                        .rounded(radius)
                        .bg(cx.theme().secondary)
                        .text_color(cx.theme().secondary_foreground)
                        .child(format!(
                            "Page {} has no text. OCR is not available.",
                            page_0_to_1(self.page_0)
                        ))
                }))
                .children(self.selection.as_ref().map(|(selection, _)| {
                    div()
                        .flex()
                        .items_center()
                        .gap(spacing.sm)
                        .p(spacing.sm)
                        .rounded(radius)
                        .bg(cx.theme().popover)
                        .border_1()
                        .border_color(cx.theme().border)
                        .shadow_md()
                        .flex_wrap()
                        .child(format!("Selection · page {}", selection.page))
                        .child(
                            Button::new("ask-selection")
                                .primary()
                                .label("Ask")
                                .disabled(!can_chat)
                                .tooltip(
                                    disabled_reason
                                        .clone()
                                        .unwrap_or_else(|| "Explain this passage".into()),
                                )
                                .on_click(cx.listener(|this, _, _, cx| this.ask_selection(cx))),
                        )
                        .child(
                            Button::new("highlight-selection")
                                .outline()
                                .label("Highlight")
                                .on_click(
                                    cx.listener(|this, _, _, cx| this.highlight_selection(cx)),
                                ),
                        )
                        .child(
                            Button::new("dismiss-selection")
                                .ghost()
                                .label("Dismiss")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.selection = None;
                                    cx.notify();
                                })),
                        )
                })),
        );

        div()
            .relative()
            .track_focus(&self.focus)
            .capture_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _, cx| {
                match event.keystroke.key.as_str() {
                    "f" if event.keystroke.modifiers.control
                        || event.keystroke.modifiers.platform =>
                    {
                        if let Some(pdf) = &this.pdf {
                            pdf.toggle_search(cx);
                        }
                        cx.stop_propagation();
                    }
                    "pagedown" => {
                        this.navigate(true, cx);
                        cx.stop_propagation();
                    }
                    "pageup" => {
                        this.navigate(false, cx);
                        cx.stop_propagation();
                    }
                    "escape" => {
                        this.bridge.abort();
                        cx.notify();
                        cx.stop_propagation();
                    }
                    _ => {}
                }
            }))
            .flex()
            .flex_col()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(spacing.sm)
                    .p(spacing.sm)
                    .flex_shrink_0()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        Button::new("open-pdf")
                            .outline()
                            .icon(IconName::FolderOpen)
                            .label(if self.dialog_open {
                                "Choosing PDF…"
                            } else if self.import.pending {
                                "Importing PDF…"
                            } else {
                                "Open PDF…"
                            })
                            .disabled(self.dialog_open || self.import.pending)
                            .on_click(cx.listener(|this, _, window, cx| this.pick_pdf(window, cx))),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .text_ellipsis()
                            .child(
                                self.document
                                    .as_ref()
                                    .map_or("Learning Companion".to_owned(), |doc| {
                                        doc.title.clone()
                                    }),
                            ),
                    ),
            )
            .child(
                div().flex_1().min_h_0().child(
                    h_resizable("reader-panes")
                        .child(
                            resizable_panel()
                                .size(px(240.))
                                .size_range(px(200.)..px(360.))
                                .flex_none()
                                .child(self.render_sidebar(window, cx)),
                        )
                        .child(
                            resizable_panel()
                                .size_range(px(280.)..gpui::Pixels::MAX)
                                .child(center),
                        )
                        .child(
                            resizable_panel()
                                .size(px(380.))
                                .size_range(px(320.)..px(560.))
                                .flex_none()
                                .child(crate::chat::column(
                                    &self.bridge,
                                    self.composer.clone(),
                                    self.model_picker.clone(),
                                    &self.chat_scroll,
                                    cx.listener(|this, _, _, cx| {
                                        this.bridge.toggle_follow();
                                        cx.notify();
                                    }),
                                    cx.listener(|this, _, _, cx| {
                                        this.bridge.submit("What did I just read?".into(), None);
                                        cx.notify();
                                    }),
                                    navigate_quote,
                                    retry,
                                    cx,
                                )),
                        ),
                ),
            )
            .children(self.password_open.then(|| {
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(cx.theme().overlay)
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .w(px(480.))
                            .p(spacing.md)
                            .rounded(radius)
                            .bg(cx.theme().popover)
                            .border_1()
                            .border_color(cx.theme().border)
                            .child("Unlock PDF")
                            .child(
                                div()
                                    .py(spacing.sm)
                                    .text_size(typography.sm.size)
                                    .child("Password required or incorrect. Kept in memory only."),
                            )
                            .child(self.password_bar.clone()),
                    )
            }))
    }
}

#[cfg(test)]
mod tests {
    use super::ImportState;

    #[test]
    fn latest_import_wins_in_either_completion_order() {
        for stale_finishes_first in [true, false] {
            let mut state = ImportState::default();
            let first = state.start();
            let second = state.start();
            if stale_finishes_first {
                assert!(!state.finish(first));
                assert!(state.pending);
            }
            assert!(state.finish(second));
            assert!(!state.pending);
            assert!(!state.finish(first));
            assert!(!state.finish(second));
        }
    }

    #[test]
    fn recent_or_close_invalidates_pending_import() {
        let mut state = ImportState::default();
        let pending = state.start();
        state.invalidate();
        assert!(!state.pending);
        assert!(!state.finish(pending));
        let next = state.start();
        assert!(state.finish(next));
    }
}
