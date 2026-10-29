//! Chat presentation. Durable state and scheduling remain in the bridge.
use crate::{
    agent_bridge::{Availability, Bridge},
    ask_bar::AskBar,
};
use gpui::{Entity, IntoElement, div, prelude::*, px};
use gpui_component::{
    ActiveTheme, Disableable, Sizable,
    button::{Button, ButtonVariants},
    text::TextView,
};

/// Follow a growing transcript only when the reader was already at its tail.
/// Call before the next layout (the handle still describes the old message sizes).
pub fn follow_tail(scroll: &gpui::ScrollHandle) {
    if is_near_tail(
        f32::from(scroll.offset().y),
        f32::from(scroll.max_offset().y),
    ) {
        scroll.scroll_to_bottom();
    }
}
fn is_near_tail(offset: f32, maximum: f32) -> bool {
    maximum + offset <= 32.0
}

fn is_user(speaker: &str) -> bool {
    matches!(speaker, "You" | "user")
}

pub(crate) fn editable_failed_question(bridge: &Bridge) -> Option<String> {
    if bridge.running || bridge.parked_task.is_some() {
        return None;
    }
    let failed = matches!(bridge.availability, Availability::Failed(_))
        || bridge.notices.iter().any(|notice| {
            notice.starts_with("session_failed:") || notice.starts_with("task_faulted:")
        })
        || bridge
            .notices
            .last()
            .is_some_and(|notice| notice == &bridge.status);
    if !failed {
        return None;
    }
    // Notices are durable across turns. A later completed assistant response
    // means an old failure must not mark the current turn as failed forever.
    if bridge
        .transcript
        .last()
        .and_then(|turn| turn.text.split_once('\n'))
        .is_some_and(|(speaker, _)| !is_user(speaker))
    {
        return None;
    }
    bridge.transcript.iter().rev().find_map(|turn| {
        let (speaker, question) = turn.text.split_once('\n')?;
        (is_user(speaker) && !question.trim().is_empty()).then(|| question.to_owned())
    })
}

pub(crate) fn status_label(bridge: &Bridge) -> &'static str {
    if bridge.status.starts_with("Not sent:") {
        return "Not sent — check agent status";
    }
    match &bridge.availability {
        Availability::NoKey => "OpenRouter key needed",
        Availability::KeyError(_) => "Key lookup failed",
        Availability::Starting => "Agent starting…",
        Availability::Failed(_) => "Agent unavailable",
        Availability::Off => "Agent disabled",
        Availability::Ready if bridge.status.starts_with("Resuming") => "Resuming…",
        Availability::Ready if bridge.status == "Stopping…" => "Stopping…",
        Availability::Ready if bridge.running => "Thinking…",
        Availability::Ready if bridge.status.starts_with("Open a PDF") => "Open a PDF to chat",
        Availability::Ready if !bridge.can_chat() => "Preparing document…",
        Availability::Ready => "Ready",
    }
}

