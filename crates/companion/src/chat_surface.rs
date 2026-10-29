//! Wide chat surface: status, conversation, composer; secondary controls live in ⋯.
use crate::{
    agent_bridge::{Availability, Bridge},
    ask_bar::AskBar,
    diagram::DiagramViewer,
};
use gpui::{Entity, div, prelude::*, px};
use gpui_component::{
    ActiveTheme, Disableable, Sizable,
    button::{Button, ButtonVariants},
    popover::Popover,
    switch::Switch,
    text::TextView,
};
use std::rc::Rc;

pub fn key_card() -> impl IntoElement {
    div().flex().flex_col().gap_2().p_3()
        .child("Add an OpenRouter key")
        .child(Popover::new("key-instructions").trigger(Button::new("key-disclosure").ghost().small().label("Setup instructions"))
            .content(|_, _, cx| div().w(px(420.)).text_sm().text_color(cx.theme().foreground)
                .child("Restart after changes. Never paste keys in chat.")
                .child(TextView::markdown("key-command", if cfg!(target_os = "macos") {
                    "```sh\nsecurity add-generic-password -U -s learning-companion -a openrouter -w '<your-key>'\n```\nAllow Keychain access if prompted. Commands can remain in shell history."
                } else {
                    "```sh\nsecret-tool store --label='Learning Companion OpenRouter' service learning-companion username openrouter target default\n```\nEnter the key at its prompt. Requires libsecret-tools and an unlocked Secret Service."
                }).selectable(true))
                .child("Or launch with COMPANION_OPENROUTER_API_KEY set (host-only fallback when keychain lookup fails).")))
}
#[allow(clippy::too_many_arguments)]
pub fn column(
    bridge: &Bridge,
    composer: Entity<AskBar>,
    model_picker: Entity<crate::model_picker::ModelPicker>,
    viewer: Entity<DiagramViewer>,
    scroll: &gpui::ScrollHandle,
    follow: impl Fn(&bool, &mut gpui::Window, &mut gpui::App) + 'static,
    ask: impl Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
    navigate: impl Fn(u32, &mut gpui::Window, &mut gpui::App) + Clone + 'static,
    retry: impl Fn(&mut gpui::Window, &mut gpui::App) + 'static,
    close: impl Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
    cx: &mut gpui::App,
) -> impl IntoElement {
    composer.update(cx, |bar, cx| {
        if bar.running != bridge.running {
            bar.running = bridge.running;
            cx.notify();
        }
    });
    let status = crate::orb::agent_status(bridge, true, false);
    let can_chat = bridge.can_chat();
    let checked = bridge.follow;
    let follow = Rc::new(follow);
    let ask = Rc::new(ask);
    div().flex().flex_col().h_full().w_full().min_w_0().bg(cx.theme().background).text_color(cx.theme().foreground)
        .child(div().flex().items_center().justify_between().px_3().py_2().gap_2().border_b_1().border_color(cx.theme().border)
            .child(div().flex().items_center().gap_2().child(crate::orb::core(status, 16., cx))
                .child(div().text_sm().text_color(cx.theme().muted_foreground).child(if status == crate::orb::AgentStatus::Idle { "" } else { crate::chat::status_label(bridge) })))
            .child(div().flex().items_center().gap_2()
                .child(Popover::new("chat-options").trigger(Button::new("chat-options-trigger").ghost().small().label("⋯").tooltip("Conversation options"))
                    .content(move |_, _, cx| {
                        let follow = follow.clone(); let ask = ask.clone();
                        div().w(px(360.)).flex().flex_col().gap_3()
                            .child(model_picker.clone())
                            .child(Switch::new("follow-mode").label("Follow reading").checked(checked).disabled(!can_chat).on_click(move |value, window, cx| follow(value, window, cx)))
                            .child(Button::new("what-did-i-read").ghost().small().label("What did I just read?").disabled(!can_chat).on_click(move |event, window, cx| ask(event, window, cx)))
                            .child(div().text_xs().text_color(cx.theme().muted_foreground).child("Web search is available when a Brave key is configured. Ask in your question."))
                            .child(key_card())
                    }))
                .child(Button::new("close-chat").ghost().small().label("×").tooltip("Close conversation (Ctrl/Cmd-J)").on_click(close))))
        .when(matches!(bridge.availability, Availability::NoKey) && bridge.transcript.is_empty(), |el| el.child(key_card()))
        .child(div().flex_1().min_h_0().child(crate::chat::pane(bridge, composer, viewer, scroll, navigate, retry, cx)))
}
