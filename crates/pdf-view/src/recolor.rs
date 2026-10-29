//! Cached page palettes and horizontal scrolling. Tinting never calls hayro again.
use std::sync::Arc;

use gpui::{
    AppContext, Context, InteractiveElement, IntoElement, MouseButton, ParentElement, Render,
    RenderImage, StatefulInteractiveElement, Styled, Window, div, point, px,
};
use image::{Frame, RgbaImage};

use crate::{PdfStyle, PdfView, SCROLLBAR_W};

/// Palette applied to page pixels, independently of the viewer chrome.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PageColors {
    #[default]
    Normal,
    Dark,
}

/// Invert HSL lightness, not RGB: channel differences (and thus hue) survive.
/// Compress the output to 28..232 to avoid glaring white text and pure black paper.
/// This is deliberately uniform: photographs retain hue but their tones invert.
/// Input/output are opaque BGRA, after compositing the PDF onto white.
fn darken_bgra(pixels: &mut [u8]) {
    for p in pixels.chunks_exact_mut(4) {
        let low = *p[..3].iter().min().unwrap() as i32;
        let high = *p[..3].iter().max().unwrap() as i32;
        let shift = 255 - low - high;
        for channel in &mut p[..3] {
            let inverted = *channel as i32 + shift;
            *channel = (28 + (inverted * 204 + 127) / 255) as u8;
        }
    }
}

/// Both palettes share one rasterization and are evicted together. Precomputing
/// the tint off-thread makes even repeated theme switches immediate and bounded.
#[derive(Clone)]
pub(crate) struct PageRaster {
    normal: Arc<RenderImage>,
    dark: Arc<RenderImage>,
}

impl PageRaster {
    pub(crate) fn new(normal: Arc<RenderImage>) -> Self {
        let size = normal.size(0);
        let mut pixels = normal.as_bytes(0).expect("raster has one frame").to_vec();
        darken_bgra(&mut pixels);
        let buffer = RgbaImage::from_raw(size.width.0, size.height.0, pixels)
            .expect("raster dimensions match pixels");
        Self {
            normal,
            dark: Arc::new(RenderImage::new(vec![Frame::new(buffer)])),
        }
    }

    pub(crate) fn image(&self, colors: PageColors) -> Arc<RenderImage> {
        match colors {
            PageColors::Normal => self.normal.clone(),
            PageColors::Dark => self.dark.clone(),
        }
    }

    pub(crate) fn images(self) -> [Arc<RenderImage>; 2] {
        [self.normal, self.dark]
    }
}

/// Mirror the direct-child page layout without centering wide pages at negative x.
pub(crate) fn page_inset(viewport: f32, page: f32) -> f32 {
    ((viewport - page) / 2.0).max(0.0)
}

/// Keep the same point under the viewport centre when an axis changes scale.
/// Origins account for fixed padding/centering, which must not scale with pages.
fn zoom_axis(
    scroll: f32,
    viewport: f32,
    old_origin: f32,
    new_origin: f32,
    ratio: f32,
    max: f32,
) -> f32 {
    ((scroll + viewport / 2.0 - old_origin) * ratio + new_origin - viewport / 2.0)
        .clamp(0.0, max.max(0.0))
}

#[derive(Debug)]
struct HorizontalMetrics {
    thumb: f32,
    left: f32,
    travel: f32,
    max_scroll: f32,
}

fn horizontal_metrics(viewport: f32, content: f32, offset: f32) -> Option<HorizontalMetrics> {
    if viewport <= 1.0 || content <= viewport + 1.0 {
        return None;
    }
    let max_scroll = content - viewport;
    let thumb = (viewport * viewport / content).max(32.0).min(viewport);
    let travel = (viewport - thumb).max(0.0);
    Some(HorizontalMetrics {
        thumb,
        left: offset.clamp(0.0, max_scroll) / max_scroll * travel,
        travel,
        max_scroll,
    })
}

#[derive(Clone)]
struct HorizontalDrag;
impl Render for HorizontalDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        gpui::Empty
    }
}

