use std::path::PathBuf;

use gpui::{App, AppContext, Bounds, WindowBounds, WindowOptions, px, size};

use crate::window::CompanionWindow;

pub fn run(path: Option<PathBuf>) {
    gpui_platform::application().run(move |cx: &mut App| {
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();

        let bounds = Bounds::centered(None, size(px(1400.), px(900.)), cx);
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                |window, cx| cx.new(|cx| CompanionWindow::new(path, window, cx)),
            )
            .expect("open Learning Companion window");

        // App quit can bypass the native window-close request. Release while
        // the window and its texture atlas are still available in either case.
        cx.on_app_quit(move |cx| {
            let _ = window.update(cx, |view, window, cx| view.release_pdf(window, cx));
            async {}
        })
        .detach();
        cx.activate(true);
        tracing::info!(target: "companion", "Learning Companion window opened");
    });
}
