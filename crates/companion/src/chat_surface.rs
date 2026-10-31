//! Wide chat surface: status, conversation, composer; secondary controls live in ⋯.
use crate::{
    agent_bridge::{Availability, Bridge},
    ask_bar::{AskBar, AskEvent},
    diagram::DiagramViewer,
};
use gpui::{Context, Entity, EventEmitter, Render, Task, Window, div, prelude::*, px};
use gpui_component::{
    ActiveTheme, Disableable, Sizable,
    button::{Button, ButtonVariants},
    popover::Popover,
    switch::Switch,
    text::TextView,
};
use std::{rc::Rc, sync::Arc, time::Duration};

#[cfg(any(test, feature = "agent"))]
use crate::agent_bridge::Event as BridgeEvent;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReaderState {
    pub status: crate::orb::AgentStatus,
    pub label: &'static str,
    pub disabled_reason: Option<String>,
}

impl ReaderState {
    fn new(bridge: &Bridge, open: bool, unread: bool) -> Self {
        let status = crate::orb::agent_status(bridge, open, unread);
        Self {
            status,
            label: crate::orb::tooltip_label(bridge, status),
            disabled_reason: bridge
                .unavailable_reason()
                .map(|_| crate::chat::status_label(bridge).to_owned()),
        }
    }
}

pub enum Event {
    ReaderChanged,
    Navigate(u32),
    Close,
}

pub struct ChatSurface {
    bridge: Bridge,
    composer: Entity<AskBar>,
    model_picker: Entity<crate::model_picker::ModelPicker>,
    viewer: Entity<DiagramViewer>,
    scroll: gpui::ScrollHandle,
    open: bool,
    unread: bool,
    pub reader_state: ReaderState,
    redraw: Option<Task<()>>,
    #[cfg(test)]
    renders: usize,
}

impl EventEmitter<Event> for ChatSurface {}

impl ChatSurface {
    pub fn new(
        catalog: Option<Arc<companion_store::SqliteHub>>,
        viewer: Entity<DiagramViewer>,
        cx: &mut Context<Self>,
    ) -> Self {
        #[cfg(feature = "agent")]
        let bridge = if let Some(hub) = catalog.clone() {
            let (bridge, events) = Bridge::start(hub);
            cx.spawn(async move |this, cx| {
                while let Ok(event) = events.recv().await {
                    if this.update(cx, |this, cx| this.event(event, cx)).is_err() {
                        break;
                    }
                }
            })
            .detach();
            bridge
        } else {
            Bridge::off()
        };
        #[cfg(not(feature = "agent"))]
        let bridge = Bridge::off();
        Self::with_bridge(bridge, catalog, viewer, cx)
    }

    fn with_bridge(
        bridge: Bridge,
        catalog: Option<Arc<companion_store::SqliteHub>>,
        viewer: Entity<DiagramViewer>,
        cx: &mut Context<Self>,
    ) -> Self {
        let composer = cx.new(AskBar::new);
        cx.subscribe(&composer, |this, _, event: &AskEvent, cx| {
            match event {
                AskEvent::Submit(text) => {
                    if this.bridge.submit(text.clone(), None) {
                        this.composer.update(cx, |bar, cx| {
                            bar.text.clear();
                            cx.notify();
                        });
                    }
                }
                AskEvent::Abort => this.bridge.abort(),
            }
            this.changed(false, cx);
        })
        .detach();
        // Cached views bake in colors; theme changes must invalidate the surface.
        cx.observe_global::<gpui_component::Theme>(|_, cx| cx.notify())
            .detach();
        let reader_state = ReaderState::new(&bridge, false, false);
        let mut surface = Self {
            bridge,
            composer,
            model_picker: cx.new(|cx| crate::model_picker::ModelPicker::new(catalog, cx)),
            viewer,
            scroll: gpui::ScrollHandle::new(),
            open: false,
            unread: false,
            reader_state,
            redraw: None,
            #[cfg(test)]
            renders: 0,
        };
        surface.changed(false, cx);
        surface
    }

    pub fn shutdown(&self) -> Option<async_channel::Receiver<()>> {
        self.bridge.shutdown()
    }

    pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.composer.update(cx, |bar, cx| bar.focus(window, cx));
    }

    pub fn set_open(&mut self, open: bool, cx: &mut Context<Self>) {
        self.open = open;
        self.unread = false;
        self.changed(false, cx);
    }

    pub fn update_bridge<R>(
        &mut self,
        update: impl FnOnce(&mut Bridge) -> R,
        cx: &mut Context<Self>,
    ) -> R {
        let result = update(&mut self.bridge);
        self.changed(false, cx);
        result
    }

    #[cfg(any(test, feature = "agent"))]
    fn event(&mut self, event: BridgeEvent, cx: &mut Context<Self>) {
        let stream = matches!(event, BridgeEvent::Stream(_));
        let was_running = self.bridge.running;
        self.bridge.event(event);
        self.unread =
            crate::orb::unread_after(self.unread, was_running, self.bridge.running, self.open);
        self.changed(stream, cx);
    }

    fn changed(&mut self, stream: bool, cx: &mut Context<Self>) {
        let state = ReaderState::new(&self.bridge, self.open, self.unread);
        let reader_changed = state != self.reader_state;
        self.reader_state = state;
        if reader_changed {
            cx.emit(Event::ReaderChanged);
        }
        self.composer.update(cx, |bar, cx| {
            if bar.running != self.bridge.running
                || bar.disabled_reason != self.reader_state.disabled_reason
            {
                bar.running = self.bridge.running;
                bar.disabled_reason = self.reader_state.disabled_reason.clone();
                cx.notify();
            }
        });
        self.model_picker.update(cx, |picker, cx| {
            if picker.disabled_reason != self.reader_state.disabled_reason {
                picker.disabled_reason = self.reader_state.disabled_reason.clone();
                cx.notify();
            }
        });
        if !self.open {
            self.redraw = None;
        } else if !stream || reader_changed {
            // Input, completion and errors never wait behind a streaming timer.
            self.redraw = None;
            crate::chat::follow_tail(&self.scroll);
            cx.notify();
        } else if self.redraw.is_none() {
            // Ingest every event, but rebuild a visible stream at most ~30 Hz.
            self.redraw = Some(cx.spawn(async |this, cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(33))
                    .await;
                let _ = this.update(cx, |this, cx| {
                    this.redraw = None;
                    crate::chat::follow_tail(&this.scroll);
                    cx.notify();
                });
            }));
        }
    }
}