impl PdfView {
    pub(crate) fn zoom_offset(&self, zoom: f32) -> gpui::Point<gpui::Pixels> {
        let viewport = self.scroll.bounds().size;
        let (vw, vh) = (f32::from(viewport.width), f32::from(viewport.height));
        if self.dims.is_empty() || vw <= 1.0 || vh <= 1.0 {
            return self.scroll.offset();
        }
        let old_width = self.page_width();
        let new_width = crate::PAGE_WIDTH * zoom;
        let ratio = new_width / old_width;
        let sx = f32::from(-self.scroll.offset().x);
        let sy = f32::from(-self.scroll.offset().y);
        let anchor = crate::current_page(&self.dims, old_width, sy + vh / 2.0);
        let old_top = crate::page_top_y(&self.dims, old_width, anchor);
        let new_top = crate::page_top_y(&self.dims, new_width, anchor);
        let content_h = 2.0 * crate::PAGE_PAD_Y
            + self
                .dims
                .iter()
                .map(|d| crate::display_height(*d, new_width))
                .sum::<f32>()
            + self.dims.len().saturating_sub(1) as f32 * crate::PAGE_GAP;
        point(
            px(-zoom_axis(
                sx,
                vw,
                page_inset(vw, old_width),
                page_inset(vw, new_width),
                ratio,
                new_width - vw,
            )),
            px(-zoom_axis(sy, vh, old_top, new_top, ratio, content_h - vh)),
        )
    }

