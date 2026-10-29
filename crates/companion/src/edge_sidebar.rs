//! Timed sidebar policy. The caller supplies time; layout never drives the policy.
use std::time::Duration;

const DWELL: Duration = Duration::from_millis(150);
const LEAVE: Duration = Duration::from_millis(350);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mode {
    Hidden,
    Peek,
    Pinned,
}

pub(crate) struct EdgeSidebar {
    mode: Mode,
    edge_since: Option<Duration>,
    leave_since: Option<Duration>,
    edge: bool,
    panel: bool,
    pub(crate) width: f32,
}

impl Default for EdgeSidebar {
    fn default() -> Self {
        Self {
            mode: Mode::Hidden,
            edge_since: None,
            leave_since: None,
            edge: false,
            panel: false,
            width: 280.,
        }
    }
}

impl EdgeSidebar {
    pub(crate) fn load(catalog: Option<&companion_store::SqliteHub>) -> Self {
        let mut state = Self::default();
        if let Some(catalog) = catalog {
            match (catalog.sidebar_pinned(), catalog.sidebar_width()) {
                (Ok(pinned), Ok(width)) => {
                    if pinned {
                        state.mode = Mode::Pinned;
                    }
                    state.width = width;
                }
                _ => tracing::warn!("Cannot restore sidebar preferences"),
            }
        }
        state
    }

    pub(crate) fn mode(&self, has_document: bool) -> Mode {
        if has_document {
            self.mode
        } else {
            Mode::Pinned
        }
    }

    pub(crate) fn toggle_pin(&mut self) {
        self.mode = if self.mode == Mode::Pinned {
            Mode::Hidden
        } else {
            Mode::Pinned
        };
        self.edge_since = None;
        self.leave_since = None;
    }

    pub(crate) fn hide(&mut self) {
        if self.mode == Mode::Peek {
            self.mode = Mode::Hidden;
        }
        self.edge_since = None;
        self.leave_since = None;
    }

    pub(crate) fn pointer(&mut self, edge: bool, panel: bool, pressed: bool, now: Duration) {
        self.edge = edge;
        self.panel = panel;
        if !edge || pressed {
            self.edge_since = None;
        } else if self.edge_since.is_none() {
            self.edge_since = Some(now);
        }
        if edge || panel {
            self.leave_since = None;
        } else if self.mode == Mode::Peek && self.leave_since.is_none() {
            self.leave_since = Some(now);
        }
    }

    pub(crate) fn tick(&mut self, now: Duration, protected: bool) -> bool {
        let before = self.mode;
        if self.mode == Mode::Hidden
            && self
                .edge_since
                .is_some_and(|start| now.saturating_sub(start) >= DWELL)
        {
            self.mode = Mode::Peek;
        }
        if self.mode == Mode::Peek {
            if protected {
                self.leave_since = None;
            } else if !self.edge && !self.panel {
                let start = *self.leave_since.get_or_insert(now);
                if now.saturating_sub(start) >= LEAVE {
                    self.hide();
                }
            }
        }
        before != self.mode
    }
}

use super::CompanionWindow;
use gpui::{
    Animation, AnimationExt, Context, FocusHandle, IntoElement, Window, div, prelude::*, px,
};
use gpui_component::{
    ActiveTheme, IconName, Selectable, Sizable,
    button::{Button, ButtonVariants},
};

pub(super) struct Runtime {
    clock: std::time::Instant,
    focus: FocusHandle,
    started: bool,
    held: bool,
    tooltips: Vec<gpui::AnyWeakView>,
    pub(super) tab: Option<super::sidebar::SidebarTab>,
}

