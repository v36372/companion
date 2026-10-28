use super::*;
use gpui_component::{
    ActiveTheme, Disableable, Sizable,
    button::Button,
    list::ListItem,
    scroll::ScrollableElement,
    tab::{Tab, TabBar},
    tooltip::Tooltip,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SidebarTab {
    Outline,
    Recent,
}

fn default_tab(has_outline: bool) -> SidebarTab {
    if has_outline {
        SidebarTab::Outline
    } else {
        SidebarTab::Recent
    }
}

/// Prefer the latest destination, then its deepest section. A deep entry in an
/// earlier chapter must not remain highlighted when the next chapter begins.
fn current_section(outline: &[gpui_pdf::OutlineItem], page_0: u32) -> Option<usize> {
    outline
        .iter()
        .enumerate()
        .filter_map(|(index, item)| {
            item.page
                .filter(|page| *page <= page_0 as usize)
                .map(|page| ((page, item.level, index), index))
        })
        .max_by_key(|(key, _)| *key)
        .map(|(_, index)| index)
}

impl CompanionWindow {
    pub(crate) fn render_sidebar(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let snapshot = self.pdf.as_ref().and_then(|pdf| pdf.snapshot.as_ref());
        let outline = snapshot
            .map(|snapshot| snapshot.outline.as_slice())
            .unwrap_or_default();
        // None means automatic until the user actually chooses a tab. In
        // particular an asynchronously loaded outline can become the default.
        let choice = window.use_keyed_state("sidebar-tab-choice", cx, |_, _| None::<SidebarTab>);
        let tab = choice
            .read(cx)
            .unwrap_or_else(|| default_tab(!outline.is_empty()));
        let current = current_section(outline, self.page_0);
        let spacing = cx.theme().spacing_tokens();
        let typography = cx.theme().typography_tokens();
        let muted = cx.theme().muted_foreground;
        let hovered = cx.theme().tokens.list_hover;
        let pressed = cx.theme().list_active;
        let radius = cx.theme().radius;
        let tabs = TabBar::new("sidebar-tabs")
            .segmented()
            .small()
            .w_full()
            .selected_index(usize::from(tab == SidebarTab::Recent))
            .child(Tab::new().label("Outline"))
            .child(Tab::new().label("Recent"))
            .on_click(move |index, _, cx| {
                choice.update(cx, |choice, cx| {
                    *choice = Some(if *index == 0 {
                        SidebarTab::Outline
                    } else {
                        SidebarTab::Recent
                    });
                    cx.notify();
                });
            });
        let open_button = || {
            Button::new("sidebar-open-pdf")
                .outline()
                .small()
                .label(if self.dialog_open {
                    "Choosing PDF…"
                } else {
                    "Open PDF…"
                })
                .disabled(self.dialog_open)
                .on_click(cx.listener(|this, _, window, cx| this.pick_pdf(window, cx)))
        };
        let empty = |message: &'static str, hint: &'static str| {
            div()
                .flex()
                .flex_col()
                .gap(spacing.sm)
                .p(spacing.md)
                .child(message)
                .child(
                    div()
                        .text_color(muted)
                        .text_size(typography.xs.size)
                        .child(hint),
                )
        };

        let content = if self.document.is_none() && self.recent.is_empty() {
            empty(
                "Your reading starts here",
                "Open a PDF to explore its outline and keep it in Recent.",
            )
            .child(open_button())
            .into_any_element()
        } else if tab == SidebarTab::Outline {
            if outline.is_empty() {
                let (message, hint) = if self.document.is_none() {
                    ("No PDF open", "Choose a document in Recent, or open a PDF.")
                } else if snapshot.is_none() {
                    (
                        "Outline not ready",
                        "Unlock the PDF if prompted, or use Recent while it loads.",
                    )
                } else {
                    ("This PDF has no outline", "Use Recent to switch documents.")
                };
                empty(message, hint).into_any_element()
            } else {
                div()
                    .flex()
                    .flex_col()
                    .gap(spacing.xxs)
                    .children(outline.iter().cloned().enumerate().map(|(index, item)| {
                        let available = item.page.is_some();
                        let tooltip = if available {
                            item.title.clone()
                        } else {
                            format!("{} — No destination in this PDF", item.title)
                        };
                        // Indentation is bounded so malformed depths cannot hide
                        // the title completely in a narrow sidebar.
                        div()
                            .pl(spacing.md * item.level.min(6) as f32)
                            .min_w_0()
                            .child(
                                ListItem::new(gpui::SharedString::from(format!("outline-{index}")))
                                    .w_full()
                                    .min_w_0()
                                    .rounded(radius)
                                    .text_size(typography.sm.size)
                                    .selected(current == Some(index))
                                    .disabled(!available)
                                    .when(available, |row| {
                                        row.cursor_pointer()
                                            .hover(move |style| style.bg(hovered))
                                            .active(move |style| style.bg(pressed))
                                    })
                                    .when(!available, |row| row.opacity(0.45))
                                    .tooltip(move |window, cx| {
                                        Tooltip::new(tooltip.clone()).build(window, cx)
                                    })
                                    .child(div().w_full().min_w_0().truncate().child(item.title))
                                    .when_some(item.page, |row, page_0| {
                                        row.on_click(cx.listener(move |this, _, _, cx| {
                                            this.goto_explicit(page_0 as u32, cx);
                                        }))
                                    }),
                            )
                    }))
                    .into_any_element()
            }
        } else if self.recent.is_empty() {
            empty("No recent PDFs", "Documents you open will appear here.")
                .child(open_button())
                .into_any_element()
        } else {
            div()
                .flex()
                .flex_col()
                .gap(spacing.xxs)
                .children(self.recent.iter().cloned().map(|document| {
                    let is_open = self
                        .document
                        .as_ref()
                        .is_some_and(|open| open.id == document.id);
                    let pages = document.page_count.or_else(|| {
                        snapshot
                            .filter(|_| is_open)
                            .map(|snapshot| snapshot.meta.page_count)
                    });
                    let detail = match (pages, is_open) {
                        (Some(count), true) => format!("{count} pages · Open"),
                        (Some(count), false) => format!("{count} pages"),
                        (None, true) => "Open".into(),
                        (None, false) => "PDF".into(),
                    };
                    let title = document.title.clone();
                    ListItem::new(gpui::SharedString::from(format!("recent-{}", document.id)))
                        .w_full()
                        .min_w_0()
                        .py(spacing.sm)
                        .rounded(radius)
                        .selected(is_open)
                        .cursor_pointer()
                        .hover(move |style| style.bg(hovered))
                        .active(move |style| style.bg(pressed))
                        .tooltip(move |window, cx| Tooltip::new(title.clone()).build(window, cx))
                        .child(
                            div()
                                .w_full()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap(spacing.xxs)
                                .child(
                                    div()
                                        .truncate()
                                        .text_size(typography.sm.size)
                                        .child(document.title.clone()),
                                )
                                .child(
                                    div()
                                        .text_color(muted)
                                        .text_size(typography.xs.size)
                                        .child(detail),
                                ),
                        )
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.open_document(document.clone(), window, cx)
                        }))
                }))
                .into_any_element()
        };

        div()
            .id("sidebar")
            .flex()
            .flex_col()
            .w_full()
            .min_w_0()
            .h_full()
            .min_h_0()
            .bg(cx.theme().sidebar)
            .text_color(cx.theme().sidebar_foreground)
            .border_r_1()
            .border_color(cx.theme().border)
            .child(div().p(spacing.sm).flex_shrink_0().child(tabs))
            .child(
                div()
                    .id("sidebar-content")
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .p(spacing.xs)
                    .child(content)
                    .overflow_y_scrollbar()
                    .id(if tab == SidebarTab::Outline {
                        "outline-scroll"
                    } else {
                        "recent-scroll"
                    }),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(page: Option<usize>, level: usize) -> gpui_pdf::OutlineItem {
        gpui_pdf::OutlineItem {
            title: "Section".into(),
            page,
            level,
        }
    }

    #[test]
    fn tab_defaults_to_outline_only_when_present() {
        assert_eq!(default_tab(true), SidebarTab::Outline);
        assert_eq!(default_tab(false), SidebarTab::Recent);
    }

    #[test]
    fn current_section_tracks_latest_destination_and_deepest_tie() {
        let outline = [
            entry(Some(2), 0),
            entry(Some(2), 1),
            entry(None, 3),
            entry(Some(10), 0),
            entry(Some(12), 1),
        ];
        assert_eq!(current_section(&outline, 0), None);
        assert_eq!(current_section(&outline, 2), Some(1));
        assert_eq!(current_section(&outline, 9), Some(1));
        assert_eq!(current_section(&outline, 10), Some(3));
        assert_eq!(current_section(&outline, 50), Some(4));
        assert_eq!(current_section(&[], 50), None);
        assert_eq!(current_section(&[entry(None, 0)], 50), None);
    }
}
