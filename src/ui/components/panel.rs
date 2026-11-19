use gpui::prelude::*;
use gpui::{AnyElement, App, Refineable, StyleRefinement, Window, div};

use super::{ComponentSize, Sizable};
use crate::settings::Settings;
use crate::ui::theme::{Theme, backdrop, overlay, panel, radius};

/// A floating panel to put anything in. By default it is a menu's: the
/// theme's surface color with a hairline border, rounded corners and a
/// shadow, and no padding, as a menu's rows keep their own room.
/// [`overlay`](Self::overlay) makes it a panel on the picture instead,
/// padded, and [`note`](Self::note) is such a panel. Its size sets its
/// corners, its text and, over the picture, its padding. Size, lay out and
/// fill it like a `div`; styles set on it win over its own.
#[derive(IntoElement, Default)]
pub struct Panel {
    style: StyleRefinement,
    children: Vec<AnyElement>,
    occlude: bool,
    overlay: bool,
    size: ComponentSize,
}

impl Panel {
    pub fn new() -> Self {
        Self::default()
    }

    /// A note over the picture: a line on a pill.
    pub fn note(text: impl IntoElement) -> Self {
        Self::new()
            .overlay()
            .rounded_full()
            .py(panel::NOTE_PADDING_Y)
            .child(text)
    }

    /// For drawing over video, which is dark in both themes: light text
    /// and well-rounded superellipse corners, no border or shadow, over the
    /// picture as Settings' panel look shows it (see-through black when
    /// plain). Use `overlay::text_muted()` for dimmer text in it.
    pub fn overlay(mut self) -> Self {
        self.overlay = true;
        self
    }

    /// Take the mouse, so clicks and hovers don't reach what is under it:
    /// for menus and dialogs, but not for notes, which would stop the tile
    /// under them counting as hovered.
    pub fn occlude(mut self) -> Self {
        self.occlude = true;
        self
    }
}

impl Sizable for Panel {
    fn with_size(mut self, size: ComponentSize) -> Self {
        self.size = size;
        self
    }
}

impl Styled for Panel {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl ParentElement for Panel {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for Panel {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let size = self.size;
        let panel = size.text(div().overflow_hidden());
        let mut panel = if self.overlay {
            let panel = panel
                .superellipse(size.pick(radius::OVERLAY))
                .p(size.pick(panel::OVERLAY_PADDING))
                .text_color(overlay::text());

            match backdrop::for_look(cx.global::<Settings>().panels) {
                Some(backdrop) => panel.backdrop(backdrop),
                None => panel.bg(overlay::scrim()),
            }
        } else {
            let theme = Theme::get(window, cx);
            panel
                .superellipse(size.pick(radius::MENU))
                .border_1()
                .border_color(theme.border)
                .bg(theme.surface)
                .shadow_lg()
                .text_color(theme.text)
        };
        panel.style().refine(&self.style);
        panel
            .when(self.occlude, |this| this.occlude())
            .children(self.children)
    }
}

/// How each look draws the picture under an overlay panel, drawn off
/// screen by the renderer windows use on macOS (Linux's has the same
/// shaders in WGSL).
#[cfg(all(test, target_os = "macos"))]
mod tests {
    use gpui::{
        BackdropStyle, Background, Bounds, Corners, DevicePixels, Pixels, black, hsla, point, px,
        size, white,
    };

    use crate::settings::PanelLook;
    use crate::ui::theme::backdrop;

    const WIDTH: i32 = 400;
    const HEIGHT: i32 = 240;
    /// The panel, across the middle: its middle row, and the rows of its
    /// top and bottom edges.
    const LEFT: f32 = 40.;
    const TOP: f32 = 100.;
    const BOTTOM: f32 = 160.;
    const MIDDLE: i32 = 130;

    fn rect(x: f32, y: f32, width: f32, height: f32) -> Bounds<Pixels> {
        Bounds::new(point(px(x), px(y)), size(px(width), px(height)))
    }

    fn look(look: PanelLook) -> BackdropStyle {
        backdrop::for_look(look).expect("a backdrop")
    }

    /// Draw `under`, then the panel with `style` over it.
    fn draw(under: &[(Bounds<Pixels>, Background)], style: BackdropStyle) -> Picture {
        let panel = (
            rect(LEFT, TOP, 320., BOTTOM - TOP),
            Corners::all(px(16.)),
            style,
        );
        Picture(gpui::render_offscreen(
            size(DevicePixels(WIDTH), DevicePixels(HEIGHT)),
            under,
            &[panel],
        ))
    }

    struct Picture(Vec<[f32; 4]>);

    impl Picture {
        fn rgb(&self, x: i32, y: i32) -> [f32; 3] {
            let [r, g, b, _] = self.0[(y * WIDTH + x) as usize];
            [r, g, b]
        }

