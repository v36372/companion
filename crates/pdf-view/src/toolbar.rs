//! Viewer chrome uses the host's gpui-component Root and Theme; never installs a theme.
use gpui::{App, Context, Focusable, IntoElement, MouseButton, Window, div, prelude::*, px};
#[cfg(feature = "markup")]
use gpui_component::popover::Popover;
use gpui_component::{
    ActiveTheme, Disableable, Selectable, Sizable,
    button::{Button, ButtonVariants},
    input::{Input, InputEvent, InputState},
};

use crate::{FitMode, MAX_ZOOM, MIN_ZOOM, PdfView};
use gpui_kit_assets::IconName;

// Fixed chrome/overlay lanes: neither find nor palette changes the page viewport.
pub(super) const HEADER_HEIGHT: f32 = 72.;
#[cfg(feature = "search")]
pub(super) const FIND_HEIGHT: f32 = 44.;

pub(super) fn icon_button(id: &'static str, icon: IconName, tip: &'static str) -> Button {
    Button::new(id).ghost().small().icon(icon).tooltip(tip)
}

fn page_number(value: &str, total: usize) -> Option<usize> {
    let page = value.trim().parse::<usize>().ok()?;
    (page > 0 && page <= total).then(|| page - 1)
}

impl PdfView {
    /// Dismiss viewer input before a host's global Escape/abort handler runs.
    /// Returns false when no viewer input/popup needs Escape, so the host can abort.
    pub fn dismiss_input(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        #[cfg(feature = "markup")]
        if self.palette_open {
            self.palette_open = false;
            window.focus(&self.focus, cx);
            cx.notify();
            return true;
        }
        #[cfg(feature = "search")]
        if self.search_open {
            self.close_search(cx);
            window.focus(&self.focus, cx);
            return true;
        }
        if self.toolbar_input_focused(window, cx) {
            window.focus(&self.focus, cx);
            cx.notify();
            return true;
        }
        false
    }

    pub(super) fn toolbar_input_focused(&self, window: &Window, cx: &App) -> bool {
        let page = self
            .page_input
            .as_ref()
            .is_some_and(|input| input.read(cx).focus_handle(cx).is_focused(window));
        #[cfg(feature = "search")]
        let page = page
            || self
                .search_input
                .as_ref()
                .is_some_and(|input| input.read(cx).focus_handle(cx).is_focused(window));
        page
    }

