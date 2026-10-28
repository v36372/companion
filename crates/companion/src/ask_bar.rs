//! Small keyboard composer; no gpui-kit dependency.
use gpui::{Context, EventEmitter, FocusHandle, Render, Window, div, prelude::*, rgb};

pub enum AskEvent {
    Submit(String),
    Abort,
}
pub struct AskBar {
    focus: FocusHandle,
    pub text: String,
    pub secret: bool,
    pub model_input: bool,
    pub disabled_reason: Option<String>,
}
impl AskBar {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus: cx.focus_handle(),
            text: String::new(),
            secret: false,
            model_input: false,
            disabled_reason: None,
        }
    }
    pub fn focus(&self, window: &mut Window, cx: &mut gpui::App) {
        self.focus.focus(window, cx);
    }
    fn submit(&mut self, cx: &mut Context<Self>) {
        if self.disabled_reason.is_none() && (self.model_input || !self.text.trim().is_empty()) {
            cx.emit(AskEvent::Submit(self.text.clone()));
        }
    }
}
impl EventEmitter<AskEvent> for AskBar {}
impl Render for AskBar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let can_send =
            self.disabled_reason.is_none() && (self.model_input || !self.text.trim().is_empty());
        div()
            .id("ask-composer")
            .track_focus(&self.focus)
            .flex()
            .flex_col()
            .p_3()
            .gap_2()
            .border_1()
            .border_color(rgb(0x52525b))
            .rounded_md()
            .bg(rgb(0x27272a))
            .on_click(cx.listener(|this, _, window, cx| this.focus(window, cx)))
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _, cx| {
                let key = &event.keystroke;
                match key.key.as_str() {
                    "escape" => cx.emit(AskEvent::Abort),
                    "enter" if !key.modifiers.shift => this.submit(cx),
                    "enter" => this.text.push('\n'),
                    "backspace" => {
                        this.text.pop();
                    }
                    "v" if key.modifiers.control || key.modifiers.platform => {
                        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                            this.text.push_str(&text);
                        }
                    }
                    _ if !key.modifiers.control && !key.modifiers.platform => {
                        if let Some(text) = &key.key_char {
                            this.text.push_str(text);
                        }
                    }
                    _ => return,
                }
                cx.stop_propagation();
                cx.notify();
            }))
            .child(if self.text.is_empty() {
                if self.model_input {
                    "OpenRouter model id…"
                } else if self.secret {
                    "PDF password…"
                } else {
                    "Ask about this PDF…"
                }
                .into()
            } else if self.secret {
                "•".repeat(self.text.chars().count())
            } else {
                self.text.clone()
            })
            .child(
                div()
                    .id("send-question")
                    .text_sm()
                    .text_color(rgb(if can_send { 0x93c5fd } else { 0x71717a }))
                    .when(can_send, |button| {
                        button
                            .cursor_pointer()
                            .hover(|style| style.bg(rgb(0x3f3f46)))
                            .active(|style| style.bg(rgb(0x52525b)))
                    })
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.submit(cx);
                        cx.stop_propagation();
                    }))
                    .child(if self.model_input {
                        "Save model ↵"
                    } else if self.secret {
                        "Unlock ↵"
                    } else {
                        "Send ↵ · Shift+Enter for newline"
                    }),
            )
            .children(self.disabled_reason.as_ref().map(|_| {
                div()
                    .text_sm()
                    .text_color(rgb(0xa1a1aa))
                    .child("Send disabled — see Agent status. You can keep typing.")
            }))
    }
}