impl Render for ChatSurface {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        #[cfg(test)]
        {
            self.renders += 1;
        }
        let weak = cx.weak_entity();
        let retry = weak.clone();
        column(
            &self.bridge,
            self.composer.clone(),
            self.model_picker.clone(),
            self.viewer.clone(),
            &self.scroll,
            cx.listener(|this, _, _, cx| this.update_bridge(Bridge::toggle_follow, cx)),
            cx.listener(|this, _, _, cx| {
                this.update_bridge(
                    |bridge| {
                        bridge.submit("What did I just read?".into(), None);
                    },
                    cx,
                );
            }),
            move |page, _, cx| {
                let _ = weak.update(cx, |_, cx| cx.emit(Event::Navigate(page)));
            },
            move |_, cx| {
                let _ = retry.update(cx, |this, cx| this.update_bridge(Bridge::retry_parked, cx));
            },
            cx.listener(|_, _, _, cx| cx.emit(Event::Close)),
            cx,
        )
    }
}

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
fn column(
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
) -> gpui::AnyElement {
    let status = crate::orb::agent_status(bridge, true, false);
    let status_text = match status {
        crate::orb::AgentStatus::Idle => "",
        crate::orb::AgentStatus::Thinking | crate::orb::AgentStatus::Streaming => status.label(),
        _ => crate::chat::status_label(bridge),
    };
    let can_chat = bridge.can_chat();
    let checked = bridge.follow;
    let follow = Rc::new(follow);
    let ask = Rc::new(ask);
    div().flex().flex_col().h_full().w_full().min_w_0().bg(cx.theme().background).text_color(cx.theme().foreground)
        .child(div().flex().items_center().justify_between().px_3().py_2().gap_2().border_b_1().border_color(cx.theme().border)
            .child(div().flex().items_center().gap_2().child(crate::orb::core(status, 16., cx))
                .child(div().text_sm().text_color(cx.theme().muted_foreground).child(status_text)))
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
        .children(match &bridge.availability {
            Availability::KeyError(reason) | Availability::Failed(reason) => Some(
                div().px_3().py_2().text_sm().text_color(cx.theme().muted_foreground).child(reason.clone())
            ),
            _ => None,
        })
        .child(div().flex_1().min_h_0().child(crate::chat::pane(bridge, composer, viewer, scroll, navigate, retry, cx)))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{AppContext, TestAppContext};
    use std::cell::Cell;

    fn surface(cx: &mut TestAppContext) -> Entity<ChatSurface> {
        cx.update(gpui_component::init);
        cx.new(|cx| {
            let viewer = cx.new(|_| DiagramViewer::default());
            let mut bridge = Bridge::off();
            bridge.availability = Availability::Ready;
            bridge.conversation = Some(1);
            bridge.running = true;
            bridge.status = "Running".into();
            ChatSurface::with_bridge(bridge, None, viewer, cx)
        })
    }

    fn stream(text: &str) -> BridgeEvent {
        BridgeEvent::Stream(serde_json::json!({
            "conversationId": 1,
            "live": {"generation": {"message": {"content": text}}}
        }))
    }

    #[gpui::test]
    fn stream_bursts_notify_once_and_completion_cancels_the_timer(cx: &mut TestAppContext) {
        let chat = surface(cx);
        chat.update(cx, |chat, cx| {
            chat.set_open(true, cx);
            chat.event(stream("first"), cx);
        });
        let notifications = Rc::new(Cell::new(0));
        let _observe = cx.update(|cx| {
            let notifications = notifications.clone();
            cx.observe(&chat, move |_, _| {
                notifications.set(notifications.get() + 1)
            })
        });
        for i in 0..100 {
            chat.update(cx, |chat, cx| chat.event(stream(&format!("chunk {i}")), cx));
        }
        cx.run_until_parked();
        assert_eq!(notifications.get(), 0);
        cx.background_executor
            .advance_clock(Duration::from_millis(32));
        cx.run_until_parked();
        assert_eq!(notifications.get(), 0);
        cx.background_executor
            .advance_clock(Duration::from_millis(1));
        cx.run_until_parked();
        assert_eq!(notifications.get(), 1);
        cx.read(|cx| assert_eq!(chat.read(cx).bridge.live, "chunk 99"));

        chat.update(cx, |chat, cx| chat.event(stream("pending"), cx));
        cx.run_until_parked();
        chat.update(cx, |chat, cx| chat.event(BridgeEvent::Settled, cx));
        assert_eq!(notifications.get(), 2, "completion must be immediate");
        cx.background_executor
            .advance_clock(Duration::from_millis(33));
        cx.run_until_parked();
        assert_eq!(notifications.get(), 2, "no stale streaming redraw");
        chat.update(cx, |chat, cx| {
            chat.update_bridge(|bridge| bridge.running = true, cx);
            chat.event(stream("writing"), cx);
        });
        let before = notifications.get();
        chat.update(cx, |chat, cx| chat.event(stream("pending abort"), cx));
        chat.update(cx, |chat, cx| chat.update_bridge(Bridge::abort, cx));
        assert_eq!(notifications.get(), before + 1, "abort must be immediate");
        chat.update(cx, |chat, cx| chat.event(stream("pending error"), cx));
        chat.update(cx, |chat, cx| {
            chat.event(BridgeEvent::Error("failed".into()), cx)
        });
        assert_eq!(notifications.get(), before + 2, "errors must be immediate");
        cx.background_executor
            .advance_clock(Duration::from_millis(33));
        cx.run_until_parked();
        assert_eq!(notifications.get(), before + 2);
    }

    #[gpui::test]
    fn hidden_chat_ingests_without_redrawing_and_reopens_current(cx: &mut TestAppContext) {
        let chat = surface(cx);
        let notifications = Rc::new(Cell::new(0));
        let reader_changes = Rc::new(Cell::new(0));
        let _observe = cx.update(|cx| {
            let notifications = notifications.clone();
            cx.observe(&chat, move |_, _| {
                notifications.set(notifications.get() + 1)
            })
        });
        let _subscribe = cx.update(|cx| {
            let reader_changes = reader_changes.clone();
            cx.subscribe(&chat, move |_, event: &Event, _| {
                if matches!(event, Event::ReaderChanged) {
                    reader_changes.set(reader_changes.get() + 1);
                }
            })
        });
        for _ in 0..100 {
            chat.update(cx, |chat, cx| chat.event(stream("latest"), cx));
        }
        cx.run_until_parked();
        assert_eq!(notifications.get(), 0);
        assert_eq!(reader_changes.get(), 1, "only Thinking → Streaming");
        cx.read(|cx| {
            assert_eq!(chat.read(cx).bridge.live, "latest");
            assert!(chat.read(cx).redraw.is_none());
        });
        chat.update(cx, |chat, cx| {
            chat.event(
                BridgeEvent::Commit(serde_json::json!({
                    "conversationId": 1,
                    "entries": [{"kind": "pi.assistant", "model": [{"content": "complete"}]}]
                })),
                cx,
            )
        });
        chat.update(cx, |chat, cx| chat.event(BridgeEvent::Settled, cx));
        cx.read(|cx| {
            assert_eq!(
                chat.read(cx).reader_state.status,
                crate::orb::AgentStatus::Unread
            )
        });
        assert_eq!(notifications.get(), 0);
        chat.update(cx, |chat, cx| chat.set_open(true, cx));
        assert_eq!(notifications.get(), 1);
        cx.read(|cx| assert!(!chat.read(cx).unread));
        cx.read(|cx| assert_eq!(chat.read(cx).bridge.transcript[0].text, "Agent\ncomplete"));
        chat.update(cx, |chat, cx| {
            chat.update_bridge(|bridge| bridge.running = true, cx);
            chat.event(stream("first"), cx);
            chat.event(stream("pending hide"), cx);
            chat.set_open(false, cx);
        });
        let before = notifications.get();
        cx.background_executor
            .advance_clock(Duration::from_millis(33));
        cx.run_until_parked();
        assert_eq!(
            notifications.get(),
            before,
            "hiding cancels a pending redraw"
        );
        chat.update(cx, |chat, cx| chat.set_open(true, cx));
        cx.read(|cx| assert_eq!(chat.read(cx).bridge.live, "pending hide"));
    }

    struct ReaderProbe(Rc<Cell<usize>>);
    impl Render for ReaderProbe {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            self.0.set(self.0.get() + 1);
            div().size_full().child("Reader")
        }
    }

    struct WindowProbe {
        chat: Entity<ChatSurface>,
        reader: Entity<ReaderProbe>,
        reader_width: f32,
    }
    impl Render for WindowProbe {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .flex()
                .size_full()
                .child(
                    div().w(px(self.reader_width)).h_full().child(
                        self.reader
                            .clone()
                            .cached(gpui::StyleRefinement::default().size_full()),
                    ),
                )
                .child(
                    div().w(px(500.)).h_full().child(
                        self.chat
                            .clone()
                            .cached(gpui::StyleRefinement::default().size_full()),
                    ),
                )
        }
    }

    #[gpui::test]
    fn streaming_does_not_notify_the_window_or_rebuild_cached_reader(cx: &mut TestAppContext) {
        let chat = surface(cx);
        chat.update(cx, |chat, cx| {
            chat.set_open(true, cx);
            chat.event(stream("first"), cx);
        });
        let reader_renders = Rc::new(Cell::new(0));
        let window_notifications = Rc::new(Cell::new(0));
        let (root, cx) = cx.add_window_view(|_, cx| {
            cx.subscribe(&chat, |_, _, event: &Event, cx| {
                if matches!(event, Event::ReaderChanged) {
                    cx.notify();
                }
            })
            .detach();
            WindowProbe {
                chat: chat.clone(),
                reader: cx.new(|_| ReaderProbe(reader_renders.clone())),
                reader_width: 500.,
            }
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        let before = reader_renders.get();
        let chat_before = cx.read(|cx| chat.read(cx).renders);
        assert!(before > 0);
        let _observe = cx.update(|_, cx| {
            let notifications = window_notifications.clone();
            cx.observe(&root, move |_, _| {
                notifications.set(notifications.get() + 1)
            })
        });
        for i in 0..100 {
            chat.update(cx, |chat, cx| chat.event(stream(&format!("chunk {i}")), cx));
            // Other animation frames must reuse chat until its timer fires.
            cx.update(|window, cx| window.draw(cx).clear(cx));
        }
        cx.read(|cx| assert_eq!(chat.read(cx).renders, chat_before));
        cx.run_until_parked();
        cx.background_executor
            .advance_clock(Duration::from_millis(33));
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        assert_eq!(window_notifications.get(), 0);
        cx.read(|cx| {
            assert!(
                chat.read(cx).renders > chat_before,
                "the latest stream must be rendered"
            )
        });
        assert_eq!(reader_renders.get(), before);
        // An unrelated shell frame (e.g. an orb status change) also reuses the reader.
        root.update(cx, |_, cx| cx.notify());
        cx.update(|window, cx| window.draw(cx).clear(cx));
        assert_eq!(reader_renders.get(), before);
        eprintln!("100 stream events: window notifications=0, cached reader rebuilds=0");
        root.update(cx, |root, cx| {
            root.reader_width = 450.;
            cx.notify();
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        assert!(
            reader_renders.get() > before,
            "resizing invalidates cached bounds"
        );
        let before_theme = cx.read(|cx| chat.read(cx).renders);
        cx.update(|window, cx| {
            gpui_component::Theme::change(gpui_component::ThemeMode::Dark, Some(window), cx);
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.read(|cx| {
            assert!(
                chat.read(cx).renders > before_theme,
                "theme colors must refresh"
            )
        });
        cx.update(|window, cx| chat.update(cx, |chat, cx| chat.focus(window, cx)));
        cx.simulate_input("draft");
        cx.read(|cx| assert_eq!(chat.read(cx).composer.read(cx).text, "draft"));
    }
}