        fn luma(&self, x: i32, y: i32) -> f32 {
            let [r, g, b] = self.rgb(x, y);
            0.2126 * r + 0.7152 * g + 0.0722 * b
        }
    }

    /// Black on the left, white from the middle on.
    fn black_and_white() -> Vec<(Bounds<Pixels>, Background)> {
        vec![
            (rect(0., 0., 400., 240.), black().into()),
            (rect(200., 0., 200., 240.), white().into()),
        ]
    }

    #[test]
    fn frosted_blurs_and_dims_what_is_under_it_and_only_that() {
        let picture = draw(&black_and_white(), look(PanelLook::Frosted));
        let (dark, light) = (picture.luma(150, MIDDLE), picture.luma(250, MIDDLE));
        let (near_dark, near_light) = (picture.luma(194, MIDDLE), picture.luma(206, MIDDLE));
        assert!(
            light > dark + 0.2,
            "it shows which side is light: {dark} {light}"
        );
        assert!(
            near_light - near_dark < (light - dark) / 2.,
            "the edge under it blurs: {near_dark} {near_light}, sides {dark} {light}"
        );
        assert!(light < 0.8, "and dims: {light}");
        assert!(
            picture.luma(199, 40) < 0.01 && picture.luma(200, 40) > 0.99,
            "the edge above it stays sharp: {} {}",
            picture.luma(199, 40),
            picture.luma(200, 40)
        );
    }

    #[test]
    fn tint_takes_the_colors_around_and_shows_nothing_of_their_shapes() {
        // Red stripes on black, 4 pixels each.
        let mut under = vec![(rect(0., 0., 400., 240.), black().into())];
        under.extend((0..50).map(|i| {
            (
                rect(i as f32 * 8., 0., 4., 240.),
                hsla(0., 1., 0.5, 1.).into(),
            )
        }));
        let picture = draw(&under, look(PanelLook::Tint));
        let reds: Vec<f32> = (100..300).map(|x| picture.rgb(x, MIDDLE)[0]).collect();
        let (low, high) = reds
            .iter()
            .fold((1f32, 0f32), |(low, high), &r| (low.min(r), high.max(r)));
        assert!(high - low < 0.02, "no stripes: reds from {low} to {high}");
        let [r, g, b] = picture.rgb(200, MIDDLE);
        assert!(r > g + 0.1 && r > b + 0.1, "but their color: {r} {g} {b}");
    }

    #[test]
    fn glass_has_a_lit_polished_edge_that_bends_light() {
        let gray = [(rect(0., 0., 400., 240.), hsla(0., 0., 0.3, 1.).into())];
        let picture = draw(&gray, look(PanelLook::Glass));
        let (rim, middle) = (picture.luma(200, TOP as i32), picture.luma(200, MIDDLE));
        assert!(rim > middle + 0.1, "a rim of light: {rim} over {middle}");

        let picture = draw(&black_and_white(), look(PanelLook::Glass));
        let step = |y: i32| picture.luma(200, y) - picture.luma(199, y);
        assert!(
            step(TOP as i32) > 2. * step(MIDDLE),
            "its edge is clear where its top is frosted: {} {}",
            step(TOP as i32),
            step(MIDDLE)
        );

        // A white bar just inside the panel shows at its edge, as the edge
        // magnifies what's under it.
        let under = [
            (rect(0., 0., 400., 240.), black().into()),
            (rect(LEFT + 5., 0., 2., 240.), white().into()),
        ];
        let flat = BackdropStyle {
            refraction: px(0.),
            ..look(PanelLook::Glass)
        };
        let (bent, straight) = (
            draw(&under, look(PanelLook::Glass)).luma(LEFT as i32 + 1, MIDDLE),
            draw(&under, flat).luma(LEFT as i32 + 1, MIDDLE),
        );
        assert!(
            bent > straight + 0.1,
            "the bar shows at the edge: {bent} over {straight}"
        );
    }

    #[test]
    fn glow_lights_up_around_it_with_the_colors_there() {
        let red = hsla(0., 1., 0.25, 1.);
        let under = [(rect(0., 0., 400., 240.), red.into())];
        let (glow, frosted) = (
            draw(&under, look(PanelLook::Glow)),
            draw(&under, look(PanelLook::Frosted)),
        );
        let background = frosted.rgb(200, 230)[0];
        let near = BOTTOM as i32 + 6;
        let [r, g, _] = glow.rgb(200, near);
        assert!(r > background + 0.05, "a red halo: {r} over {background}");
        assert!(r > g + 0.3, "red: {r} {g}");
        assert!(
            (glow.rgb(200, 230)[0] - background).abs() < 0.01,
            "that fades away: {}",
            glow.rgb(200, 230)[0]
        );
        assert!(
            (frosted.rgb(200, near)[0] - background).abs() < 0.01,
            "frosted has none: {}",
            frosted.rgb(200, near)[0]
        );
    }
}
