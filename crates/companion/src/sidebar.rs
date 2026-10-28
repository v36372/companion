use super::*;

impl CompanionWindow {
    pub(crate) fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let outline = self
            .pdf
            .as_ref()
            .and_then(|pdf| pdf.snapshot.as_ref())
            .map(|snapshot| snapshot.outline.clone())
            .unwrap_or_default();
        div()
            .w_full()
            .flex_shrink_0()
            .h_full()
            .p_4()
            .border_r_1()
            .border_color(rgb(0x3f3f46))
            .id("recent-files")
            .overflow_y_scroll()
            .child("Outline")
            .children(outline.into_iter().enumerate().map(|(index, item)| {
                let row = div()
                    .id(gpui::SharedString::from(format!("outline-{index}")))
                    .py_1()
                    .pl(px(item.level as f32 * 10.0))
                    .child(item.title);
                if let Some(page_0) = item.page {
                    row.cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.goto_explicit(page_0 as u32, cx);
                        }))
                } else {
                    row.opacity(0.45).child(" (unavailable)")
                }
            }))
            .child(div().py_2().child("Recent"))
            .child(
                div()
                    .text_sm()
                    .py_2()
                    .child("Progress tracks app Previous/Next and Page Up/Down, not wheel scroll."),
            )
            .children(self.recent.iter().cloned().map(|document| {
                div()
                    .id(gpui::SharedString::from(document.id.clone()))
                    .py_2()
                    .cursor_pointer()
                    .child(document.title.clone())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_document(document.clone(), window, cx)
                    }))
            }))
    }
}
