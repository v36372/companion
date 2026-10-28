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
}
impl AskBar {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus: cx.focus_handle(),
            text: String::new(),
            secret: false,
            model_input: false,
        }
    }
    pub fn focus(&self, window: &mut Window, cx: &mut gpui::App) {
        self.focus.focus(window, cx);
    }
    fn submit(&mut self, cx: &mut Context<Self>) {
        if self.model_input || !self.text.trim().is_empty() {
            cx.emit(AskEvent::Submit(self.text.clone()));
        }
    }
}
impl EventEmitter<AskEvent> for AskBar {}
impl Render for AskBar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
                    .cursor_pointer()
                    .text_sm()
                    .text_color(rgb(0x93c5fd))
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
    }
}
