//! Agent presence and cursor following. Motion is independent of rendering.
use crate::agent_bridge::{Availability, Bridge};
use gpui::{App, Context, EventEmitter, Render, Window, div, prelude::*, px};
use gpui_component::{ActiveTheme, tooltip::Tooltip};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentStatus {
    Idle,
    Thinking,
    Streaming,
    Unread,
    Blocked,
    Error,
    Off,
}

pub fn agent_status(bridge: &Bridge, surface_open: bool, unread: bool) -> AgentStatus {
    match bridge.availability {
        Availability::Off => AgentStatus::Off,
        Availability::KeyError(_) | Availability::Failed(_) => AgentStatus::Error,
        Availability::NoKey => AgentStatus::Blocked,
        Availability::Starting => AgentStatus::Thinking,
        Availability::Ready if bridge.running => {
            if bridge.live.is_empty() {
                AgentStatus::Thinking
            } else {
                AgentStatus::Streaming
            }
        }
        Availability::Ready if crate::chat::editable_failed_question(bridge).is_some() => {
            AgentStatus::Error
        }
        Availability::Ready if bridge.parked_task.is_some() || !bridge.can_chat() => {
            AgentStatus::Blocked
        }
        Availability::Ready if unread && !surface_open => AgentStatus::Unread,
        Availability::Ready => AgentStatus::Idle,
    }
}

pub fn tooltip_label(bridge: &Bridge, status: AgentStatus) -> &'static str {
    match bridge.availability {
        Availability::NoKey => "Needs an OpenRouter key",
        Availability::KeyError(_) => "Key lookup failed",
        Availability::Failed(_) => "Agent unavailable",
        _ if status == AgentStatus::Blocked && bridge.parked_task.is_some() => "Retry parked task",
        _ if status == AgentStatus::Blocked => crate::chat::status_label(bridge),
        _ => status.label(),
    }
}

impl AgentStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Idle => "Ready",
            Self::Thinking => "Thinking…",
            Self::Streaming => "Writing…",
            Self::Unread => "Answer ready",
            Self::Blocked => "Needs attention",
            Self::Error => "Could not finish",
            Self::Off => "Agent off",
        }
    }
}

#[cfg(any(test, feature = "agent"))]
pub fn unread_after(unread: bool, was_running: bool, running: bool, open: bool) -> bool {
    !open && (unread || (was_running && !running))
}

pub fn follow_target(pointer: [f32; 2], bounds: [f32; 2]) -> [f32; 2] {
    std::array::from_fn(|i| {
        let next = if pointer[i] + 26. + 32. > bounds[i] {
            pointer[i] - 26. - 32.
        } else {
            pointer[i] + 26.
        };
        next.clamp(0., (bounds[i] - 32.).max(0.))
    })
}
/// Region is [left, top, width, height], in window coordinates.
pub fn constrain(position: [f32; 2], region: [f32; 4]) -> [f32; 2] {
    std::array::from_fn(|i| position[i].clamp(region[i], region[i] + (region[i + 2] - 32.).max(0.)))
}
pub fn follow_in_region(pointer: [f32; 2], region: [f32; 4]) -> [f32; 2] {
    let local = follow_target(
        [pointer[0] - region[0], pointer[1] - region[1]],
        [region[2], region[3]],
    );
    [local[0] + region[0], local[1] + region[1]]
}
pub fn dock_in_region(region: [f32; 4]) -> [f32; 2] {
    std::array::from_fn(|i| region[i] + (region[i + 2] - 56.).max(0.))
}

pub fn caught(pointer: [f32; 2], position: [f32; 2]) -> bool {
    (pointer[0] - position[0] - 16.).hypot(pointer[1] - position[1] - 16.) <= 34.
}
pub fn follow_step(
    position: [f32; 2],
    target: [f32; 2],
    dt: f32,
    reduced: bool,
) -> ([f32; 2], bool) {
    let distance = (target[0] - position[0]).hypot(target[1] - position[1]);
    if reduced || distance < 0.5 {
        return (target, false);
    }
    let alpha = 1. - (-dt.max(0.) / 0.15).exp();
    (
        std::array::from_fn(|i| position[i] + (target[i] - position[i]) * alpha),
        true,
    )
}

