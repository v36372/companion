use std::path::PathBuf;

use gpui::{App, AppContext, Bounds, WindowBounds, WindowOptions, px, size};
use gpui_component::{Root, Theme, ThemeMode};

use crate::window::CompanionWindow;

// Toolbar icons outside gpui-kit-assets' default component bundle.
gpui_kit_assets::icon_assets!(ExtraIcons, [Highlighter, MousePointer, MoveHorizontal]);

/// Component icons plus the few extra Lucide icons the viewer toolbar uses.
struct AppAssets;

impl gpui::AssetSource for AppAssets {
    fn load(&self, path: &str) -> gpui::Result<Option<std::borrow::Cow<'static, [u8]>>> {
        if let Some(bytes) = ExtraIcons.load(path)? {
            return Ok(Some(bytes));
        }
        gpui_kit_assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> gpui::Result<Vec<gpui::SharedString>> {
        let mut paths = gpui_kit_assets::Assets.list(path)?;
        paths.extend(ExtraIcons.list(path)?);
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}

pub fn run(path: Option<PathBuf>) {
    gpui_platform::application()
        .with_assets(AppAssets)
        .run(move |cx: &mut App| {
            gpui_component::init(cx);
            if std::env::var("COMPANION_REDUCE_MOTION").as_deref() == Ok("1") {
                cx.set_reduce_motion(true);
            } else {
                let preference = cx.background_executor().spawn(async {
                    if cfg!(target_os = "macos") {
                        std::process::Command::new("defaults")
                            .args(["read", "com.apple.universalaccess", "reduceMotion"])
                            .output()
                            .ok()
                            .is_some_and(|out| {
                                out.status.success()
                                    && String::from_utf8_lossy(&out.stdout).trim() == "1"
                            })
                    } else {
                        std::process::Command::new("gsettings")
                            .args(["get", "org.gnome.desktop.interface", "enable-animations"])
                            .output()
                            .ok()
                            .is_some_and(|out| {
                                out.status.success()
                                    && String::from_utf8_lossy(&out.stdout).trim() == "false"
                            })
                    }
                });
                cx.spawn(async move |cx| {
                    if preference.await {
                        cx.update(|cx| cx.set_reduce_motion(true));
                    }
                })
                .detach();
            }
            // Component Theme is the shared design-token source for subsequent UI
            // migrations: ActiveTheme::theme().semantic_tokens() supplies spacing,
            // typography and colours. Keep existing hand-rolled styles unchanged.
            cx.on_window_closed(|cx, _| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();

            let bounds = Bounds::centered(None, size(px(1400.), px(900.)), cx);
            let mut companion = None;
            let window = cx
                .open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(bounds)),
                        ..Default::default()
                    },
                    |window, cx| {
                        let theme_override =
                            theme_override(std::env::var("COMPANION_THEME").ok().as_deref());
                        if let Some(mode) = theme_override {
                            Theme::change(mode, Some(window), cx);
                        } else {
                            Theme::sync_system_appearance(Some(window), cx);
                            window
                                .observe_window_appearance(|window, cx| {
                                    Theme::sync_system_appearance(Some(window), cx);
                                })
                                .detach();
                        }
                        let view = cx.new(|cx| CompanionWindow::new(path, window, cx));
                        companion = Some(view.clone());
                        // Root 0.8.1 always includes its client-side border; there is
                        // no bordered(false) opt-out. Its font/rem match GPUI's own.
                        cx.new(|cx| Root::new(view, window, cx))
                    },
                )
                .expect("open Learning Companion window");

            // App quit can bypass the native window-close request. Release while
            // the window and its texture atlas are still available in either case.
            // Retain the completion receiver independently of the native window:
            // window-close may destroy its entity before on_app_quit runs.
            let companion = companion.expect("create Learning Companion view");
            let agent_shutdown = companion.read(cx).agent_shutdown();
            cx.on_app_quit(move |cx| {
                let _ = window.update(cx, |_, window, cx| {
                    companion.update(cx, |view, cx| view.close_pdf(window, cx));
                });
                let shutdown = agent_shutdown.clone();
                async move {
                    if let Some(shutdown) = shutdown {
                        let _ = shutdown.recv().await;
                    }
                }
            })
            .detach();
            cx.activate(true);
            tracing::info!(target: "companion", "Learning Companion window opened");
        });
}

fn theme_override(value: Option<&str>) -> Option<ThemeMode> {
    match value {
        Some("dark") => Some(ThemeMode::Dark),
        Some("light") => Some(ThemeMode::Light),
        None | Some("system") => None,
        Some(other) => {
            tracing::warn!(target: "companion", value = other, "invalid COMPANION_THEME; using system appearance");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_environment_override() {
        assert_eq!(theme_override(Some("dark")), Some(ThemeMode::Dark));
        assert_eq!(theme_override(Some("light")), Some(ThemeMode::Light));
        assert_eq!(theme_override(Some("system")), None);
        assert_eq!(theme_override(None), None);
        assert_eq!(theme_override(Some("invalid")), None);
    }
}
