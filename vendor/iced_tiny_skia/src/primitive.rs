use crate::core::Rectangle;

#[derive(Debug, Clone, PartialEq)]
pub enum Primitive {
    /// A path filled with some paint.
    Fill {
        /// The path to fill.
        path: tiny_skia::Path,
        /// The paint to use.
        paint: tiny_skia::Paint<'static>,
        /// The fill rule to follow.
        rule: tiny_skia::FillRule,
    },
    /// A path stroked with some paint.
    Stroke {
        /// The path to stroke.
        path: tiny_skia::Path,
        /// The paint to use.
        paint: tiny_skia::Paint<'static>,
        /// The stroke settings.
        stroke: tiny_skia::Stroke,
    },
}

impl Primitive {
    /// Returns the visible bounds of the [`Primitive`].
    pub fn visible_bounds(&self) -> Rectangle {
        let (bounds, padding) = match self {
            Primitive::Fill { path, .. } => (path.bounds(), 1.0),
            Primitive::Stroke { path, stroke, .. } => (
                path.bounds(),
                // Include caps, miter joins, and antialiasing beyond the path.
                stroke.width / 2.0 * stroke.miter_limit.max(std::f32::consts::SQRT_2) + 1.0,
            ),
        };

        Rectangle {
            x: bounds.x(),
            y: bounds.y(),
            width: bounds.width(),
            height: bounds.height(),
        }
        .expand(padding)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn axis_aligned_strokes_have_nonzero_repaint_bounds() {
        for end in [(100.0, 0.0), (0.0, 100.0)] {
            let mut path = tiny_skia::PathBuilder::new();
            path.move_to(0.0, 0.0);
            path.line_to(end.0, end.1);
            let primitive = Primitive::Stroke {
                path: path.finish().unwrap(),
                paint: tiny_skia::Paint::default(),
                stroke: tiny_skia::Stroke {
                    width: 1.8,
                    ..Default::default()
                },
            };
            let bounds = primitive.visible_bounds();
            assert!(bounds.width > 1.8 && bounds.height > 1.8);
            assert!(bounds.x < 0.0 && bounds.y < 0.0);
            assert!(bounds.x + bounds.width > end.0);
            assert!(bounds.y + bounds.height > end.1);
        }
    }
}