pub struct Toggle;
pub struct Orb {
    pub status: AgentStatus,
    pub label: String,
    pub surface_open: bool,
    pub surface_bounds: gpui::Bounds<gpui::Pixels>,
    pub reader_bounds: gpui::Bounds<gpui::Pixels>,
    position: Option<[f32; 2]>,
    pointer: Option<[f32; 2]>,
    dragging: bool,
    last_move: Instant,
    last_frame: Instant,
}
impl Orb {
    pub fn new(cx: &mut Context<Self>) -> Self {
        // Only the idle transition needs a timer; settled motion requests no frames.
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(250))
                    .await;
                if this
                    .update(cx, |this, cx| {
                        if this.pointer.is_some()
                            && this.last_move.elapsed() >= Duration::from_secs(4)
                        {
                            this.pointer = None;
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
        Self {
            status: AgentStatus::Off,
            label: "Agent off".into(),
            surface_open: false,
            surface_bounds: Default::default(),
            reader_bounds: Default::default(),
            position: None,
            pointer: None,
            dragging: false,
            last_move: Instant::now(),
            last_frame: Instant::now(),
        }
    }
}
impl EventEmitter<Toggle> for Orb {}

pub fn core(status: AgentStatus, diameter: f32, cx: &App) -> gpui::Div {
    let color = crate::theme::orb_color(status, cx);
    div()
        .relative()
        .size(px(diameter))
        .rounded_full()
        .bg(gpui::linear_gradient(
            135.,
            gpui::linear_color_stop(color, 0.),
            gpui::linear_color_stop(color.opacity(0.6), 1.),
        ))
        .shadow(vec![gpui::BoxShadow {
            color: color.opacity(0.3),
            offset: gpui::point(px(0.), px(2.)),
            blur_radius: px(12.),
            spread_radius: px(2.),
            inset: false,
        }])
        .child(
            div()
                .absolute()
                .top(px(diameter * 0.15))
                .left(px(diameter * 0.2))
                .w(px(diameter * 0.4))
                .h(px(diameter * 0.2))
                .rounded_full()
                .bg(gpui::white().opacity(0.55)),
        )
        .when(status == AgentStatus::Unread, |el| {
            el.child(
                div()
                    .absolute()
                    .right_0()
                    .top_0()
                    .size(px(7.))
                    .rounded_full()
                    .bg(cx.theme().background),
            )
        })
}
fn animated_core(status: AgentStatus, catch: bool, cx: &App) -> gpui::AnyElement {
    use gpui::AnimationExt;
    let el = core(status, if catch { 32. } else { 28. }, cx);
    if cx.reduce_motion() || matches!(status, AgentStatus::Off | AgentStatus::Unread) {
        return el.into_any_element();
    }
    let seconds = match status {
        AgentStatus::Idle => 4.,
        AgentStatus::Thinking => 1.2,
        AgentStatus::Streaming => 2.,
        AgentStatus::Blocked => 5.,
        AgentStatus::Error => 0.35,
        _ => unreachable!(),
    };
    let animation = gpui::Animation::new(Duration::from_secs_f32(seconds));
    let animation = if status == AgentStatus::Error {
        animation
    } else {
        animation.repeat()
    };
    el.with_animation(
        ("orb-presence", status as usize),
        animation,
        move |el, t| {
            if status == AgentStatus::Error {
                el.relative()
                    .left(px((t * std::f32::consts::TAU * 3.).sin() * 3. * (1. - t)))
            } else if status == AgentStatus::Blocked {
                el.opacity(if (0.8..0.84).contains(&t) || (0.89..0.93).contains(&t) {
                    0.5
                } else {
                    1.
                })
            } else {
                let el = el.opacity(0.82 + 0.18 * (t * std::f32::consts::TAU).cos());
                if matches!(status, AgentStatus::Thinking | AgentStatus::Streaming) {
                    let phase = t * std::f32::consts::TAU;
                    el.child(
                        div()
                            .absolute()
                            .size(px(6.))
                            .rounded_full()
                            .bg(gpui::white().opacity(0.4))
                            .left(px(11. + 7. * phase.sin()))
                            .top(px(11. + 7. * phase.cos())),
                    )
                } else {
                    el
                }
            }
        },
    )
    .into_any_element()
}

impl Render for Orb {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let size = window.viewport_size();
        let bounds = [f32::from(size.width), f32::from(size.height)];
        let region = if self.surface_open {
            let reader = self.reader_bounds;
            let left = f32::from(reader.origin.x);
            let top = f32::from(reader.origin.y);
            // Until the newly opened panel paints, reserve its default 45% so
            // even the first frame cannot dock over the composer.
            let chat_left = if self.surface_bounds.origin.x > px(0.) {
                f32::from(self.surface_bounds.origin.x)
            } else {
                bounds[0] * 0.55
            };
            let right = (if reader.size.width > px(0.) {
                f32::from(reader.right()).min(chat_left)
            } else {
                chat_left
            })
            .min((bounds[0] - 420.).max(0.));
            [
                left,
                top,
                (right - left).max(0.),
                if reader.size.height > px(0.) {
                    f32::from(reader.size.height)
                } else {
                    bounds[1]
                },
            ]
        } else {
            [0., 0., bounds[0], bounds[1]]
        };
        let dock = dock_in_region(region);
        let previous = self.position.unwrap_or(dock);
        let position = constrain(previous, region);
        let catch = self
            .pointer
            .is_some_and(|pointer| caught(pointer, position));
        let target = if catch || self.dragging {
            position
        } else {
            self.pointer
                .map(|p| follow_in_region(p, region))
                .unwrap_or(dock)
        };
        let now = Instant::now();
        let (position, moving) = follow_step(
            position,
            target,
            now.duration_since(self.last_frame).as_secs_f32().min(0.05),
            cx.reduce_motion(),
        );
        self.last_frame = now;
        self.position = Some(position);
        if moving {
            window.request_animation_frame();
            let weak = cx.weak_entity();
            window.on_next_frame(move |_, cx| {
                let _ = weak.update(cx, |_, cx| cx.notify());
            });
        }
        let weak = cx.weak_entity();
        let listener = gpui::canvas(
            |_, _, _| (),
            move |_, _, window, _| {
                let exit = weak.clone();
                let down = weak.clone();
                window.on_mouse_event(move |_: &gpui::MouseDownEvent, phase, _, cx| {
                    if phase == gpui::DispatchPhase::Capture {
                        let _ = down.update(cx, |this, _| this.dragging = true);
                    }
                });
                let up = weak.clone();
                window.on_mouse_event(move |_: &gpui::MouseUpEvent, phase, _, cx| {
                    if phase == gpui::DispatchPhase::Capture {
                        let _ = up.update(cx, |this, cx| {
                            this.dragging = false;
                            cx.notify();
                        });
                    }
                });
                window.on_mouse_event(move |_: &gpui::MouseExitEvent, phase, _, cx| {
                    if phase == gpui::DispatchPhase::Capture {
                        let _ = exit.update(cx, |this, cx| {
                            this.pointer = None;
                            this.dragging = false;
                            cx.notify();
                        });
                    }
                });
                let weak = weak.clone();
                window.on_mouse_event(move |event: &gpui::MouseMoveEvent, phase, _, cx| {
                    if phase == gpui::DispatchPhase::Capture {
                        let _ = weak.update(cx, |this, cx| {
                            this.dragging = event.pressed_button.is_some();
                            if !this.dragging {
                                this.pointer =
                                    Some([event.position.x.into(), event.position.y.into()]);
                                this.last_move = Instant::now();
                                cx.notify();
                            }
                        });
                    }
                });
            },
        )
        .absolute()
        .size_full();
        let faded = self.surface_open && self.surface_bounds.contains(&window.mouse_position());
        let label = self.label.clone();
        let status = self.status;
        div().absolute().inset_0().child(listener).child(
            div()
                .id("agent-orb")
                .absolute()
                .left(px(position[0]))
                .top(px(position[1]))
                .size(px(32.))
                .rounded_full()
                .cursor_pointer()
                .opacity(if faded { 0.25 } else { 1. })
                .border_2()
                .border_color(if catch {
                    cx.theme().ring
                } else {
                    gpui::transparent_black()
                })
                .hover(|el| el.border_color(cx.theme().ring))
                .tooltip(move |window, cx| Tooltip::new(label.clone()).build(window, cx))
                .on_click(cx.listener(|_, _, _, cx| cx.emit(Toggle)))
                .child(animated_core(status, catch, cx)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn statuses_and_unread() {
        let mut b = Bridge::off();
        assert_eq!(agent_status(&b, false, false), AgentStatus::Off);
        for (availability, expected) in [
            (Availability::NoKey, AgentStatus::Blocked),
            (Availability::KeyError("x".into()), AgentStatus::Error),
            (Availability::Failed("x".into()), AgentStatus::Error),
            (Availability::Starting, AgentStatus::Thinking),
            (Availability::Ready, AgentStatus::Blocked),
        ] {
            b.availability = availability;
            assert_eq!(agent_status(&b, false, false), expected);
        }
        b.conversation = Some(1);
        assert_eq!(agent_status(&b, false, false), AgentStatus::Idle);
        b.running = true;
        assert_eq!(agent_status(&b, false, false), AgentStatus::Thinking);
        b.live = "Hi".into();
        assert_eq!(agent_status(&b, false, false), AgentStatus::Streaming);
        b.running = false;
        let unread = unread_after(false, true, false, false);
        assert_eq!(agent_status(&b, false, unread), AgentStatus::Unread);
        assert!(!unread_after(unread, false, false, true));
        assert_eq!(agent_status(&b, true, unread), AgentStatus::Idle);
        b.parked_task = Some(1);
        assert_eq!(agent_status(&b, false, false), AgentStatus::Blocked);
        b.parked_task = None;
        b.transcript.push(crate::agent_bridge::TranscriptTurn {
            text: "You\nQuestion".into(),
            quote: None,
        });
        b.notices.push("task_faulted: provider failure".into());
        assert_eq!(agent_status(&b, false, false), AgentStatus::Error);
    }
    #[test]
    fn open_surface_bounds_keep_follow_and_dock_in_reader() {
        let reader = [240., 48., 530., 852.]; // Chat starts at x=770.
        assert_eq!(dock_in_region(reader), [714., 844.]);
        for pointer in [[0., 0.], [600., 500.], [1000., 880.], [1400., 900.]] {
            let target = follow_in_region(pointer, reader);
            assert!(target[0] >= 240. && target[0] + 32. <= 770.);
            assert!(target[1] >= 48. && target[1] + 32. <= 900.);
        }
        assert_eq!(constrain([1344., 844.], reader), [738., 844.]);
        let mut b = Bridge::off();
        b.availability = Availability::KeyError("a very long credential error".into());
        assert_eq!(tooltip_label(&b, AgentStatus::Error), "Key lookup failed");
    }

    #[test]
    fn motion_converges_without_overshoot_and_settles() {
        let mut p = [0., 0.];
        let mut moving = true;
        for _ in 0..200 {
            (p, moving) = follow_step(p, [100., 80.], 1. / 60., false);
            assert!(p[0] <= 100. && p[1] <= 80.);
        }
        assert_eq!(p, [100., 80.]);
        assert!(!moving);
        assert_eq!(
            follow_step([0., 0.], [100., 80.], 0., true),
            ([100., 80.], false)
        );
    }
    #[test]
    fn flips_clamps_and_catches() {
        assert_eq!(follow_target([50., 50.], [200., 200.]), [76., 76.]);
        assert_eq!(follow_target([195., 195.], [200., 200.]), [137., 137.]);
        assert_eq!(follow_target([1., 1.], [20., 20.]), [0., 0.]);
        assert!(caught([16., 16.], [0., 0.]));
        assert!(!caught([60., 60.], [0., 0.]));
    }
}
