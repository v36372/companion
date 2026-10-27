//! Thin div-based presentation; durable state and scheduling live in the bridge.
use crate::{agent_bridge::Bridge, ask_bar::AskBar};
use gpui::{Entity, IntoElement, div, prelude::*, px, rgb};

pub fn pane(
    bridge: &Bridge,
    composer: Entity<AskBar>,
    scroll: &gpui::ScrollHandle,
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
                .text_color(rgb(0xa1a1aa))
                .child(bridge.status.clone()),
        )
        .child(
            div()
                .id("chat-transcript")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .track_scroll(scroll)
                .children(
                    bridge
                        .transcript
                        .iter()
                        .map(|text| div().py_3().child(text.clone())),
                )
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
        .child(composer)
}
