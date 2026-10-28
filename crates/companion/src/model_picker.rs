//! Compact, local-only OpenRouter preference editor.
use crate::ask_bar::{AskBar, AskEvent};
use companion_store::{
    SqliteHub,
    settings::{DEFAULT_MODEL_ID, MODEL_CHOICES},
};
use gpui::{Context, Entity, Render, Window, div, prelude::*, rgb};
use std::sync::Arc;

pub struct ModelPicker {
    hub: Option<Arc<SqliteHub>>,
    selected: String,
    open: bool,
    custom: bool,
    input: Entity<AskBar>,
    error: Option<String>,
    pub disabled_reason: Option<String>,
}

impl ModelPicker {
    pub fn new(hub: Option<Arc<SqliteHub>>, cx: &mut Context<Self>) -> Self {
        let saved = hub.as_ref().map(|hub| hub.model_id()).transpose();
        let (selected, error) = match saved {
            Ok(value) => (value.unwrap_or_else(|| DEFAULT_MODEL_ID.into()), None),
            Err(error) => (DEFAULT_MODEL_ID.into(), Some(error.to_string())),
        };
        let input = cx.new(|cx| {
            let mut input = AskBar::new(cx);
            input.model_input = true;
            input
        });
        cx.subscribe(&input, |this, _, event: &AskEvent, cx| {
            match event {
                AskEvent::Submit(value) => this.save(value, cx),
                AskEvent::Abort => this.open = false,
            }
            cx.notify();
        })
        .detach();
        Self {
            hub,
            selected,
            open: false,
            custom: false,
            input,
            error,
            disabled_reason: None,
        }
    }

    fn save(&mut self, value: &str, cx: &mut Context<Self>) {
        if self.disabled_reason.is_some() {
            return;
        }
        let result = self
            .hub
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Settings are unavailable."))
            .and_then(|hub| hub.set_model_id(value));
        match result {
            Ok(()) => {
                self.selected = value.trim().to_owned();
                self.error = None;
                self.open = false;
            }
            Err(error) => self.error = Some(error.to_string()),
        }
        cx.notify();
    }
}

impl Render for ModelPicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let label = MODEL_CHOICES
            .iter()
            .find(|(id, _)| *id == self.selected)
            .map(|(_, label)| (*label).to_owned())
            .unwrap_or_else(|| self.selected.clone());
        div()
            .px_4()
            .py_2()
            .text_sm()
            .child(
                div()
                    .id("model-picker")
                    .text_color(rgb(if self.disabled_reason.is_some() {
                        0x71717a
                    } else {
                        0x93c5fd
                    }))
                    .when(self.disabled_reason.is_none(), |button| {
                        button
                            .cursor_pointer()
                            .hover(|style| style.bg(rgb(0x3f3f46)))
                            .active(|style| style.bg(rgb(0x52525b)))
                    })
                    .child(format!("Model: {label} ▾"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        if this.disabled_reason.is_none() {
                            this.open = !this.open;
                        }
                        cx.notify();
                    })),
            )
            .children((self.open && self.disabled_reason.is_none()).then(|| {
                div()
                    .mt_2()
                    .p_2()
                    .rounded_md()
                    .bg(rgb(0x27272a))
                    .children(
                        MODEL_CHOICES
                            .iter()
                            .enumerate()
                            .map(|(index, &(id, label))| {
                                div()
                                    .id(("model-choice", index))
                                    .py_2()
                                    .cursor_pointer()
                                    .child(format!(
                                        "{} {label}",
                                        if id == self.selected { "✓" } else { " " }
                                    ))
                                    .on_click(cx.listener(move |this, _, _, cx| this.save(id, cx)))
                            }),
                    )
                    .child(
                        div()
                            .id("custom-model")
                            .py_2()
                            .cursor_pointer()
                            .child("Custom…")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.custom = true;
                                this.input.update(cx, |input, cx| {
                                    input.text = this.selected.clone();
                                    input.focus(window, cx);
                                    cx.notify();
                                });
                                cx.notify();
                            })),
                    )
                    .children(self.custom.then(|| self.input.clone()))
                    .child(
                        div()
                            .mt_2()
                            .text_color(rgb(0xa1a1aa))
                            .child("Applies to the next turn; current run is unchanged."),
                    )
            }))
            .children(
                self.error
                    .clone()
                    .map(|error| div().text_color(rgb(0xfca5a5)).child(error)),
            )
    }
}
