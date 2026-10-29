//! Observe the laid-out scroll handle outside render/layout. A weak task stops
//! automatically when its viewer is dropped; idle views never request a redraw.
use std::time::Duration;

use gpui::Context;

use crate::{PdfEvent, PdfView, current_page, display_height, page_top_y};

const INTERVAL: Duration = Duration::from_millis(150);

#[derive(Clone, Copy, Debug, PartialEq)]
struct Position {
    page: usize,
    bucket: u8,
}

fn position(dims: &[(f32, f32)], width: f32, scroll_y: f32) -> Position {
    let page = current_page(dims, width, scroll_y);
    let height = dims
        .get(page)
        .map_or(1.0, |dim| display_height(*dim, width));
    let fraction = ((scroll_y - page_top_y(dims, width, page)) / height.max(1.0)).clamp(0.0, 1.0);
    Position {
        page,
        bucket: (fraction * 20.0).floor() as u8,
    }
}

#[derive(Default)]
struct Coalescer {
    last: Option<Position>,
}

impl Coalescer {
    fn observe(&mut self, position: Position) -> bool {
        if self.last == Some(position) {
            return false;
        }
        self.last = Some(position);
        true
    }
}

pub(super) fn start(cx: &mut Context<PdfView>) {
    cx.spawn(async move |this, cx| {
        let mut changes = Coalescer::default();
        loop {
            cx.background_executor().timer(INTERVAL).await;
            if this
                .update(cx, |view, cx| {
                    if view.dims.is_empty() || f32::from(view.scroll.bounds().size.height) < 1.0 {
                        return;
                    }
                    let position = position(
                        &view.dims,
                        view.page_width(),
                        f32::from(-view.scroll.offset().y),
                    );
                    if changes.observe(position) {
                        cx.emit(PdfEvent::PageChanged {
                            page: position.page,
                            scroll_frac: f32::from(position.bucket) / 20.0,
                        });
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PAGE_GAP, PAGE_PAD_Y, PAGE_WIDTH};

    #[test]
    fn page_and_fraction_follow_variable_page_heights() {
        let dims = [(612.0, 792.0), (612.0, 400.0)];
        let height = display_height(dims[0], PAGE_WIDTH);
        assert_eq!(
            position(&dims, PAGE_WIDTH, -20.0),
            Position { page: 0, bucket: 0 }
        );
        assert_eq!(
            position(&dims, PAGE_WIDTH, PAGE_PAD_Y + height / 2.0),
            Position {
                page: 0,
                bucket: 10
            }
        );
        assert_eq!(
            position(&dims, PAGE_WIDTH, PAGE_PAD_Y + height + PAGE_GAP / 2.0),
            Position { page: 1, bucket: 0 }
        );
        assert_eq!(
            position(&dims, PAGE_WIDTH, 1e6),
            Position {
                page: 1,
                bucket: 20
            }
        );
    }

    #[test]
    fn sampling_coalesces_and_deduplicates() {
        let mut changes = Coalescer::default();
        let first = Position { page: 0, bucket: 0 };
        assert!(changes.observe(first));
        assert!(!changes.observe(first));
        // Only the latest position is sampled each 150ms, not intervening wheel events.
        assert!(changes.observe(Position {
            page: 3,
            bucket: 10
        }));
        assert!(!changes.observe(Position {
            page: 3,
            bucket: 10
        }));
        assert!(changes.observe(first));
    }
}
