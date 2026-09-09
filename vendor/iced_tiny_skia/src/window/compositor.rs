use crate::core::{Color, Rectangle, Size};
use crate::graphics::compositor::{self, Information};
use crate::graphics::damage;
use crate::graphics::error::{self, Error};
use crate::graphics::{self, Shell, Viewport};
use crate::{Layer, Renderer, Settings};

use std::collections::VecDeque;
use std::num::NonZeroU32;

pub struct Compositor {
    context: softbuffer::Context<Box<dyn compositor::Display>>,
    settings: Settings,
}

pub struct Surface {
    window: softbuffer::Surface<
        Box<dyn compositor::Display>,
        Box<dyn compositor::Window>,
    >,
    clip_mask: tiny_skia::Mask,
    layer_stack: VecDeque<Vec<Layer>>,
    background_color: Color,
    max_age: u8,
}

impl crate::graphics::Compositor for Compositor {
    type Renderer = Renderer;
    type Surface = Surface;

    async fn with_backend(
        settings: graphics::Settings,
        display: impl compositor::Display,
        _compatible_window: impl compositor::Window,
        _shell: Shell,
        backend: Option<&str>,
    ) -> Result<Self, Error> {
        match backend {
            None | Some("tiny-skia") | Some("tiny_skia") => {
                Ok(new(settings.into(), display))
            }
            Some(backend) => Err(Error::GraphicsAdapterNotFound {
                backend: "tiny-skia",
                reason: error::Reason::DidNotMatch {
                    preferred_backend: backend.to_owned(),
                },
            }),
        }
    }

    fn create_renderer(&self) -> Self::Renderer {
        Renderer::new(
            self.settings.default_font,
            self.settings.default_text_size,
        )
    }

    fn create_surface<W: compositor::Window + Clone>(
        &mut self,
        window: W,
        width: u32,
        height: u32,
    ) -> Self::Surface {
        let window = softbuffer::Surface::new(
            &self.context,
            Box::new(window.clone()) as _,
        )
        .expect("Create softbuffer surface for window");

        let mut surface = Surface {
            window,
            clip_mask: tiny_skia::Mask::new(1, 1).expect("Create clip mask"),
            layer_stack: VecDeque::new(),
            background_color: Color::BLACK,
            max_age: 0,
        };

        if width > 0 && height > 0 {
            self.configure_surface(&mut surface, width, height);
        }

        surface
    }

    fn configure_surface(
        &mut self,
        surface: &mut Self::Surface,
        width: u32,
        height: u32,
    ) {
        surface
            .window
            .resize(
                NonZeroU32::new(width).expect("Non-zero width"),
                NonZeroU32::new(height).expect("Non-zero height"),
            )
            .expect("Resize surface");

        surface.clip_mask =
            tiny_skia::Mask::new(width, height).expect("Create clip mask");
        surface.layer_stack.clear();
    }

    fn information(&self) -> Information {
        Information {
            adapter: String::from("CPU"),
            backend: String::from("tiny-skia"),
        }
    }

    fn present(
        &mut self,
        renderer: &mut Self::Renderer,
        surface: &mut Self::Surface,
        viewport: &Viewport,
        background_color: Color,
        on_pre_present: impl FnOnce(),
    ) -> Result<(), compositor::SurfaceError> {
        present(
            renderer,
            surface,
            viewport,
            background_color,
            on_pre_present,
        )
    }

    fn screenshot(
        &mut self,
        renderer: &mut Self::Renderer,
        viewport: &Viewport,
        background_color: Color,
    ) -> Vec<u8> {
        screenshot(renderer, viewport, background_color)
    }
}

pub fn new(
    settings: Settings,
    display: impl compositor::Display,
) -> Compositor {
    #[allow(unsafe_code)]
    let context = softbuffer::Context::new(Box::new(display) as _)
        .expect("Create softbuffer context");

    Compositor { context, settings }
}

pub fn present(
    renderer: &mut Renderer,
    surface: &mut Surface,
    viewport: &Viewport,
    background_color: Color,
    on_pre_present: impl FnOnce(),
) -> Result<(), compositor::SurfaceError> {
    let physical_size = viewport.physical_size();

    let mut buffer = surface
        .window
        .buffer_mut()
        .map_err(|_| compositor::SurfaceError::Lost)?;

    let last_layers = {
        let age = buffer.age();

        surface.max_age = surface.max_age.max(age);
        surface.layer_stack.truncate(surface.max_age as usize);

        if age > 0 {
            surface.layer_stack.get(age as usize - 1)
        } else {
            None
        }
    };

    let damage = last_layers
        .and_then(|last_layers| {
            (surface.background_color == background_color).then(|| {
                damage::diff(
                    last_layers,
                    renderer.layers(),
                    |layer| vec![layer.bounds],
                    Layer::damage,
                )
            })
        })
        .unwrap_or_else(|| vec![Rectangle::with_size(viewport.logical_size())]);

    if damage.is_empty() {
        if let Some(last_layers) = last_layers {
            surface.layer_stack.push_front(last_layers.clone());
        }
    } else {
        surface.layer_stack.push_front(renderer.layers().to_vec());
        surface.background_color = background_color;

        let damage = coalesce_damage(damage::group(
            damage,
            Rectangle::with_size(viewport.logical_size()),
        ));

        let mut pixels = tiny_skia::PixmapMut::from_bytes(
            bytemuck::cast_slice_mut(&mut buffer),
            physical_size.width,
            physical_size.height,
        )
        .expect("Create pixel map");

        renderer.draw(
            &mut pixels,
            &mut surface.clip_mask,
            viewport,
            &damage,
            background_color,
        );
    }

    on_pre_present();
    buffer.present().map_err(|_| compositor::SurfaceError::Lost)
}

