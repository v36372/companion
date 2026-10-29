use gpui::{Bounds, Pixels, Point, point, px};

#[derive(Debug, PartialEq)]
pub enum EscapeTarget {
    Peek,
    CloseDiagram,
    Input,
    Selection,
    CloseSurface,
    Abort,
}

pub fn escape_target(
    peek: bool,
    diagram: bool,
    input: bool,
    selection: bool,
    surface: bool,
) -> EscapeTarget {
    if peek {
        EscapeTarget::Peek
    } else if diagram {
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
        for bits in 0..32 {
            let [peek, diagram, input, selection, surface] =
                std::array::from_fn(|i| bits & (1 << i) != 0);
            let expected = if peek {
                EscapeTarget::Peek
            } else if diagram {
                EscapeTarget::CloseDiagram
            } else if input {
                EscapeTarget::Input
            } else if selection {
                EscapeTarget::Selection
            } else if surface {
                EscapeTarget::CloseSurface
            } else {
                EscapeTarget::Abort
            };
            assert_eq!(
                escape_target(peek, diagram, input, selection, surface),
                expected
            );
        }
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