    pub(crate) fn horizontal_scrollbar(
        &self,
        style: PdfStyle,
        cx: &mut Context<Self>,
    ) -> Option<gpui::Div> {
        let width = f32::from(self.scroll.bounds().size.width);
        let metrics =
            horizontal_metrics(width, self.page_width(), f32::from(-self.scroll.offset().x))?;
        Some(
            div()
                .absolute()
                .bottom_0()
                .left_0()
                .w(px(width))
                .h(px(SCROLLBAR_W))
                .bg(style.bg)
                .child(
                    div()
                        .id("pdf-horizontal-thumb")
                        .absolute()
                        .left(px(metrics.left))
                        .bottom(px(2.0))
                        .w(px(metrics.thumb))
                        .h(px(SCROLLBAR_W - 4.0))
                        .rounded(px(4.0))
                        .bg(gpui::Hsla {
                            a: 0.6,
                            ..style.header_muted
                        })
                        .cursor_pointer()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, ev: &gpui::MouseDownEvent, window, cx| {
                                let bounds = this.scroll.bounds();
                                if let Some(m) = horizontal_metrics(
                                    f32::from(bounds.size.width),
                                    this.page_width(),
                                    f32::from(-this.scroll.offset().x),
                                ) {
                                    this.horizontal_drag =
                                        Some(f32::from(ev.position.x - bounds.origin.x) - m.left);
                                    window.focus(&this.focus, cx);
                                }
                                cx.stop_propagation();
                            }),
                        )
                        .on_drag(HorizontalDrag, |_, _, _, cx| {
                            cx.stop_propagation();
                            cx.new(|_| HorizontalDrag)
                        })
                        .on_drag_move(cx.listener(
                            |this, ev: &gpui::DragMoveEvent<HorizontalDrag>, _, cx| {
                                let Some(grab) = this.horizontal_drag else {
                                    return;
                                };
                                let bounds = this.scroll.bounds();
                                let Some(m) = horizontal_metrics(
                                    f32::from(bounds.size.width),
                                    this.page_width(),
                                    f32::from(-this.scroll.offset().x),
                                ) else {
                                    return;
                                };
                                if m.travel > 0.0 {
                                    let left = (f32::from(ev.event.position.x - bounds.origin.x)
                                        - grab)
                                        .clamp(0.0, m.travel);
                                    this.scroll.set_offset(point(
                                        px(-left / m.travel * m.max_scroll),
                                        this.scroll.offset().y,
                                    ));
                                    cx.notify();
                                }
                            },
                        )),
                ),
        )
    }

    pub(crate) fn shift_scroll_listener(&self, cx: &mut Context<Self>) -> gpui::Canvas<()> {
        let viewer = cx.entity().downgrade();
        // A paint-time listener survives GPUI's cached view renders. The canvas
        // has no hitbox, so it never interferes with text selection or links.
        gpui::canvas(
            |_, _, _| (),
            move |_, _, window, _| {
                let viewer = viewer.clone();
                // Capture before the built-in bubble listener has already moved y.
                window.on_mouse_event(move |event: &gpui::ScrollWheelEvent, phase, window, cx| {
                    if phase == gpui::DispatchPhase::Capture && event.modifiers.shift {
                        let _ = viewer.update(cx, |this, cx| {
                            if this.scroll.bounds().contains(&event.position) {
                                this.scroll_pages(event, window, cx);
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

    fn scroll_pages(
        &mut self,
        event: &gpui::ScrollWheelEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Platforms that already swap Shift-wheel report x; otherwise convert y.
        // Stop propagation only for this manual path; native two-axis scrolling handles
        // trackpads and ordinary vertical wheel events on the same ScrollHandle.
        if event.modifiers.shift {
            let delta = event.delta.pixel_delta(window.line_height());
            let dx = if delta.x != px(0.0) { delta.x } else { delta.y };
            let width = f32::from(self.scroll.bounds().size.width);
            let max = (self.page_width() - width).max(0.0);
            let x = (f32::from(self.scroll.offset().x + dx)).clamp(-max, 0.0);
            self.scroll.set_offset(point(px(x), self.scroll.offset().y));
            cx.stop_propagation();
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn palette_switch_reuses_rasters_and_leaves_normal_pixels_untouched() {
        let normal = Arc::new(RenderImage::new(vec![Frame::new(
            RgbaImage::from_raw(1, 1, vec![255, 255, 255, 255]).unwrap(),
        )]));
        let raster = PageRaster::new(normal.clone());
        assert!(Arc::ptr_eq(&normal, &raster.image(PageColors::Normal)));
        let dark = raster.image(PageColors::Dark);
        assert_eq!(normal.as_bytes(0).unwrap(), &[255, 255, 255, 255]);
        assert_eq!(dark.as_bytes(0).unwrap(), &[28, 28, 28, 255]);
        assert!(Arc::ptr_eq(&dark, &raster.image(PageColors::Dark)));
        assert_eq!(dark.size(0), normal.size(0));
    }

    #[test]
    fn paper_and_ink_swap_without_changing_alpha() {
        let mut pixels = [255, 255, 255, 255, 0, 0, 0, 255, 128, 128, 128, 255];
        darken_bgra(&mut pixels);
        assert_eq!(
            pixels,
            [28, 28, 28, 255, 232, 232, 232, 255, 130, 130, 130, 255]
        );
    }
    #[test]
    fn saturated_pixels_keep_channel_order_and_alpha() {
        let mut pixels = [20, 80, 200, 255, 255, 0, 0, 255];
        darken_bgra(&mut pixels);
        assert!(pixels[0] < pixels[1] && pixels[1] < pixels[2]);
        assert_eq!(&pixels[4..], &[232, 28, 28, 255]);
        assert_eq!(pixels[3], 255);
    }
    #[test]
    fn zoom_keeps_centre_stable_and_clamps_edges() {
        // Centred narrow page becomes wide: the page midpoint stays centred.
        assert_eq!(zoom_axis(0.0, 1000.0, 90.0, 0.0, 3.0, 1460.0), 730.0);
        // A point 300px into a page scales, while fixed page origin does not.
        assert_eq!(zoom_axis(100.0, 400.0, 16.0, 16.0, 2.0, 2000.0), 384.0);
        assert_eq!(zoom_axis(0.0, 900.0, 0.0, 0.0, 0.5, -100.0), 0.0);
        assert_eq!(zoom_axis(2000.0, 600.0, 0.0, 0.0, 3.0, 1860.0), 1860.0);
    }

    #[test]
    fn horizontal_geometry_reaches_both_edges() {
        assert_eq!(page_inset(1000.0, 820.0), 90.0);
        assert_eq!(page_inset(600.0, 2460.0), 0.0);
        assert!(horizontal_metrics(0.0, 2460.0, 0.0).is_none());
        assert!(horizontal_metrics(900.0, 820.0, 0.0).is_none());
        let start = horizontal_metrics(600.0, 2460.0, -10.0).unwrap();
        assert_eq!(start.left, 0.0);
        let end = horizontal_metrics(600.0, 2460.0, 9999.0).unwrap();
        assert!((end.left + end.thumb - 600.0).abs() < 0.001);
        assert_eq!(end.max_scroll, 1860.0);
    }
}
