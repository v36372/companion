//! Local-only model preference editor, in an overlay rather than document flow.
use crate::ask_bar::{AskBar, AskEvent};
use companion_store::{
    SqliteHub,
    settings::{DEFAULT_MODEL_ID, MODEL_CHOICES},
};
use gpui::{AnyElement, Context, Entity, Render, Window, div, prelude::*};
use gpui_component::{
    ActiveTheme, Disableable, Sizable,
    button::{Button, ButtonVariants},
    popover::Popover,
};
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
    fn menu(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .w(gpui::px(300.))
            .flex()
            .flex_col()
            .gap_1()
            .children(
                MODEL_CHOICES
                    .iter()
                    .enumerate()
                    .map(|(index, &(id, label))| {
                        Button::new(("model-choice", index))
                            .ghost()
                            .label(format!(
                                "{} {label}",
                                if id == self.selected { "✓" } else { " " }
                            ))
                            .disabled(self.disabled_reason.is_some())
                            .on_click(cx.listener(move |this, _, _, cx| this.save(id, cx)))
                    }),
            )
            .child(
                Button::new("custom-model")
                    .ghost()
                    .label("Custom…")
                    .disabled(self.disabled_reason.is_some())
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
            .when(self.custom, |menu| menu.child(self.input.clone()))
            .children(
                self.error
                    .clone()
                    .map(|error| div().text_sm().text_color(cx.theme().danger).child(error)),
            )
            .child(
                div()
                    .pt_2()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child("Applies to the next turn; the current run is unchanged."),
            )
            .into_any_element()
    }
}
impl Render for ModelPicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let label = MODEL_CHOICES
            .iter()
            .find(|(id, _)| *id == self.selected)
            .map(|(_, label)| (*label).to_owned())
            .unwrap_or_else(|| self.selected.clone());
        let weak = cx.entity().downgrade();
        div().px_3().child(
            Popover::new("model-picker-popover")
                .open(self.open && self.disabled_reason.is_none())
                .on_open_change(cx.listener(|this, open, _, cx| {
                    this.open = *open && this.disabled_reason.is_none();
                    cx.notify();
                }))
                .trigger(
                    Button::new("model-picker")
                        .max_w_full()
                        .outline()
                        .small()
                        .label(format!("Model: {label}"))
                        .dropdown_caret(true)
                        .disabled(self.disabled_reason.is_some())
                        .tooltip(
                            self.disabled_reason
                                .clone()
                                .unwrap_or_else(|| "Choose a model for your next turn".into()),
                        ),
                )
                .content(move |_, _, cx| {
                    weak.update(cx, |this, cx| this.menu(cx))
                        .unwrap_or_else(|_| div().into_any_element())
                }),
        )
    }
}
