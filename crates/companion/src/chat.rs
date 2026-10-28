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

#[allow(clippy::too_many_arguments)]
pub fn column(
    bridge: &Bridge,
    composer: Entity<AskBar>,
    model_picker: Entity<crate::model_picker::ModelPicker>,
    scroll: &gpui::ScrollHandle,
    key_setup_open: bool,
    setup: impl Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
    follow: impl Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
    ask: impl Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
    navigate: impl Fn(u32, &mut gpui::Window, &mut gpui::App) + Clone + 'static,
    retry: impl Fn(&mut gpui::Window, &mut gpui::App) + 'static,
) -> impl IntoElement {
    let can_chat = bridge.can_chat();
    div().flex().flex_col().h_full().w_full().flex_shrink_0()
                        .child(div().id("key-setup").px_4().py_2().cursor_pointer()
                            .text_color(rgb(0x93c5fd))
                            .hover(|style| style.bg(rgb(0x3f3f46)))
                            .active(|style| style.bg(rgb(0x52525b)))
                            .on_click(setup)
                            .child(if key_setup_open { "Hide key setup" } else { "Set up key" }))
                        .children(key_setup_open.then(|| div().id("key-setup-instructions").max_h(px(220.)).overflow_y_scroll().px_4().py_2().text_sm()
                            .child("OpenRouter key setup (restart after changes)")
                            .child(div().py_2().child("Launch from a terminal with: COMPANION_OPENROUTER_API_KEY='<your-key>' /path/to/learning-companion your.pdf. This is a host-only fallback when keychain lookup fails, not an override. Never paste keys in chat. Shell commands containing keys may remain in history."))
                            .child(div().py_2().child(if cfg!(target_os = "macos") {
                                "Or save in macOS Keychain: security add-generic-password -U -s learning-companion -a openrouter -w '<your-key>'. Allow/unlock Keychain access if prompted. Unsigned rebuilds may prompt again."
                            } else {
                                "Or use an unlocked Linux Secret Service session: secret-tool store --label='Learning Companion OpenRouter' service learning-companion username openrouter target default. Enter the key at its prompt. Install libsecret-tools if secret-tool is missing."
                            }))))
                        .child(model_picker)
                        .child(div().id("follow-mode").px_4().py_2()
                            .text_color(rgb(if can_chat { 0xe4e4e7 } else { 0x71717a }))
                            .when(can_chat, |button| button.cursor_pointer()
                                .hover(|style| style.bg(rgb(0x3f3f46)))
                                .active(|style| style.bg(rgb(0x52525b)))
                                .on_click(follow))
                            .child(if bridge.follow { "Follow: on" } else { "Follow: off" }))
                        .child(div().id("what-did-i-read").px_4().py_2()
                            .text_color(rgb(if can_chat { 0xe4e4e7 } else { 0x71717a }))
                            .when(can_chat, |button| button.cursor_pointer()
                                .hover(|style| style.bg(rgb(0x3f3f46)))
                                .active(|style| style.bg(rgb(0x52525b)))
                                .on_click(ask))
                            .child("What did I just read?"))
                        .child(div().flex_1().min_h_0().child(crate::chat::pane(bridge, composer, scroll, navigate, retry)))
}