    pub(super) fn render_toolbar(
        &mut self,
        name: String,
        current: usize,
        total: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        if self.page_input.is_none() {
            let input = cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("Page")
                    .submit_on_enter(true)
            });
            cx.subscribe_in(
                &input,
                window,
                |this, input, event: &InputEvent, window, cx| {
                    if let InputEvent::PressEnter { .. } = event {
                        if let Some(page) =
                            page_number(input.read(cx).value().as_ref(), this.dims.len())
                        {
                            this.go_to_page(page, cx);
                        }
                        // Invalid/out-of-range input cancels instead of unexpectedly jumping.
                        window.focus(&this.focus, cx);
                        cx.notify();
                    }
                },
            )
            .detach();
            self.page_input = Some(input);
        }
        let input = self.page_input.as_ref().unwrap();
        if !input.read(cx).focus_handle(cx).is_focused(window) {
            let value = (current + 1).to_string();
            if input.read(cx).value().as_ref() != value {
                input.update(cx, |input, cx| input.set_value(value, window, cx));
            }
        }
        if std::mem::take(&mut self.page_input_focus) {
            input.update(cx, |input, cx| {
                input.focus(window, cx);
                input.select_all(window, cx);
            });
        }

        let controls = div()
            .flex()
            .items_center()
            .gap_1()
            .flex_shrink_0()
            .child(
                icon_button("pdf-prev", IconName::ChevronLeft, "Previous page (PageUp)")
                    .disabled(current == 0)
                    .on_click(cx.listener(|this, _, _, cx| this.prev_page(cx))),
            )
            .child(
                Input::new(input)
                    .small()
                    .w(px(52.))
                    .aria_label("Page number (Cmd/Ctrl-Alt-G); Enter to jump"),
            )
            .child(
                div()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!("/ {total}")),
            )
            .child(
                icon_button("pdf-next", IconName::ChevronRight, "Next page (PageDown)")
                    .disabled(current + 1 >= total)
                    .on_click(cx.listener(|this, _, _, cx| this.next_page(cx))),
            )
            .child(
                icon_button("pdf-zoom-out", IconName::Minus, "Zoom out (Cmd/Ctrl-minus)")
                    .disabled(self.zoom <= MIN_ZOOM)
                    .on_click(cx.listener(|this, _, _, cx| this.zoom_out(cx))),
            )
            .child(
                Button::new("pdf-zoom-reset")
                    .ghost()
                    .small()
                    .label(format!("{}%", (self.zoom * 100.).round() as i32))
                    .tooltip("Reset zoom (Cmd/Ctrl-0)")
                    .on_click(cx.listener(|this, _, _, cx| this.reset_zoom(cx))),
            )
            .child(
                icon_button("pdf-zoom-in", IconName::Plus, "Zoom in (Cmd/Ctrl-plus)")
                    .disabled(self.zoom >= MAX_ZOOM)
                    .on_click(cx.listener(|this, _, _, cx| this.zoom_in(cx))),
            )
            .child(
                icon_button("pdf-fit-width", IconName::MoveHorizontal, "Fit width")
                    .selected(self.fit == Some(FitMode::Width))
                    .on_click(cx.listener(|this, _, _, cx| this.fit_width(cx))),
            )
            .child(
                icon_button("pdf-fit-page", IconName::Maximize, "Fit page")
                    .selected(self.fit == Some(FitMode::Page))
                    .on_click(cx.listener(|this, _, _, cx| this.fit_page(cx))),
            );

        #[cfg(feature = "markup")]
        let controls = controls
            .child(
                Button::new("pdf-select")
                    .ghost()
                    .small()
                    .icon(IconName::MousePointer)
                    .selected(!self.selecting)
                    .tooltip("Stop highlighting")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.selecting = false;
                        this.palette_open = false;
                        this.sel_drag = None;
                        cx.notify();
                    })),
            )
            .child(self.palette_popover(cx));

        #[cfg(feature = "search")]
        let controls = controls.child(
            icon_button("pdf-find", IconName::Search, "Find (Cmd/Ctrl-F)")
                .selected(self.search_open)
                .on_click(cx.listener(|this, _, window, cx| {
                    if this.search_open {
                        this.close_search(cx);
                        window.focus(&this.focus, cx);
                    } else {
                        this.toggle_search(cx);
                    }
                })),
        );

        div()
            .id("pdf-toolbar")
            .flex_shrink_0()
            .h(px(HEADER_HEIGHT))
            .flex()
            .flex_col()
            .justify_center()
            .px_3()
            .gap_1()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .border_b_1()
            .border_color(cx.theme().border)
            .text_sm()
            // Native inputs and popover triggers must not focus the page/start a drag.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .min_w_0()
                    .child(
                        icon_button(
                            "pdf-toc",
                            IconName::PanelLeft,
                            if self.has_outline() {
                                "Table of contents"
                            } else {
                                "This PDF has no table of contents"
                            },
                        )
                        .disabled(!self.has_outline())
                        .selected(self.toc_open)
                        .on_click(cx.listener(|this, _, _, cx| this.toggle_toc(cx))),
                    )
                    .child(div().flex_1().min_w_0().truncate().child(name))
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("{total} pages")),
                    ),
            )
            .child(controls)
            .into_any_element()
    }

    #[cfg(feature = "markup")]
    fn palette_popover(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let mut colors = div().flex().flex_col().gap_1();
        if self.palette.is_empty() {
            colors = colors.child(div().text_sm().child("Default highlight color"));
        }
        for (index, (name, color)) in self.palette.iter().enumerate() {
            colors = colors.child(
                Button::new(("pdf-swatch", index))
                    .ghost()
                    .small()
                    .label(name.clone())
                    .icon(gpui_component::Icon::new(IconName::Palette).text_color(*color))
                    .selected(index == self.active_color)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.active_color = index;
                        this.selecting = true;
                        this.palette_open = false;
                        window.focus(&this.focus, cx);
                        cx.notify();
                    })),
            );
        }
        // The find lane always sits immediately below the header. Offset only the
        // popover surface, not its trigger or the document, when both tools are open.
        #[cfg(feature = "search")]
        let offset = if self.search_open {
            FIND_HEIGHT + 8.
        } else {
            0.
        };
        #[cfg(not(feature = "search"))]
        let offset = 0.;
        Popover::new("pdf-palette")
            .open(self.palette_open)
            .on_open_change(cx.listener(|this, open: &bool, _, cx| {
                this.palette_open = *open;
                if *open {
                    this.selecting = true;
                }
                cx.notify();
            }))
            .trigger(
                Button::new("pdf-mark")
                    .ghost()
                    .small()
                    .icon(
                        gpui_component::Icon::new(IconName::Highlighter)
                            .text_color(self.active_color_hsla()),
                    )
                    .selected(self.selecting)
                    .tooltip("Highlight text / choose color (Cmd/Ctrl-Shift-H)"),
            )
            .offset(px(offset + 4.))
            .child(colors)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::page_number;
    #[test]
    fn page_input_accepts_only_one_based_pages_in_document() {
        assert_eq!(page_number(" 3 ", 17), Some(2));
        for value in ["", "0", "18", "-1", "python", "999999999999999999999999999"] {
            assert_eq!(page_number(value, 17), None);
        }
        assert_eq!(page_number("1", 0), None);
        assert_eq!(page_number("17", 17), Some(16));
    }
}
