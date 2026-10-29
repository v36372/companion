use gpui::{Bounds, Pixels, Point, point, px};

#[derive(Debug, PartialEq)]
pub enum EscapeTarget {
    CloseDiagram,
    Input,
    Selection,
    CloseSurface,
    Abort,
}

pub fn escape_target(diagram: bool, input: bool, selection: bool, surface: bool) -> EscapeTarget {
    if diagram {
        EscapeTarget::CloseDiagram
    } else if input {
        EscapeTarget::Input
    } else if selection {
        EscapeTarget::Selection
    } else if surface {
        EscapeTarget::CloseSurface
    } else {
        EscapeTarget::Abort
    }
}

/// Position a fixed-size pill in window coordinates, clamped to the reader viewport.
pub fn position(anchor: Option<Bounds<Pixels>>, reader: Bounds<Pixels>) -> Point<Pixels> {
    let left = f32::from(reader.origin.x);
    let top = f32::from(reader.origin.y);
    let width = f32::from(reader.size.width);
    let height = f32::from(reader.size.height);
    let (w, h, gap) = (210.0_f32, 40.0_f32, 4.0_f32);
    let (x, y) = if let Some(anchor) = anchor {
        let x = f32::from(anchor.origin.x);
        let y = f32::from(anchor.origin.y);
        (
            x,
            if y - h - gap >= top {
                y - h - gap
            } else {
                y + f32::from(anchor.size.height) + gap
            },
        )
    } else {
        (left + (width - w) / 2.0, top + gap)
    };
    point(
        px(x.clamp(left, (left + width - w).max(left))),
        px(y.clamp(top, (top + height - h).max(top))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::size;

    #[test]
    fn escape_order() {
        assert_eq!(
            escape_target(true, true, true, true),
            EscapeTarget::CloseDiagram
        );
        assert_eq!(escape_target(false, true, true, true), EscapeTarget::Input);
        assert_eq!(
            escape_target(false, false, true, true),
            EscapeTarget::Selection
        );
        assert_eq!(
            escape_target(false, false, false, true),
            EscapeTarget::CloseSurface
        );
        assert_eq!(
            escape_target(false, false, false, false),
            EscapeTarget::Abort
        );
    }

    #[test]
    fn placement_flips_and_clamps() {
        let reader = Bounds {
            origin: point(px(100.), px(100.)),
            size: size(px(300.), px(300.)),
        };
        let anchor = |x, y| {
            Some(Bounds {
                origin: point(px(x), px(y)),
                size: size(px(20.), px(10.)),
            })
        };
        assert_eq!(
            position(anchor(120., 200.), reader),
            point(px(120.), px(156.))
        );
        assert_eq!(
            position(anchor(120., 110.), reader),
            point(px(120.), px(124.))
        );
        assert_eq!(
            position(anchor(390., 500.), reader),
            point(px(190.), px(360.))
        );
    }
}
