//! Application-specific presence colours; chrome uses component theme tokens.
use crate::orb::AgentStatus;
use gpui::{App, Hsla, rgb};
use gpui_component::ActiveTheme;
pub fn orb_color(status: AgentStatus, cx: &App) -> Hsla {
    match status {
        AgentStatus::Idle => rgb(if cx.theme().is_dark() {
            0x829bff
        } else {
            0x4b65d9
        })
        .into(),
        AgentStatus::Thinking => rgb(0x9564e8).into(),
        AgentStatus::Streaming => rgb(if cx.theme().is_dark() {
            0x43d9cd
        } else {
            0x159baf
        })
        .into(),
        AgentStatus::Unread => cx.theme().success,
        AgentStatus::Blocked => cx.theme().warning,
        AgentStatus::Error => cx.theme().danger,
        AgentStatus::Off => cx.theme().muted_foreground,
    }
}
