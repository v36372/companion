//! Native find input, match status, and keyboard navigation in a non-layout overlay.
use crate::{
    PdfView, TextSlot,
    toolbar::{FIND_HEIGHT, HEADER_HEIGHT, icon_button},
};
use gpui::{Context, IntoElement, MouseButton, Window, deferred, div, prelude::*, px};
use gpui_component::{
    ActiveTheme, Disableable, IconName, Sizable,
    input::{Escape, Input, InputEvent, InputState},
};

fn match_count(current: Option<usize>, total: usize) -> String {
    format!("{}/{}", current.map_or(0, |index| index + 1), total)
}

impl PdfView {
    pub(super) fn render_find_bar(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        if !self.search_open {
            return None;
        }
        if self.search_input.is_none() {
            let input = cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("Find in PDF…")
                    .submit_on_enter(true)
            });
            cx.subscribe(&input, |this, input, event: &InputEvent, cx| {
                match event {
                    InputEvent::Change => {
                        this.search_query = input.read(cx).value().to_string();
                        this.on_search_query_changed(cx);
                    }
                    InputEvent::PressEnter { shift, .. } => {
                        if *shift {
                            this.prev_match(cx);
                        } else {
                            this.next_match(cx);
                        }
                    }
                    _ => {}
                }
                cx.notify();
            })
            .detach();
            self.search_input = Some(input);
        }
        let input = self.search_input.as_ref().unwrap();
        if std::mem::take(&mut self.search_input_focus) {
            input.update(cx, |input, cx| {
                input.focus(window, cx);
                input.select_all(window, cx);
            });
        }
        let searching = !self.search_query.trim().is_empty()
            && self
                .page_text
                .values()
                .any(|slot| matches!(slot, TextSlot::Loading));
        let count = match_count(self.current_match, self.matches.len());
        let disabled = self.matches.is_empty();
        Some(
            deferred(
                div()
                    .id("pdf-find-bar")
                    .absolute()
                    .top(px(HEADER_HEIGHT + 4.))
                    .right_3()
                    .h(px(FIND_HEIGHT))
                    .flex()
                    .items_center()
                    .gap_1()
                    .px_2()
                    .bg(cx.theme().popover)
                    .text_color(cx.theme().popover_foreground)
                    .border_1()
                    .border_color(cx.theme().border)
                    .rounded(cx.theme().radius)
                    .shadow_md()
                    .text_sm()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_action(cx.listener(|this, _: &Escape, window, cx| {
                        this.close_search(cx);
                        window.focus(&this.focus, cx);
                        cx.stop_propagation();
                    }))
                    .child(
                        Input::new(input)
                            .small()
                            .w(px(176.))
                            .aria_label("Find in PDF"),
                    )
                    .child(
                        div()
                            .min_w(px(44.))
                            .text_color(cx.theme().muted_foreground)
                            .child(count),
                    )
                    .children(
                        searching.then(|| div().text_color(cx.theme().muted_foreground).child("…")),
                    )
                    .child(
                        icon_button(
                            "pdf-find-prev",
                            IconName::ChevronUp,
                            "Previous match (Shift-Enter / Cmd/Ctrl-Shift-G)",
                        )
                        .disabled(disabled)
                        .on_click(cx.listener(|this, _, _, cx| this.prev_match(cx))),
                    )
                    .child(
                        icon_button(
                            "pdf-find-next",
                            IconName::ChevronDown,
                            "Next match (Enter / Cmd/Ctrl-G)",
                        )
                        .disabled(disabled)
                        .on_click(cx.listener(|this, _, _, cx| this.next_match(cx))),
                    )
                    .child(
                        icon_button("pdf-find-close", IconName::Close, "Close find (Esc)")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.close_search(cx);
                                window.focus(&this.focus, cx);
                            })),
                    ),
            )
            .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::match_count;
    #[test]
    fn compact_one_based_match_counter() {
        assert_eq!(match_count(Some(2), 17), "3/17");
        assert_eq!(match_count(None, 0), "0/0");
        assert_eq!(match_count(Some(0), 1), "1/1");
    }
}
