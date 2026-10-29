//! Cursor-anchored zoom geometry and gesture handling.
use gpui::{Context, Pixels, Point, ScrollDelta, Size, Styled, point, px};

use crate::{
    MIN_ZOOM, PAGE_GAP, PAGE_PAD_Y, PAGE_WIDTH, PdfView, current_page, display_height, page_top_y,
};

// Cap fit width at 1640px so very wide windows do not produce oversized text.
fn fit_width(viewport: f32) -> f32 {
    ((viewport - crate::SCROLLBAR_W - 6.0) / PAGE_WIDTH).clamp(MIN_ZOOM, 2.0)
}

fn anchored_offset(
    dims: &[(f32, f32)],
    old_width: f32,
    new_width: f32,
    offset: Point<Pixels>,
    viewport: Size<Pixels>,
    anchor: Point<Pixels>,
) -> Point<Pixels> {
    let (vw, vh) = (f32::from(viewport.width), f32::from(viewport.height));
    if dims.is_empty() || vw <= 1.0 || vh <= 1.0 {
        return offset;
    }
    let (x, y) = (f32::from(anchor.x), f32::from(anchor.y));
    let (sx, sy) = (f32::from(-offset.x), f32::from(-offset.y));
    let page = current_page(dims, old_width, sy + y);
    let ratio = new_width / old_width;
    let height = 2.0 * PAGE_PAD_Y
        + dims
            .iter()
            .map(|d| display_height(*d, new_width))
            .sum::<f32>()
        + dims.len().saturating_sub(1) as f32 * PAGE_GAP;
    let nx = ((sx + x - crate::recolor::page_inset(vw, old_width)) * ratio
        + crate::recolor::page_inset(vw, new_width)
        - x)
        .clamp(0.0, (new_width - vw).max(0.0));
    let ny = ((sy + y - page_top_y(dims, old_width, page)) * ratio
        + page_top_y(dims, new_width, page)
        - y)
        .clamp(0.0, (height - vh).max(0.0));
    point(px(-nx), px(-ny))
}

fn wheel_factor(delta: ScrollDelta, line_height: Pixels) -> f32 {
    // GPUI scroll offsets are positive for wheel-up (opposite screen y).
    let delta = delta.pixel_delta(line_height);
    let dy = if delta.y == px(0.0) { delta.x } else { delta.y };
    (f32::from(dy) * 0.0025).exp()
}

impl PdfView {
    pub(crate) fn fit_width_zoom(viewport: f32) -> f32 {
        fit_width(viewport)
    }

    fn gesture_zoom(&mut self, factor: f32, position: Point<Pixels>, cx: &mut Context<Self>) {
        let z = (self.zoom * factor).clamp(crate::MIN_ZOOM, crate::MAX_ZOOM);
        let was_fit = self.fit.take().is_some();
        if (z - self.zoom).abs() < 0.001 {
            if was_fit {
                cx.notify();
            }
            return;
        }
        let bounds = self.scroll.bounds();
        let offset = anchored_offset(
            &self.dims,
            self.page_width(),
            PAGE_WIDTH * z,
            self.scroll.offset(),
            bounds.size,
            position - bounds.origin,
        );
        self.zoom = z;
        self.zoom_active = true;
        self.scroll.set_offset(offset);
        self.zoom_gesture = self.zoom_gesture.wrapping_add(1);
        let token = self.zoom_gesture;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(150))
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.zoom_gesture == token && !this.pinch_active {
                    this.zoom_active = false;
                    this.generation = this.generation.wrapping_add(1);
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    fn finish_pinch(&mut self, cx: &mut Context<Self>) {
        self.zoom_active = false;
        self.zoom_gesture = self.zoom_gesture.wrapping_add(1);
        self.generation = self.generation.wrapping_add(1);
        cx.notify();
    }

    pub(crate) fn zoom_listener(&self, cx: &mut Context<Self>) -> gpui::Canvas<()> {
        let viewer = cx.entity().downgrade();
        gpui::canvas(
            |_, _, _| (),
            move |_, _, window, _| {
                let wheel_viewer = viewer.clone();
                window.on_mouse_event(move |event: &gpui::ScrollEvent, phase, window, cx| {
                    if phase == gpui::DispatchPhase::Capture
                        && (event.modifiers.secondary(cx) || event.modifiers.control)
                    {
                        let _ = wheel_viewer.update(cx, |this, cx| {
                            if this.scroll.bounds().contains(&event.position) {
                                this.gesture_zoom(
                                    wheel_factor(event.delta, window.line_height()),
                                    event.position,
                                    cx,
                                );
                                cx.stop_propagation();
                            }
                        });
                    }
                });
                let pinch_viewer = viewer.clone();
                window.on_mouse_event(move |event: &gpui::PinchEvent, phase, _, cx| {
                    if phase == gpui::DispatchPhase::Capture {
                        let _ = pinch_viewer.update(cx, |this, cx| {
                            if this.pinch_active || this.scroll.bounds().contains(&event.position) {
                                this.pinch_active = !matches!(
                                    event.phase,
                                    gpui::TouchPhase::Ended | gpui::TouchPhase::Cancelled
                                );
                                this.gesture_zoom(event.delta.exp(), event.position, cx);
                                if !this.pinch_active {
                                    this.finish_pinch(cx);
                                }
                                cx.stop_propagation();
                            }
                        });
                    }
                });
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn anchored_clamps_and_accounts_for_centering() {
        let dims = [(1.0, 2.0)];
        let vp = gpui::size(px(600.0), px(600.0));
        let anchor = point(px(300.0), px(300.0));
        // The centered narrow page expands around the cursor.
        let centered = anchored_offset(&dims, 400.0, 800.0, point(px(0.0), px(-100.0)), vp, anchor);
        assert_eq!(centered.x, px(-100.0));
        // Zooming out at each extreme clamps to the new scroll range.
        let top_left = anchored_offset(
            &dims,
            800.0,
            1200.0,
            point(px(0.0), px(0.0)),
            vp,
            point(px(0.0), px(0.0)),
        );
        assert_eq!(top_left, point(px(0.0), px(0.0)));
        let bottom_right = anchored_offset(
            &dims,
            1200.0,
            800.0,
            point(px(-800.0), px(-2000.0)),
            vp,
            point(px(600.0), px(600.0)),
        );
        assert_eq!(bottom_right.x, px(-200.0));
        assert_eq!(bottom_right.y, px(-1032.0));
    }

    #[test]
    fn width_only_resize_changes_fit() {
        assert!(fit_width(1000.0) > fit_width(800.0));
        assert!(fit_width(1000.0) < fit_width(1400.0));
    }

    #[test]
    fn geometry_and_gestures() {
        let dims = [(1.0, 2.0), (1.0, 2.0)];
        let viewport = gpui::size(px(600.0), px(600.0));
        let anchor = point(px(300.0), px(300.0));
        let offset = anchored_offset(
            &dims,
            820.0,
            1640.0,
            point(px(-100.0), px(-200.0)),
            viewport,
            anchor,
        );
        assert_eq!(offset.x, px(-500.0));
        assert_eq!(offset.y, px(-684.0));
        assert_eq!(fit_width(300.0), MIN_ZOOM);
        assert_eq!(fit_width(5000.0), 2.0);
        assert!(
            (wheel_factor(ScrollDelta::Lines(point(0.0, 1.0)), px(40.0)) - 0.1f32.exp()).abs()
                < 0.0001
        );
        assert!((0.1f32.exp() * 0.2f32.exp() - 0.3f32.exp()).abs() < 0.0001);
    }
}
