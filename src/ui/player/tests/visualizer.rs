use gpui::prelude::*;
use gpui::{Bounds, Context, TestAppContext, VisualizerShader, Window, div, point, px, red};

use super::speaker;

/// A speaker at 0.3 in a block drawn half see-through.
struct Faded;

impl Render for Faded {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .opacity(0.5)
            .child(speaker(0.3, red().opacity(0.8)))
    }
}

/// It fills what it's in, in its color, as see-through as that and what
/// it's in together.
#[gpui::test]
fn a_speaker_is_as_see_through_as_what_its_in(cx: &mut TestAppContext) {
    let (_, cx) = cx.add_window_view(|_, _| Faded);
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    let (painted, viewport) =
        cx.update(|window, _| (window.painted_visualizers(), window.viewport_size()));
    assert_eq!(
        painted,
        [(
            Bounds::new(point(px(0.), px(0.)), viewport),
            VisualizerShader::Speaker { level: 0.3 },
            red().opacity(0.4)
        )]
    );
}

/// The speaker, drawn off screen by the renderer windows use on macOS
/// (Linux's has the same shader in WGSL), at a point a pixel.
#[cfg(target_os = "macos")]
mod drawn {
    use gpui::{Bounds, DevicePixels, Hsla, VisualizerShader, black, point, px, size, white};

    /// The speaker's radius, its rim's, in a block whose shorter side is
    /// 200 px.
    const RADIUS: f32 = 0.46 * 200.;

    /// A speaker in `color` at `level`, over black, in a block `width` by 200
    /// px: how bright each pixel is, row by row.
    struct Speaker {
        width: i32,
        pixels: Vec<[f32; 4]>,
    }

    impl Speaker {
        fn draw(width: i32, level: f32, color: Hsla) -> Self {
            let block = Bounds::new(point(px(0.), px(0.)), size(px(width as f32), px(200.)));
            let pixels = gpui::render_offscreen(
                size(DevicePixels(width), DevicePixels(200)),
                &[(block, black().into())],
                &[(block, VisualizerShader::Speaker { level }, color)],
                &[],
            );
            Self { width, pixels }
        }

        /// How bright the pixel at `x`, `y` in the block is.
        fn at(&self, x: usize, y: usize) -> f32 {
            self.pixels[y * self.width as usize + x][0]
        }

        /// How bright the pixels right of its middle are, from `from` to `to`
        /// radii out, or up from it: how many pixels' worth of light there.
        fn right(&self, from: f32, to: f32) -> f32 {
            let middle = self.width as usize / 2;
            Self::out(from, to).map(|x| self.at(middle + x, 100)).sum()
        }

        fn up(&self, from: f32, to: f32) -> f32 {
            let middle = self.width as usize / 2;
            Self::out(from, to).map(|y| self.at(middle, 99 - y)).sum()
        }

        /// The pixels from `from` to `to` radii out.
        fn out(from: f32, to: f32) -> std::ops::RangeInclusive<usize> {
            (from * RADIUS) as usize..=(to * RADIUS) as usize
        }

        /// How far right of its middle, in radii, the light between `from`
        /// and `to` radii is, on the whole.
        fn middle_of(&self, from: f32, to: f32) -> f32 {
            let middle = self.width as usize / 2;
            let (moment, light) = Self::out(from, to).fold((0., 0.), |(moment, light), x| {
                let shade = self.at(middle + x, 100);
                (moment + shade * (x as f32 + 0.5), light + shade)
            });
            moment / light / RADIUS
        }
    }

    /// Two rings in the middle of its block, as wide as its shorter side
    /// lets them be, as wide across as up: its rim, a pixel wide, and in it
    /// its cone's edge, two pixels wide; and nothing else.
    #[test]
    fn a_speaker_is_two_thin_rings_in_the_middle_of_its_block() {
        for width in [200, 320] {
            let speaker = Speaker::draw(width, 0., white());
            for (from, to, light, what) in [
                (0.97, 1.03, 1., "the rim"),
                (0.8, 0.88, 2., "the cone's edge"),
                (0., 0.8, 0., "inside"),
                (0.88, 0.97, 0., "between them"),
                (1.03, 1.08, 0., "outside"),
            ] {
                let (across, up) = (speaker.right(from, to), speaker.up(from, to));
                assert!(
                    (across - light).abs() < 0.05,
                    "{what} across {width}: {across}"
                );
                assert!((up - light).abs() < 0.05, "{what} up {width}: {up}");
            }
            assert!(speaker.at(2, 2) < 0.01, "a corner of {width}");
        }
    }

    /// With the level, its cone's edge moves out, and its rim stays.
    #[test]
    fn a_speakers_cone_moves_out_with_the_level() {
        let (still, half, loud) = (
            Speaker::draw(200, 0., white()),
            Speaker::draw(200, 0.5, white()),
            Speaker::draw(200, 1., white()),
        );
        let cone = |speaker: &Speaker| speaker.middle_of(0.78, 0.95);
        assert!((cone(&still) - 0.84).abs() < 0.01, "{}", cone(&still));
        assert!((cone(&half) - 0.88).abs() < 0.01, "{}", cone(&half));
        assert!((cone(&loud) - 0.92).abs() < 0.01, "{}", cone(&loud));
        assert!((loud.right(0.89, 0.96) - 2.).abs() < 0.05, "as wide");
        assert!((loud.right(0.97, 1.03) - 1.).abs() < 0.05, "the rim");
        assert_eq!(Speaker::draw(200, 2., white()).pixels, loud.pixels);
    }

    /// Drawn in its color, as see-through as it is.
    #[test]
    fn a_speaker_is_drawn_in_its_color() {
        let speaker = Speaker::draw(200, 0., white().opacity(0.5));
        let rim = speaker.right(0.97, 1.03);
        assert!((rim - 0.5).abs() < 0.03, "{rim}");
        let red = Speaker::draw(200, 0., gpui::red());
        let [r, g, b, _] = red.pixels[100 * 200 + 100 + RADIUS as usize];
        assert!(r > 0.4 && g < 0.05 && b < 0.05, "{r} {g} {b}");
    }
}
