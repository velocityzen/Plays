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
/// (Linux's has the same shader in WGSL).
#[cfg(target_os = "macos")]
mod drawn {
    use gpui::{Bounds, DevicePixels, Hsla, VisualizerShader, black, point, px, size, white};

    /// The speaker's radius in a block whose shorter side is 200 px.
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

        /// How bright it is `r` radii from its middle, to its right, or up:
        /// the pixels as far from it, their middles half a pixel past a whole
        /// one.
        fn right(&self, r: f32) -> f32 {
            let middle = self.width as usize / 2;
            self.at(middle + (r * RADIUS) as usize, 100)
        }

        fn up(&self, r: f32) -> f32 {
            let middle = self.width as usize / 2;
            self.at(middle, 99 - (r * RADIUS) as usize)
        }

        /// How bright each pixel right of its middle is, from `from` to `to`
        /// radii out.
        fn along(&self, from: f32, to: f32) -> Vec<f32> {
            let middle = self.width as usize / 2;
            ((from * RADIUS) as usize..=(to * RADIUS) as usize)
                .map(|x| self.at(middle + x, 100))
                .collect()
        }

        /// How many pixels right of its middle it is darkest, between `from`
        /// and `to` radii.
        fn darkest(&self, from: f32, to: f32) -> usize {
            let middle = self.width as usize / 2;
            ((from * RADIUS) as usize..=(to * RADIUS) as usize)
                .min_by(|a, b| {
                    self.at(middle + a, 100)
                        .total_cmp(&self.at(middle + b, 100))
                })
                .unwrap()
        }
    }

    /// A circle in the middle of its block, as wide as its shorter side lets
    /// it be, and nothing outside it: as wide across as up.
    #[test]
    fn a_speaker_is_a_circle_in_the_middle_of_its_block() {
        for width in [200, 320] {
            let speaker = Speaker::draw(width, 0., white());
            assert!(speaker.at(2, 2) < 0.01, "a corner of {width}");
            for r in [0., 0.5, 0.97] {
                assert!(speaker.right(r) > 0.2, "{r} across {width}");
                assert!(
                    (speaker.right(r) - speaker.up(r)).abs() < 0.02,
                    "{r} up {width}"
                );
            }
            assert!(speaker.right(1.05) < 0.01, "past its rim across {width}");
            assert!(speaker.up(1.05) < 0.01, "past its rim up {width}");
        }
    }

    /// With the level, its cap grows and brightens, and the surround, darkest
    /// where it meets the cone, narrows.
    #[test]
    fn a_speakers_cone_moves_out_with_the_level() {
        let (still, loud) = (
            Speaker::draw(200, 0., white()),
            Speaker::draw(200, 1., white()),
        );
        assert!((still.right(0.) - 0.75).abs() < 0.02, "{}", still.right(0.));
        assert!(loud.right(0.) > 0.97, "{}", loud.right(0.));
        // The cap, wider than it was.
        assert!(loud.right(0.32) > still.right(0.32) + 0.3);
        let (from, to) = (still.darkest(0.75, 0.95), loud.darkest(0.75, 0.95));
        assert!(to > from + 3, "the surround from {from} px to {to} px");
        // Each half way, and the same past it.
        let half = Speaker::draw(200, 0.5, white());
        assert!(half.right(0.) > still.right(0.) + 0.1 && half.right(0.) < loud.right(0.) - 0.1);
        assert_eq!(Speaker::draw(200, 2., white()).pixels, loud.pixels);
    }

    /// Its cone brightens out from the cap, ridged as it goes, and its
    /// surround is rolled, brightest half way across.
    #[test]
    fn a_speakers_cone_is_ridged_and_its_surround_rolled() {
        let speaker = Speaker::draw(200, 0., white());
        let mean = |from, to| {
            let pixels = speaker.along(from, to);
            pixels.iter().sum::<f32>() / pixels.len() as f32
        };
        let (inner, outer) = (mean(0.35, 0.45), mean(0.65, 0.75));
        assert!(outer > inner + 0.08, "{inner} out to {outer}");
        let cone = speaker.along(0.3, 0.8);
        let ridges = cone
            .windows(3)
            .filter(|three| three[1] > three[0] && three[1] >= three[2])
            .count();
        assert!(ridges >= 5, "{ridges} ridges: {cone:?}");
        let (inside, middle, outside) = (
            speaker.right(0.83),
            speaker.right(0.885),
            speaker.right(0.94),
        );
        assert!(
            middle > inside + 0.15 && middle > outside + 0.15,
            "{inside}, {middle}, {outside}"
        );
    }

    /// Drawn in its color, as see-through as it is.
    #[test]
    fn a_speaker_is_drawn_in_its_color() {
        let speaker = Speaker::draw(200, 0., white().opacity(0.5));
        assert!(
            (speaker.right(0.) - 0.375).abs() < 0.02,
            "{}",
            speaker.right(0.)
        );
        let red = Speaker::draw(200, 0., gpui::red());
        let [r, g, b, _] = red.pixels[100 * 200 + 100];
        assert!(r > 0.5 && g < 0.05 && b < 0.05, "{r} {g} {b}");
    }
}