// Each region replays the scene and rebuilds window-sized clipping masks.
// Bound that cost for dense tables while retaining sparse partial redraws.
fn coalesce_damage(mut damage: Vec<Rectangle>) -> Vec<Rectangle> {
    if damage.len() > 8 {
        if let Some(bounds) = damage.iter().copied().reduce(|a, b| a.union(&b))
        {
            damage.clear();
            damage.push(bounds);
        }
    }
    damage
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::renderer::Renderer as _;

    #[test]
    fn coalesced_redraw_matches_full_redraw() {
        let size = Size::new(256, 128);
        let viewport = Viewport::with_physical_size(size, 1.0);
        let full = Rectangle::with_size(Size::new(256.0, 128.0));
        let regions: Vec<_> = (0..9)
            .map(|i| {
                Rectangle::new(
                    crate::core::Point::new(i as f32 * 26.0, 20.0),
                    Size::new(20.0, 30.0),
                )
            })
            .collect();
        assert!(coalesce_damage(Vec::new()).is_empty());
        assert_eq!(coalesce_damage(regions[..8].to_vec()), regions[..8]);
        let merged = coalesce_damage(regions.clone());
        assert_eq!(merged.len(), 1);
        assert!(regions.iter().all(|region| region.is_within(&merged[0])));

        let mut renderer =
            Renderer::new(crate::core::Font::default(), 14.0.into());
        let mut partial =
            tiny_skia::Pixmap::new(size.width, size.height).unwrap();
        let mut expected = partial.clone();
        let mut mask = tiny_skia::Mask::new(size.width, size.height).unwrap();
        for color in [Color::BLACK, Color::from_rgba(0.2, 0.6, 0.9, 0.5)] {
            renderer.reset(full);
            for bounds in &regions {
                renderer.fill_quad(
                    crate::core::renderer::Quad {
                        bounds: *bounds,
                        ..Default::default()
                    },
                    color,
                );
            }
            let damage = if color == Color::BLACK {
                &[full][..]
            } else {
                &merged
            };
            renderer.draw(
                &mut partial.as_mut(),
                &mut mask,
                &viewport,
                damage,
                Color::WHITE,
            );
            renderer.draw(
                &mut expected.as_mut(),
                &mut mask,
                &viewport,
                &[full],
                Color::WHITE,
            );
        }
        assert_eq!(partial.data(), expected.data());
    }
}

pub fn screenshot(
    renderer: &mut Renderer,
    viewport: &Viewport,
    background_color: Color,
) -> Vec<u8> {
    let size = viewport.physical_size();

    let mut offscreen_buffer: Vec<u32> =
        vec![0; size.width as usize * size.height as usize];

    let mut clip_mask = tiny_skia::Mask::new(size.width, size.height)
        .expect("Create clip mask");

    renderer.draw(
        &mut tiny_skia::PixmapMut::from_bytes(
            bytemuck::cast_slice_mut(&mut offscreen_buffer),
            size.width,
            size.height,
        )
        .expect("Create offscreen pixel map"),
        &mut clip_mask,
        viewport,
        &[Rectangle::with_size(Size::new(
            size.width as f32,
            size.height as f32,
        ))],
        background_color,
    );

    offscreen_buffer.iter().fold(
        Vec::with_capacity(offscreen_buffer.len() * 4),
        |mut acc, pixel| {
            const A_MASK: u32 = 0xFF_00_00_00;
            const R_MASK: u32 = 0x00_FF_00_00;
            const G_MASK: u32 = 0x00_00_FF_00;
            const B_MASK: u32 = 0x00_00_00_FF;

            let a = ((A_MASK & pixel) >> 24) as u8;
            let r = ((R_MASK & pixel) >> 16) as u8;
            let g = ((G_MASK & pixel) >> 8) as u8;
            let b = (B_MASK & pixel) as u8;

            acc.extend([r, g, b, a]);
            acc
        },
    )
}
