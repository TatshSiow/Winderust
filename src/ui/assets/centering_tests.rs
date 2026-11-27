use super::draw_icon;
use iced::advanced::renderer::Headless as _;
use iced::{Color, Font, Pixels, Rectangle, Renderer, Size};

#[test]
fn stepper_icons_keep_their_ink_centered_at_fractional_dpi() {
    let size = Size::new(64, 64);
    for path in ["icons/minus.svg", "icons/plus.svg"] {
        for scale in [1.0, 1.25, 1.5, 1.75] {
            // The SVG raster path used to truncate the physical origin and dimensions.
            for (x, y) in [(6.0, 11.0), (7.875, 9.75), (10.4, 5.9)] {
                let bounds = Rectangle {
                    x,
                    y,
                    width: 18.0,
                    height: 18.0,
                };
                let mut renderer = Renderer::new(Font::DEFAULT, Pixels(14.0));
                draw_icon(
                    &mut renderer,
                    path,
                    bounds,
                    Color::WHITE.scale_alpha(0.5),
                    0.0,
                );
                let shot = renderer.screenshot(size, scale, Color::BLACK);

                let (mut ink, mut sum_x, mut sum_y) = (0.0, 0.0, 0.0);
                for (index, pixel) in shot.as_chunks::<4>().0.iter().enumerate() {
                    assert!(pixel[0] <= 128, "{path} must dim evenly");
                    let weight = f32::from(pixel[0]);
                    ink += weight;
                    sum_x += weight * ((index % size.width as usize) as f32 + 0.5);
                    sum_y += weight * ((index / size.width as usize) as f32 + 0.5);
                }
                assert!(ink > 0.0, "{path} must be visible");
                let offset_x = sum_x / ink - bounds.center_x() * scale;
                let offset_y = sum_y / ink - bounds.center_y() * scale;
                assert!(
                    offset_x.abs() < 0.4 && offset_y.abs() < 0.4,
                    "{path} at ({x}, {y}) and scale {scale} is offset by ({offset_x}, {offset_y}) physical pixels"
                );
            }
        }
    }
}
