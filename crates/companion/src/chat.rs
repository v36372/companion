//! Thin div-based presentation; durable state and scheduling live in the bridge.
use crate::{agent_bridge::Bridge, ask_bar::AskBar};
use gpui::{Entity, IntoElement, div, prelude::*, px, rgb};

pub fn pane(
    bridge: &Bridge,
    composer: Entity<AskBar>,
    scroll: &gpui::ScrollHandle,
    navigate: impl Fn(u32, &mut gpui::Window, &mut gpui::App) + Clone + 'static,
    retry: impl Fn(&mut gpui::Window, &mut gpui::App) + 'static,
) -> impl IntoElement {
    div()
        .w(px(360.))
        .flex_shrink_0()
        .h_full()
        .flex()
        .flex_col()
        .p_4()
        .gap_3()
        .border_l_1()
        .border_color(rgb(0x3f3f46))
        .child("Agent")
        .child(
            div()
                .text_sm()
                .text_color(rgb(if bridge.can_chat() {
                    0xa1a1aa
                } else {
                    0xfcd34d
                }))
                .child(
                    bridge
                        .unavailable_reason()
                        .unwrap_or_else(|| bridge.status.clone()),
                ),
        )
        .children(
            (!bridge.can_chat() && bridge.status.starts_with("Not sent:")).then(|| {
                div()
                    .text_sm()
                    .text_color(rgb(0xfca5a5))
                    .child(bridge.status.clone())
            }),
        )
        .child(
            div()
                .id("chat-transcript")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .track_scroll(scroll)
                .children(bridge.transcript.iter().enumerate().map(|(index, turn)| {
                    let navigate = navigate.clone();
                    let (speaker, question) =
                        turn.text.split_once('\n').unwrap_or(("", &turn.text));
                    div()
                        .py_3()
                        .child(div().text_sm().child(speaker.to_owned()))
                        .children(turn.quote.as_ref().map(|quote| {
                            let page_0 = quote.page - 1;
                            div()
                                .id(gpui::SharedString::from(format!("quote-{index}")))
                                .px_2()
                                .py_1()
                                .rounded_md()
                                .bg(rgb(0x3f3f46))
                                .text_sm()
                                .cursor_pointer()
                                .child(quote.label())
                                .on_click(move |_, window, cx| navigate(page_0, window, cx))
                        }))
                        .child(question.to_owned())
                }))
                .children(bridge.notices.iter().map(|text| {
                    div()
                        .py_1()
                        .text_sm()
                        .text_color(rgb(0x93c5fd))
                        .child(text.clone())
                }))
                .child(div().py_3().child(bridge.live.clone())),
        )
        .child(div().text_sm().child(if bridge.queued() > 0 {
            format!("{} follow-up(s) queued", bridge.queued())
        } else {
            "Select text, then Ask. Asking does not save a highlight.".into()
        }))
        .children(bridge.parked_task.map(|_| {
            div()
                .id("retry-parked-task")
                .p_2()
                .bg(rgb(0x3f3f46))
                .text_color(rgb(if bridge.can_retry() {
                    0xe4e4e7
                } else {
                    0x71717a
                }))
                .child(if bridge.running {
                    "Retry disabled while agent is running"
                } else {
                    "Retry parked task"
                })
                .when(bridge.can_retry(), |button| {
                    button
                        .cursor_pointer()
                        .hover(|style| style.bg(rgb(0x52525b)))
                        .active(|style| style.bg(rgb(0x71717a)))
                        .on_click(move |_, window, cx| retry(window, cx))
                })
        }))
        .child(composer)
}