impl CompanionWindow {
    /// Escape priority hook: transient sidebar precedes viewer inputs/selection/abort.
    pub(super) fn dismiss_sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.edge_sidebar.mode(self.document.is_some()) != Mode::Peek {
            return false;
        }
        self.hide_sidebar(cx);
        window.focus(&self.focus, cx);
        true
    }

    pub(super) fn sidebar_runtime(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Entity<Runtime> {
        window.use_keyed_state("edge-sidebar-runtime", cx, |_, cx| Runtime {
            clock: std::time::Instant::now(),
            focus: cx.focus_handle(),
            started: false,
            held: false,
            tooltips: Vec::new(),
            tab: None,
        })
    }

    pub(super) fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.edge_sidebar.toggle_pin();
        if let Some(catalog) = &self.catalog
            && let Err(error) = catalog.set_sidebar_pinned(self.edge_sidebar.mode == Mode::Pinned)
        {
            tracing::warn!("Cannot save sidebar preference: {error:#}");
        }
        cx.notify();
    }

    pub(super) fn sidebar_resized(
        &mut self,
        state: &gpui::Entity<gpui_component::resizable::ResizableState>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.sidebar_pinned()
            && let Some(width) = state.read(cx).sizes().first()
        {
            self.remember_sidebar_width(f32::from(*width));
        }
    }

    pub(super) fn remember_sidebar_width(&mut self, width: f32) {
        self.edge_sidebar.width = width.clamp(220., 360.);
        if let Some(catalog) = &self.catalog
            && let Err(error) = catalog.set_sidebar_width(self.edge_sidebar.width)
        {
            tracing::warn!("Cannot save sidebar width: {error:#}");
        }
    }

    pub(super) fn sidebar_pinned(&self) -> bool {
        self.edge_sidebar.mode(self.document.is_some()) == Mode::Pinned
    }

    pub(super) fn hide_sidebar(&mut self, cx: &mut Context<Self>) {
        self.edge_sidebar.hide();
        cx.notify();
    }

    pub(super) fn sidebar_toggle(
        &self,
        id: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let runtime = self.sidebar_runtime(window, cx);
        div()
            .id(id)
            .tooltip(move |window, cx| {
                sidebar_tooltip("Pin sidebar (Ctrl/Cmd-B)", &runtime, window, cx)
            })
            .child(
                Button::new(id)
                    .ghost()
                    .small()
                    .icon(IconName::PanelLeft)
                    .accessibility_label("Pin sidebar")
                    .toggled(self.sidebar_pinned())
                    .selected(self.sidebar_pinned())
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.toggle_sidebar(cx);
                    })),
            )
    }

    pub(super) fn render_edge_sidebar(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let runtime = self.sidebar_runtime(window, cx);
        let focus = runtime.read(cx).focus.clone();
        let started = runtime.update(cx, |runtime, _| {
            std::mem::replace(&mut runtime.started, true)
        });
        if !started {
            let runtime = runtime.clone();
            cx.spawn_in(window, async move |this, cx| {
                loop {
                    cx.background_executor()
                        .timer(Duration::from_millis(30))
                        .await;
                    if this
                        .update_in(cx, |this, window, cx| {
                            runtime.update(cx, |runtime, _| {
                                if this.edge_sidebar.mode(this.document.is_some()) != Mode::Peek {
                                    runtime.held = false;
                                }
                                runtime
                                    .tooltips
                                    .retain(|tooltip| tooltip.upgrade().is_some())
                            });
                            let runtime = runtime.read(cx);
                            if this.edge_sidebar.tick(
                                runtime.clock.elapsed(),
                                runtime.held
                                    || !runtime.tooltips.is_empty()
                                    || runtime.focus.contains_focused(window, cx),
                            ) {
                                cx.notify();
                            }
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .detach();
        }
        let mode = self.edge_sidebar.mode(self.document.is_some());
        div()
            .absolute()
            .inset_0()
            .children((mode == Mode::Hidden).then(|| {
                let leave = runtime.clone();
                let movement = runtime.clone();
                div()
                    .id("sidebar-hot-zone")
                    .absolute()
                    .left_0()
                    .top_0()
                    .bottom_0()
                    .w(px(8.))
                    .on_mouse_move(
                        cx.listener(move |this, event: &gpui::MouseMoveEvent, _, cx| {
                            this.edge_sidebar.pointer(
                                true,
                                false,
                                event.pressed_button.is_some(),
                                movement.read(cx).clock.elapsed(),
                            );
                        }),
                    )
                    .on_hover(cx.listener(move |this, hovered, _, cx| {
                        if !hovered {
                            this.edge_sidebar.pointer(
                                false,
                                false,
                                false,
                                leave.read(cx).clock.elapsed(),
                            );
                        }
                    }))
            }))
            .children((mode == Mode::Peek).then(|| {
                let hover = runtime.clone();
                let down = runtime.clone();
                let up = runtime.clone();
                let outside = runtime.clone();
                let outside_right = runtime.clone();
                let outside_middle = runtime.clone();
                div()
                    .id("sidebar-peek")
                    .occlude()
                    .absolute()
                    .left_0()
                    .top_0()
                    .bottom_0()
                    .w(px(self.edge_sidebar.width.clamp(220., 360.)))
                    .bg(cx.theme().sidebar)
                    .shadow_md()
                    .track_focus(&focus)
                    .on_hover(cx.listener(move |this, hovered, _, cx| {
                        this.edge_sidebar.pointer(
                            false,
                            *hovered,
                            hover.read(cx).held,
                            hover.read(cx).clock.elapsed(),
                        );
                    }))
                    .capture_any_mouse_down(move |_, _, cx| {
                        down.update(cx, |runtime, _| runtime.held = true)
                    })
                    .capture_any_mouse_up(move |_, _, cx| {
                        up.update(cx, |runtime, _| runtime.held = false)
                    })
                    .on_mouse_up_out(gpui::MouseButton::Left, move |_, _, cx| {
                        outside.update(cx, |runtime, _| runtime.held = false)
                    })
                    .on_mouse_up_out(gpui::MouseButton::Right, move |_, _, cx| {
                        outside_right.update(cx, |runtime, _| runtime.held = false)
                    })
                    .on_mouse_up_out(gpui::MouseButton::Middle, move |_, _, cx| {
                        outside_middle.update(cx, |runtime, _| runtime.held = false)
                    })
                    .child(self.render_sidebar(window, cx))
                    .with_animation(
                        "sidebar-reveal",
                        Animation::new(Duration::from_millis(120)),
                        |panel, progress| panel.opacity(progress).left(px(-12. * (1. - progress))),
                    )
            }))
    }
}

pub(super) fn sidebar_tooltip(
    text: impl Into<gpui::SharedString>,
    runtime: &gpui::Entity<Runtime>,
    window: &mut Window,
    cx: &mut gpui::App,
) -> gpui::AnyView {
    let tooltip = gpui_component::tooltip::Tooltip::new(text.into()).build(window, cx);
    runtime.update(cx, |runtime, _| runtime.tooltips.push(tooltip.downgrade()));
    tooltip
}

#[cfg(test)]
mod tests {
    use super::*;
    fn ms(value: u64) -> Duration {
        Duration::from_millis(value)
    }

    #[test]
    fn dwell_and_quick_pass() {
        let mut state = EdgeSidebar::default();
        state.pointer(true, false, false, ms(0));
        assert!(!state.tick(ms(149), false));
        state.pointer(false, false, false, ms(149));
        assert!(!state.tick(ms(200), false));
        state.pointer(true, false, false, ms(300));
        assert!(state.tick(ms(450), false));
        assert_eq!(state.mode(true), Mode::Peek);
    }

    #[test]
    fn leave_delay_and_reentry() {
        let mut state = EdgeSidebar::default();
        state.pointer(true, false, false, ms(0));
        state.tick(ms(150), false);
        state.pointer(false, false, false, ms(200));
        assert!(!state.tick(ms(549), false));
        state.pointer(false, true, false, ms(549));
        assert!(!state.tick(ms(900), false));
        state.pointer(false, false, false, ms(1000));
        assert!(state.tick(ms(1350), false));
        assert_eq!(state.mode(true), Mode::Hidden);
    }

    #[test]
    fn pressed_button_blocks_dwell_and_protection_restarts_leave_delay() {
        let mut state = EdgeSidebar::default();
        state.pointer(true, false, true, ms(0));
        assert!(!state.tick(ms(500), false));
        state.pointer(true, false, false, ms(600));
        state.tick(ms(750), false);
        state.pointer(false, false, false, ms(800));
        assert!(!state.tick(ms(1200), true));
        assert!(!state.tick(ms(1300), false));
        assert!(state.tick(ms(1650), false));
    }

    #[test]
    fn pin_keyboard_cycle_and_no_document_override() {
        let mut state = EdgeSidebar::default();
        assert_eq!(state.mode(false), Mode::Pinned);
        assert_eq!(state.mode(true), Mode::Hidden);
        state.toggle_pin();
        assert_eq!(state.mode(true), Mode::Pinned);
        state.toggle_pin();
        assert_eq!(state.mode(true), Mode::Hidden);
        state.pointer(true, false, false, ms(0));
        state.tick(ms(150), false);
        state.toggle_pin();
        assert_eq!(state.mode(true), Mode::Pinned);
        assert_eq!(state.mode(false), Mode::Pinned);
        state.toggle_pin();
        assert_eq!(state.mode(false), Mode::Pinned);
        assert_eq!(state.mode(true), Mode::Hidden);
    }
}