pub fn pane(
    bridge: &Bridge,
    composer: Entity<AskBar>,
    viewer: Entity<crate::diagram::DiagramViewer>,
    scroll: &gpui::ScrollHandle,
    navigate: impl Fn(u32, &mut gpui::Window, &mut gpui::App) + Clone + 'static,
    retry: impl Fn(&mut gpui::Window, &mut gpui::App) + 'static,
    cx: &mut gpui::App,
) -> impl IntoElement {
    follow_tail(scroll);
    let unavailable = bridge.unavailable_reason();
    div()
        .w_full()
        .min_w_0()
        .h_full()
        .flex()
        .flex_col()
        .p_3()
        .gap_3()
        .child(
            div()
                .id("chat-transcript")
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .gap_3()
                .overflow_y_scroll()
                .track_scroll(scroll)
                .when(
                    bridge.transcript.is_empty() && bridge.live.is_empty(),
                    |pane| {
                        pane.child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_3()
                                .py_4()
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(cx.theme().muted_foreground)
                                        .child("Ask a question, or select a passage."),
                                )
                                .children(
                                    [
                                        "Summarize this document",
                                        "Explain the key ideas",
                                        "Suggest questions to explore",
                                    ]
                                    .into_iter()
                                    .enumerate()
                                    .map(|(index, prompt)| {
                                        let composer = composer.clone();
                                        Button::new(("chat-suggestion", index))
                                            .outline()
                                            .small()
                                            .label(prompt)
                                            .tooltip(unavailable.clone().unwrap_or_else(|| {
                                                "Edit this question before sending".into()
                                            }))
                                            .on_click(move |_, window, cx| {
                                                composer.update(cx, |bar, cx| {
                                                    bar.text = prompt.into();
                                                    bar.focus(window, cx);
                                                    cx.notify();
                                                })
                                            })
                                    }),
                                ),
                        )
                    },
                )
                .children(bridge.transcript.iter().enumerate().map(|(index, turn)| {
                    let navigate = navigate.clone();
                    let (speaker, content) = turn
                        .text
                        .split_once('\n')
                        .unwrap_or(("assistant", &turn.text));
                    let user = is_user(speaker);
                    div()
                        .w_full()
                        .flex()
                        .when(user, |el| el.justify_end())
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_2()
                                .min_w_0()
                                .when(user, |el| {
                                    el.max_w(px(560.))
                                        .p_3()
                                        .rounded_lg()
                                        .bg(cx.theme().secondary)
                                })
                                .when(!user, |el| el.w_full())
                                .children(turn.quote.as_ref().map(|quote| {
                                    let page_0 = quote.page.saturating_sub(1);
                                    Button::new(("quote", index))
                                        .max_w_full()
                                        .outline()
                                        .small()
                                        .label(quote.label())
                                        .tooltip("Go to quoted page")
                                        .on_click(move |_, window, cx| navigate(page_0, window, cx))
                                }))
                                .child(if user {
                                    div().child(content.to_owned()).into_any_element()
                                } else {
                                    crate::diagram::markdown(
                                        ("assistant-message", index),
                                        content.to_owned(),
                                        viewer.clone(),
                                    )
                                    .selectable(true)
                                    .into_any_element()
                                }),
                        )
                }))
                .children(bridge.notices.last().map(|text| {
                    div()
                        .text_xs()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .text_color(cx.theme().muted_foreground)
                        .child(text.clone())
                }))
                .when(!bridge.live.is_empty(), |pane| {
                    pane.child(
                        div().w_full().child(
                            TextView::markdown(
                                "live-message",
                                crate::diagram::streaming_text(&bridge.live),
                            )
                            .selectable(true),
                        ),
                    )
                }),
        )
        .children(bridge.parked_task.map(|_| {
            Button::new("retry-parked-task")
                .outline()
                .label("Retry parked task")
                .disabled(!bridge.can_retry())
                .tooltip(unavailable.clone().unwrap_or_else(|| {
                    if bridge.running {
                        "Wait for the current turn to finish".into()
                    } else {
                        "Retry without duplicating the question".into()
                    }
                }))
                .on_click(move |_, window, cx| retry(window, cx))
        }))
        .children(editable_failed_question(bridge).map(|question| {
            let composer = composer.clone();
            Button::new("edit-failed-question")
                .ghost()
                .small()
                .label("Edit last question")
                .tooltip("Replace the draft with your last question; this does not send it")
                .on_click(move |_, window, cx| {
                    composer.update(cx, |bar, cx| {
                        bar.text = question.clone();
                        bar.focus(window, cx);
                        cx.notify();
                    })
                })
        }))
        .child(composer)
        .when(bridge.queued() > 0, |pane| {
            pane.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!("{} follow-up(s) queued", bridge.queued())),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::{editable_failed_question, is_near_tail, is_user, status_label};
    use crate::agent_bridge::{Availability, Bridge, TranscriptTurn};

    #[test]
    fn failed_turn_restores_draft_without_conflicting_with_parked_retry() {
        let mut bridge = Bridge::off();
        bridge.transcript.push(TranscriptTurn {
            text: "You\nExplain this passage".into(),
            quote: None,
        });
        assert!(editable_failed_question(&bridge).is_none());
        bridge.notices.push("task_faulted: provider error".into());
        assert_eq!(
            editable_failed_question(&bridge).as_deref(),
            Some("Explain this passage")
        );
        bridge.running = true;
        assert!(editable_failed_question(&bridge).is_none());
        bridge.running = false;
        bridge.parked_task = Some(42);
        assert!(editable_failed_question(&bridge).is_none());
    }

    #[test]
    fn successful_answer_clears_historical_failure_presentation() {
        let mut bridge = Bridge::off();
        bridge.notices.push("task_faulted: old error".into());
        bridge.transcript.push(TranscriptTurn {
            text: "You\nQuestion".into(),
            quote: None,
        });
        assert!(editable_failed_question(&bridge).is_some());
        bridge.transcript.push(TranscriptTurn {
            text: "Agent\nAnswer".into(),
            quote: None,
        });
        assert!(editable_failed_question(&bridge).is_none());
    }

    #[test]
    fn durable_display_roles_have_correct_alignment() {
        assert!(is_user("You"));
        assert!(is_user("user"));
        assert!(!is_user("Agent"));
        assert!(!is_user("assistant"));
    }

    #[test]
    fn availability_and_recovery_have_compact_visible_labels() {
        let mut bridge = Bridge::off();
        bridge.availability = Availability::NoKey;
        assert_eq!(status_label(&bridge), "OpenRouter key needed");
        bridge.availability = Availability::KeyError("specific store failure".into());
        assert_eq!(status_label(&bridge), "Key lookup failed");
        assert!(
            bridge
                .unavailable_reason()
                .unwrap()
                .contains("specific store failure")
        );
        bridge.availability = Availability::Starting;
        assert_eq!(status_label(&bridge), "Agent starting…");
        bridge.availability = Availability::Ready;
        bridge.running = true;
        bridge.status = "Resuming… · Esc to stop".into();
        assert_eq!(status_label(&bridge), "Resuming…");
    }
    #[test]
    fn tail_following_preserves_reading_position() {
        assert!(is_near_tail(0., 0.));
        assert!(is_near_tail(-500., 500.));
        assert!(is_near_tail(-480., 500.));
        assert!(!is_near_tail(-200., 500.));
    }
}
