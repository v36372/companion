use std::path::PathBuf;

use companion_pdf::CompanionPdf;
use gpui::{Context, FocusHandle, Window, div, prelude::*, px, rgb};

pub struct CompanionWindow {
    pdf: Option<CompanionPdf>,
    focus: FocusHandle,
    dialog_open: bool,
}

impl CompanionWindow {
    pub fn new(path: Option<PathBuf>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        let weak = cx.weak_entity();
        window.on_window_should_close(cx, move |window, cx| {
            let _ = weak.update(cx, |view, cx| view.release_pdf(window, cx));
            true
        });
        let mut view = Self {
            pdf: None,
            focus,
            dialog_open: false,
        };
        if let Some(path) = path {
            view.open_pdf(path, window, cx);
        }
        view
    }

    pub fn release_pdf(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(pdf) = self.pdf.take() {
            pdf.release(window, cx);
        }
    }

    fn open_pdf(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let _span = tracing::info_span!(target: "companion", "open_pdf").entered();
        // Exactly one document; release its textures before constructing another.
        self.release_pdf(window, cx);
        self.pdf = Some(CompanionPdf::open(path, cx));
        cx.notify();
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
                    ),
            )
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
                            .child("Outline"),
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
