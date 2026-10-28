//! Native, IME-capable input shared by chat, password and custom-model entry.
use gpui::{Context, Entity, EventEmitter, FocusHandle, Render, Window, div, prelude::*};
use gpui_component::{
    ActiveTheme, Disableable, IconName,
    button::{Button, ButtonVariants},
    input::{Input, InputEvent, InputState, Textarea, TextareaState},
};

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
    pub running: bool,
    input: Option<Entity<InputState>>,
    textarea: Option<Entity<TextareaState>>,
}
impl AskBar {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus: cx.focus_handle(),
            text: String::new(),
            secret: false,
            model_input: false,
            disabled_reason: None,
            running: false,
            input: None,
            textarea: None,
        }
    }
    pub fn focus(&self, window: &mut Window, cx: &mut gpui::App) {
        if let Some(input) = &self.input {
            input.update(cx, |input, cx| input.focus(window, cx));
        } else if let Some(input) = &self.textarea {
            input.update(cx, |input, cx| input.focus(window, cx));
        } else {
            self.focus.focus(window, cx);
        }
    }
    fn submit(&mut self, cx: &mut Context<Self>) {
        if self.disabled_reason.is_none() && (self.model_input || !self.text.trim().is_empty()) {
            cx.emit(AskEvent::Submit(self.text.clone()));
        }
    }
}
impl EventEmitter<AskEvent> for AskBar {}
impl Render for AskBar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Created on first render because native input state requires a Window.
        macro_rules! init_input {
            ($field:ident, $state:ident, $placeholder:expr) => {
                if self.$field.is_none() {
                    let input = cx.new(|cx| {
                        $state::new(window, cx)
                            .placeholder($placeholder)
                            .submit_on_enter(true)
                    });
                    cx.subscribe(&input, |this, input, event: &InputEvent, cx| {
                        match event {
                            InputEvent::Change => this.text = input.read(cx).value().to_string(),
                            InputEvent::PressEnter { shift: false, .. } => this.submit(cx),
                            _ => {}
                        }
                        cx.notify();
                    })
                    .detach();
                    self.$field = Some(input);
                }
                let input = self.$field.as_ref().unwrap();
                if input.read(cx).value().as_ref() != self.text {
                    input.update(cx, |input, cx| {
                        input.set_value(self.text.clone(), window, cx)
                    });
                }
                if self.focus.is_focused(window) {
                    input.update(cx, |input, cx| input.focus(window, cx));
                }
            };
        }
        let editor = if self.secret || self.model_input {
            init_input!(
                input,
                InputState,
                if self.secret {
                    "PDF password…"
                } else {
                    "OpenRouter model id…"
                }
            );
            let input = self.input.as_ref().unwrap();
            if input.read(cx).presentation().is_masked() != self.secret {
                input.update(cx, |input, cx| input.set_masked(self.secret, window, cx));
            }
            Input::new(input).into_any_element()
        } else {
            init_input!(textarea, TextareaState, "Ask about this PDF…");
            Textarea::new(self.textarea.as_ref().unwrap())
                .h(gpui::px(96.))
                .into_any_element()
        };
        let can_send =
            self.disabled_reason.is_none() && (self.model_input || !self.text.trim().is_empty());
        div()
            .id("ask-composer")
            .track_focus(&self.focus)
            .flex()
            .flex_col()
            .gap_2()
            .w_full()
            .min_w_0()
            .on_key_down(cx.listener(|_, event: &gpui::KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    cx.emit(AskEvent::Abort);
                    cx.stop_propagation();
                }
            }))
            .child(editor)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .flex_wrap()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(if self.secret || self.model_input {
                                "Enter to confirm"
                            } else {
                                "Shift+Enter for newline"
                            }),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .when(self.running, |row| {
                                row.child(
                                    Button::new("stop-question")
                                        .outline()
                                        .icon(IconName::CircleX)
                                        .label("Stop")
                                        .tooltip("Stop generation (Escape)")
                                        .on_click(
                                            cx.listener(|_, _, _, cx| cx.emit(AskEvent::Abort)),
                                        ),
                                )
                            })
                            .child(
                                Button::new("send-question")
                                    .primary()
                                    .icon(IconName::ArrowUp)
                                    .label(if self.model_input {
                                        "Save"
                                    } else if self.secret {
                                        "Unlock"
                                    } else {
                                        "Send"
                                    })
                                    .disabled(!can_send)
                                    .tooltip(
                                        self.disabled_reason
                                            .clone()
                                            .unwrap_or_else(|| "Send (Enter)".into()),
                                    )
                                    .on_click(cx.listener(|this, _, _, cx| this.submit(cx))),
                            ),
                    ),
            )
    }
}
