// todo("windows"): remove
#![cfg_attr(windows, allow(dead_code))]

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    AtlasTextureId, AtlasTile, Background, Bounds, ContentMask, Corners, Edges, Hsla, Pixels,
    Point, Radians, ScaledPixels, Size, bounds_tree::BoundsTree, point,
};
use std::{
    fmt::Debug,
    iter::Peekable,
    ops::{Add, Range, Sub},
    slice,
};

#[allow(non_camel_case_types, unused)]
pub(crate) type PathVertex_ScaledPixels = PathVertex<ScaledPixels>;

pub(crate) type DrawOrder = u32;

#[derive(Default)]
pub(crate) struct Scene {
    pub(crate) paint_operations: Vec<PaintOperation>,
    primitive_bounds: BoundsTree<ScaledPixels>,
    layer_stack: Vec<DrawOrder>,
    pub(crate) shadows: Vec<Shadow>,
    pub(crate) quads: Vec<Quad>,
    pub(crate) paths: Vec<Path<ScaledPixels>>,
    pub(crate) underlines: Vec<Underline>,
    pub(crate) monochrome_sprites: Vec<MonochromeSprite>,
    pub(crate) polychrome_sprites: Vec<PolychromeSprite>,
    pub(crate) surfaces: Vec<PaintSurface>,
    /// Plays: see `Backdrop`.
    pub(crate) backdrops: Vec<Backdrop>,
    /// Plays: see `Visualizer`.
    pub(crate) visualizers: Vec<Visualizer>,
}

impl Scene {
    pub fn clear(&mut self) {
        self.paint_operations.clear();
        self.primitive_bounds.clear();
        self.layer_stack.clear();
        self.paths.clear();
        self.shadows.clear();
        self.quads.clear();
        self.underlines.clear();
        self.monochrome_sprites.clear();
        self.polychrome_sprites.clear();
        self.surfaces.clear();
        self.backdrops.clear();
        self.visualizers.clear();
    }

    pub fn len(&self) -> usize {
        self.paint_operations.len()
    }

    pub fn push_layer(&mut self, bounds: Bounds<ScaledPixels>) {
        let order = self.primitive_bounds.insert(bounds);
        self.layer_stack.push(order);
        self.paint_operations
            .push(PaintOperation::StartLayer(bounds));
    }

    pub fn pop_layer(&mut self) {
        self.layer_stack.pop();
        self.paint_operations.push(PaintOperation::EndLayer);
    }

    pub fn insert_primitive(&mut self, primitive: impl Into<Primitive>) {
        let mut primitive = primitive.into();
        let clipped_bounds = primitive
            .bounds()
            .intersect(&primitive.content_mask().bounds);

        if clipped_bounds.is_empty() {
            return;
        }

        // Plays: a backdrop goes over what's drawn before it as far around
        // as it reads, not just where it draws, so that's all drawn first.
        let claimed = match &primitive {
            Primitive::Backdrop(backdrop) => clipped_bounds.dilate(backdrop.reach()),
            _ => clipped_bounds,
        };
        let order = self
            .layer_stack
            .last()
            .copied()
            .unwrap_or_else(|| self.primitive_bounds.insert(claimed));
        match &mut primitive {
            Primitive::Shadow(shadow) => {
                shadow.order = order;
                self.shadows.push(shadow.clone());
            }
            Primitive::Quad(quad) => {
                quad.order = order;
                self.quads.push(quad.clone());
            }
            Primitive::Path(path) => {
                path.order = order;
                path.id = PathId(self.paths.len());
                self.paths.push(path.clone());
            }
            Primitive::Underline(underline) => {
                underline.order = order;
                self.underlines.push(underline.clone());
            }
            Primitive::MonochromeSprite(sprite) => {
                sprite.order = order;
                self.monochrome_sprites.push(sprite.clone());
            }
            Primitive::PolychromeSprite(sprite) => {
                sprite.order = order;
                self.polychrome_sprites.push(sprite.clone());
            }
            Primitive::Surface(surface) => {
                surface.order = order;
                self.surfaces.push(surface.clone());
            }
            Primitive::Backdrop(backdrop) => {
                backdrop.order = order;
                self.backdrops.push(backdrop.clone());
            }
            Primitive::Visualizer(visualizer) => {
                visualizer.order = order;
                self.visualizers.push(visualizer.clone());
            }
        }
        self.paint_operations
            .push(PaintOperation::Primitive(primitive));
    }

    pub fn replay(&mut self, range: Range<usize>, prev_scene: &Scene) {
        for operation in &prev_scene.paint_operations[range] {
            match operation {
                PaintOperation::Primitive(primitive) => self.insert_primitive(primitive.clone()),
                PaintOperation::StartLayer(bounds) => self.push_layer(*bounds),
                PaintOperation::EndLayer => self.pop_layer(),
            }
        }
    }

    pub fn finish(&mut self) {
        self.shadows.sort_by_key(|shadow| shadow.order);
        self.quads.sort_by_key(|quad| quad.order);
        self.paths.sort_by_key(|path| path.order);
        self.underlines.sort_by_key(|underline| underline.order);
        self.monochrome_sprites
            .sort_by_key(|sprite| (sprite.order, sprite.tile.tile_id));
        self.polychrome_sprites
            .sort_by_key(|sprite| (sprite.order, sprite.tile.tile_id));
        self.surfaces.sort_by_key(|surface| surface.order);
        self.backdrops.sort_by_key(|backdrop| backdrop.order);
        self.visualizers.sort_by_key(|visualizer| visualizer.order);
    }

    #[cfg_attr(
        all(
            any(target_os = "linux", target_os = "freebsd"),
            not(any(feature = "x11", feature = "wayland"))
        ),
        allow(dead_code)
    )]
    pub(crate) fn batches(&self) -> impl Iterator<Item = PrimitiveBatch<'_>> {
        BatchIterator {
            shadows: &self.shadows,
            shadows_start: 0,
            shadows_iter: self.shadows.iter().peekable(),
            quads: &self.quads,
            quads_start: 0,
            quads_iter: self.quads.iter().peekable(),
            paths: &self.paths,
            paths_start: 0,
            paths_iter: self.paths.iter().peekable(),
            underlines: &self.underlines,
            underlines_start: 0,
            underlines_iter: self.underlines.iter().peekable(),
            monochrome_sprites: &self.monochrome_sprites,
            monochrome_sprites_start: 0,
            monochrome_sprites_iter: self.monochrome_sprites.iter().peekable(),
            polychrome_sprites: &self.polychrome_sprites,
            polychrome_sprites_start: 0,
            polychrome_sprites_iter: self.polychrome_sprites.iter().peekable(),
            surfaces: &self.surfaces,
            surfaces_start: 0,
            surfaces_iter: self.surfaces.iter().peekable(),
            backdrops: &self.backdrops,
            backdrops_start: 0,
            backdrops_iter: self.backdrops.iter().peekable(),
            visualizers: &self.visualizers,
            visualizers_start: 0,
            visualizers_iter: self.visualizers.iter().peekable(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Default)]
#[cfg_attr(
    all(
        any(target_os = "linux", target_os = "freebsd"),
        not(any(feature = "x11", feature = "wayland"))
    ),
    allow(dead_code)
)]
pub(crate) enum PrimitiveKind {
    /// Plays: first, so a backdrop sharing its draw order with what is
    /// drawn on it (in one layer) goes under it.
    Backdrop,
    Shadow,
    /// Plays: under what's drawn on it in one layer.
    Visualizer,
    #[default]
    Quad,
    Path,
    Underline,
    MonochromeSprite,
    PolychromeSprite,
    Surface,
}

pub(crate) enum PaintOperation {
    Primitive(Primitive),
    StartLayer(Bounds<ScaledPixels>),
    EndLayer,
}

#[derive(Clone)]
pub(crate) enum Primitive {
    Shadow(Shadow),
    Quad(Quad),
    Path(Path<ScaledPixels>),
    Underline(Underline),
    MonochromeSprite(MonochromeSprite),
    PolychromeSprite(PolychromeSprite),
    Surface(PaintSurface),
    Backdrop(Backdrop),
    Visualizer(Visualizer),
}

impl Primitive {
    pub fn bounds(&self) -> &Bounds<ScaledPixels> {
        match self {
            Primitive::Shadow(shadow) => &shadow.bounds,
            Primitive::Quad(quad) => &quad.bounds,
            Primitive::Path(path) => &path.bounds,
            Primitive::Underline(underline) => &underline.bounds,
            Primitive::MonochromeSprite(sprite) => &sprite.bounds,
            Primitive::PolychromeSprite(sprite) => &sprite.bounds,
            Primitive::Surface(surface) => &surface.bounds,
            Primitive::Backdrop(backdrop) => &backdrop.bounds,
            Primitive::Visualizer(visualizer) => &visualizer.bounds,
        }
    }

    pub fn content_mask(&self) -> &ContentMask<ScaledPixels> {
        match self {
            Primitive::Shadow(shadow) => &shadow.content_mask,
            Primitive::Quad(quad) => &quad.content_mask,
            Primitive::Path(path) => &path.content_mask,
            Primitive::Underline(underline) => &underline.content_mask,
            Primitive::MonochromeSprite(sprite) => &sprite.content_mask,
            Primitive::PolychromeSprite(sprite) => &sprite.content_mask,
            Primitive::Surface(surface) => &surface.content_mask,
            Primitive::Backdrop(backdrop) => &backdrop.content_mask,
            Primitive::Visualizer(visualizer) => &visualizer.content_mask,
        }
    }
}

#[cfg_attr(
    all(
        any(target_os = "linux", target_os = "freebsd"),
        not(any(feature = "x11", feature = "wayland"))
    ),
    allow(dead_code)
)]
struct BatchIterator<'a> {
    shadows: &'a [Shadow],
    shadows_start: usize,
    shadows_iter: Peekable<slice::Iter<'a, Shadow>>,
    quads: &'a [Quad],
    quads_start: usize,
    quads_iter: Peekable<slice::Iter<'a, Quad>>,
    paths: &'a [Path<ScaledPixels>],
    paths_start: usize,
    paths_iter: Peekable<slice::Iter<'a, Path<ScaledPixels>>>,
    underlines: &'a [Underline],
    underlines_start: usize,
    underlines_iter: Peekable<slice::Iter<'a, Underline>>,
    monochrome_sprites: &'a [MonochromeSprite],
    monochrome_sprites_start: usize,
    monochrome_sprites_iter: Peekable<slice::Iter<'a, MonochromeSprite>>,
    polychrome_sprites: &'a [PolychromeSprite],
    polychrome_sprites_start: usize,
    polychrome_sprites_iter: Peekable<slice::Iter<'a, PolychromeSprite>>,
    surfaces: &'a [PaintSurface],
    surfaces_start: usize,
    surfaces_iter: Peekable<slice::Iter<'a, PaintSurface>>,
    backdrops: &'a [Backdrop],
    backdrops_start: usize,
    backdrops_iter: Peekable<slice::Iter<'a, Backdrop>>,
    visualizers: &'a [Visualizer],
    visualizers_start: usize,
    visualizers_iter: Peekable<slice::Iter<'a, Visualizer>>,
}

impl<'a> Iterator for BatchIterator<'a> {
    type Item = PrimitiveBatch<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut orders_and_kinds = [
            (
                self.shadows_iter.peek().map(|s| s.order),
                PrimitiveKind::Shadow,
            ),
            (self.quads_iter.peek().map(|q| q.order), PrimitiveKind::Quad),
            (self.paths_iter.peek().map(|q| q.order), PrimitiveKind::Path),
            (
                self.underlines_iter.peek().map(|u| u.order),
                PrimitiveKind::Underline,
            ),
            (
                self.monochrome_sprites_iter.peek().map(|s| s.order),
                PrimitiveKind::MonochromeSprite,
            ),
            (
                self.polychrome_sprites_iter.peek().map(|s| s.order),
                PrimitiveKind::PolychromeSprite,
            ),
            (
                self.surfaces_iter.peek().map(|s| s.order),
                PrimitiveKind::Surface,
            ),
            (
                self.backdrops_iter.peek().map(|b| b.order),
                PrimitiveKind::Backdrop,
            ),
            (
                self.visualizers_iter.peek().map(|v| v.order),
                PrimitiveKind::Visualizer,
            ),
        ];
        orders_and_kinds.sort_by_key(|(order, kind)| (order.unwrap_or(u32::MAX), *kind));

        let first = orders_and_kinds[0];
        let second = orders_and_kinds[1];
        let (batch_kind, max_order_and_kind) = if first.0.is_some() {
            (first.1, (second.0.unwrap_or(u32::MAX), second.1))
        } else {
            return None;
        };

        match batch_kind {
            PrimitiveKind::Shadow => {
                let shadows_start = self.shadows_start;
                let mut shadows_end = shadows_start + 1;
                self.shadows_iter.next();
                while self
                    .shadows_iter
                    .next_if(|shadow| (shadow.order, batch_kind) < max_order_and_kind)
                    .is_some()
                {
                    shadows_end += 1;
                }
                self.shadows_start = shadows_end;
                Some(PrimitiveBatch::Shadows(
                    &self.shadows[shadows_start..shadows_end],
                ))
            }
            PrimitiveKind::Quad => {
                let quads_start = self.quads_start;
                let mut quads_end = quads_start + 1;
                self.quads_iter.next();
                while self
                    .quads_iter
                    .next_if(|quad| (quad.order, batch_kind) < max_order_and_kind)
                    .is_some()
                {
                    quads_end += 1;
                }
                self.quads_start = quads_end;
                Some(PrimitiveBatch::Quads(&self.quads[quads_start..quads_end]))
            }
            PrimitiveKind::Path => {
                let paths_start = self.paths_start;
                let mut paths_end = paths_start + 1;
                self.paths_iter.next();
                while self
                    .paths_iter
                    .next_if(|path| (path.order, batch_kind) < max_order_and_kind)
                    .is_some()
                {
                    paths_end += 1;
                }
                self.paths_start = paths_end;
                Some(PrimitiveBatch::Paths(&self.paths[paths_start..paths_end]))
            }
            PrimitiveKind::Underline => {
                let underlines_start = self.underlines_start;
                let mut underlines_end = underlines_start + 1;
                self.underlines_iter.next();
                while self
                    .underlines_iter
                    .next_if(|underline| (underline.order, batch_kind) < max_order_and_kind)
                    .is_some()
                {
                    underlines_end += 1;
                }
                self.underlines_start = underlines_end;
                Some(PrimitiveBatch::Underlines(
                    &self.underlines[underlines_start..underlines_end],
                ))
            }
            PrimitiveKind::MonochromeSprite => {
                let texture_id = self.monochrome_sprites_iter.peek().unwrap().tile.texture_id;
                let sprites_start = self.monochrome_sprites_start;
                let mut sprites_end = sprites_start + 1;
                self.monochrome_sprites_iter.next();
                while self
                    .monochrome_sprites_iter
                    .next_if(|sprite| {
                        (sprite.order, batch_kind) < max_order_and_kind
                            && sprite.tile.texture_id == texture_id
                    })
                    .is_some()
                {
                    sprites_end += 1;
                }
                self.monochrome_sprites_start = sprites_end;
                Some(PrimitiveBatch::MonochromeSprites {
                    texture_id,
                    sprites: &self.monochrome_sprites[sprites_start..sprites_end],
                })
            }
            PrimitiveKind::PolychromeSprite => {
                let texture_id = self.polychrome_sprites_iter.peek().unwrap().tile.texture_id;
                let sprites_start = self.polychrome_sprites_start;
                let mut sprites_end = self.polychrome_sprites_start + 1;
                self.polychrome_sprites_iter.next();
                while self
                    .polychrome_sprites_iter
                    .next_if(|sprite| {
                        (sprite.order, batch_kind) < max_order_and_kind
                            && sprite.tile.texture_id == texture_id
                    })
                    .is_some()
                {
                    sprites_end += 1;
                }
                self.polychrome_sprites_start = sprites_end;
                Some(PrimitiveBatch::PolychromeSprites {
                    texture_id,
                    sprites: &self.polychrome_sprites[sprites_start..sprites_end],
                })
            }
            PrimitiveKind::Surface => {
                let surfaces_start = self.surfaces_start;
                let mut surfaces_end = surfaces_start + 1;
                self.surfaces_iter.next();
                while self
                    .surfaces_iter
                    .next_if(|surface| (surface.order, batch_kind) < max_order_and_kind)
                    .is_some()
                {
                    surfaces_end += 1;
                }
                self.surfaces_start = surfaces_end;
                Some(PrimitiveBatch::Surfaces(
                    &self.surfaces[surfaces_start..surfaces_end],
                ))
            }
            PrimitiveKind::Backdrop => {
                let backdrops_start = self.backdrops_start;
                let mut backdrops_end = backdrops_start + 1;
                self.backdrops_iter.next();
                while self
                    .backdrops_iter
                    .next_if(|backdrop| (backdrop.order, batch_kind) < max_order_and_kind)
                    .is_some()
                {
                    backdrops_end += 1;
                }
                self.backdrops_start = backdrops_end;
                Some(PrimitiveBatch::Backdrops(
                    &self.backdrops[backdrops_start..backdrops_end],
                ))
            }
            PrimitiveKind::Visualizer => {
                let visualizers_start = self.visualizers_start;
                let mut visualizers_end = visualizers_start + 1;
                self.visualizers_iter.next();
                while self
                    .visualizers_iter
                    .next_if(|visualizer| (visualizer.order, batch_kind) < max_order_and_kind)
                    .is_some()
                {
                    visualizers_end += 1;
                }
                self.visualizers_start = visualizers_end;
                Some(PrimitiveBatch::Visualizers(
                    &self.visualizers[visualizers_start..visualizers_end],
                ))
            }
        }
    }
}

#[derive(Debug)]
#[cfg_attr(
    all(
        any(target_os = "linux", target_os = "freebsd"),
        not(any(feature = "x11", feature = "wayland"))
    ),
    allow(dead_code)
)]
pub(crate) enum PrimitiveBatch<'a> {
    Shadows(&'a [Shadow]),
    Quads(&'a [Quad]),
    Paths(&'a [Path<ScaledPixels>]),
    Underlines(&'a [Underline]),
    MonochromeSprites {
        texture_id: AtlasTextureId,
        sprites: &'a [MonochromeSprite],
    },
    PolychromeSprites {
        texture_id: AtlasTextureId,
        sprites: &'a [PolychromeSprite],
    },
    Surfaces(&'a [PaintSurface]),
    /// Plays: see `Backdrop`.
    Backdrops(&'a [Backdrop]),
    /// Plays: see `Visualizer`.
    Visualizers(&'a [Visualizer]),
}

/// Plays: how an element's backdrop shows what is drawn under it, behind
/// its own background and children: blurred, its colors and brightness
/// changed, bent along the edge like glass, with a rim of light and a glow
/// of the colors around it (see `Styled::backdrop`). Renderers that can't
/// read back what they drew (Windows) leave it out.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BackdropStyle {
    /// How far what's under it is blurred, about the blur's radius. Even 0
    /// blurs it a little, as it is read at half the window's resolution.
    pub blur: Pixels,
    /// How colorful it is: 1 as it is, 0 gray, more to brighten colors.
    pub saturation: f32,
    /// What its brightness is multiplied by.
    pub brightness: f32,
    /// A color laid over it, as far as the color's alpha says.
    pub tint: Hsla,
    /// How far it is the average color around each point (from about the
    /// window's 1/64th) rather than what is right under it: 1 shows only
    /// that, a tint of the colors around.
    pub average: f32,
    /// How far, at the most, the edge bends what shows through it toward
    /// the outside, as the rim of a lens does.
    pub refraction: Pixels,
    /// How wide the band along the edge is that bends, catches the light
    /// and glows.
    pub edge: Pixels,
    /// How bright the rim of light along the edge is, brightest where it
    /// faces the top left or the bottom right.
    pub rim: f32,
    /// How strongly the colors around it glow inside its edge and color
    /// the rim.
    pub glow: f32,
    /// How far the glow spills outside the element.
    pub halo: Pixels,
    /// How far it covers what's under it.
    pub opacity: f32,
}

impl Default for BackdropStyle {
    fn default() -> Self {
        Self {
            blur: Pixels::ZERO,
            saturation: 1.,
            brightness: 1.,
            tint: Hsla::transparent_black(),
            average: 0.,
            refraction: Pixels::ZERO,
            edge: Pixels::ZERO,
            rim: 0.,
            glow: 0.,
            halo: Pixels::ZERO,
            opacity: 1.,
        }
    }
}

/// Plays: how many times what's under backdrops is halved, to 1/64th.
pub(crate) const BACKDROP_LEVELS: usize = 6;

/// Plays: an element's backdrop (see `BackdropStyle`). Renderers copy what
/// they drew so far around a batch of them, halve it again and again
/// (`BACKDROP_LEVELS`), and draw each from those.
#[derive(Debug, Clone)]
#[repr(C)]
pub(crate) struct Backdrop {
    pub order: DrawOrder,
    pub pad: u32,
    /// Where it draws: its shape, and the halo around it.
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    /// The element's shape.
    pub shape: Bounds<ScaledPixels>,
    pub corner_radii: Corners<ScaledPixels>,
    pub tint: Hsla,
    pub corner_smoothing: f32,
    pub blur: ScaledPixels,
    pub saturation: f32,
    pub brightness: f32,
    pub average: f32,
    pub refraction: ScaledPixels,
    pub edge: ScaledPixels,
    pub rim: f32,
    pub glow: f32,
    pub halo: ScaledPixels,
    pub opacity: f32,
    /// Keeps the size a multiple of 8, as WGSL lays the struct out.
    pub pad2: u32,
}

impl From<Backdrop> for Primitive {
    fn from(backdrop: Backdrop) -> Self {
        Primitive::Backdrop(backdrop)
    }
}

impl Backdrop {
    /// A backdrop for an element of `bounds` and `corner_radii`, inside
    /// `content_mask`, as `style` says and `opacity` (the element's) fades
    /// it, at `scale_factor`.
    pub(crate) fn new(
        bounds: Bounds<Pixels>,
        corner_radii: Corners<Pixels>,
        corner_smoothing: f32,
        style: BackdropStyle,
        content_mask: ContentMask<Pixels>,
        opacity: f32,
        scale_factor: f32,
    ) -> Self {
        let halo = style.halo.max(Pixels::ZERO);
        Self {
            order: 0,
            pad: 0,
            bounds: bounds.dilate(halo).scale(scale_factor),
            content_mask: content_mask.scale(scale_factor),
            shape: bounds.scale(scale_factor),
            corner_radii: corner_radii.scale(scale_factor),
            tint: style.tint,
            corner_smoothing,
            blur: style.blur.scale(scale_factor),
            saturation: style.saturation,
            brightness: style.brightness,
            average: style.average,
            refraction: style.refraction.scale(scale_factor),
            edge: style.edge.scale(scale_factor),
            rim: style.rim,
            glow: style.glow,
            halo: halo.scale(scale_factor),
            opacity: style.opacity * opacity,
            pad2: 0,
        }
    }

    /// How far around where it draws a backdrop reads what's under it: as
    /// far as its edge bends, and as far as the coarsest level it reads
    /// does. It blends that level's texels a texel and a half around, each
    /// made from the level before up to three of its texels around, and so
    /// on down to the frame (see `backdrop_level_region`).
    pub(crate) fn reach(&self) -> ScaledPixels {
        let last = BACKDROP_LEVELS - 1;
        let level = if self.average > 0. || self.glow > 0. {
            // The colors around.
            last
        } else {
            let lod = (self.blur.0.max(2.).log2() - 1.).clamp(0., last as f32);
            lod.ceil() as usize
        };
        let texel = (2u32 << level) as f32;
        ScaledPixels(self.refraction.0.max(0.) + 5.5 * texel)
    }

    /// Where a batch of backdrops draws, in device pixels, with room for
    /// how far their edges bend what shows through them. None if empty.
    #[cfg_attr(
        all(
            any(target_os = "linux", target_os = "freebsd"),
            not(any(feature = "x11", feature = "wayland"))
        ),
        allow(dead_code)
    )]
    pub(crate) fn region(backdrops: &[Backdrop]) -> Option<Bounds<ScaledPixels>> {
        backdrops
            .iter()
            .map(|backdrop| {
                backdrop
                    .bounds
                    .intersect(&backdrop.content_mask.bounds)
                    .dilate(ScaledPixels(backdrop.refraction.0.max(0.)))
            })
            .reduce(|region, bounds| region.union(&bounds))
            .filter(|region| !region.is_empty())
    }
}

/// Plays: the part of a `level` of what's under backdrops to make for a
/// batch drawing in `region` (see `Backdrop::region`), in the viewport's
/// pixels, inside the viewport of `width` by `height`. Backdrops read a
/// level a texel and a half around where they draw, and each level reads
/// the one before up to three of its texels around it, so a level reaches
/// as far out as the levels after it need, and a texel more.
#[cfg_attr(
    all(
        any(target_os = "linux", target_os = "freebsd"),
        not(any(feature = "x11", feature = "wayland"))
    ),
    allow(dead_code)
)]
pub(crate) fn backdrop_level_region(
    region: Bounds<ScaledPixels>,
    level: usize,
    width: f32,
    height: f32,
) -> [f32; 4] {
    let texel = (2u32 << level) as f32;
    let last_texel = (2u32 << (BACKDROP_LEVELS - 1)) as f32;
    // Three texels of each level from this one to the last, from 2^(level
    // + 1) to 2^BACKDROP_LEVELS, and one of this level.
    let margin = 3. * (2. * last_texel - texel) + texel;
    let left = (region.origin.x.0 - margin).floor().clamp(0., width);
    let top = (region.origin.y.0 - margin).floor().clamp(0., height);
    let right = (region.origin.x.0 + region.size.width.0 + margin)
        .ceil()
        .clamp(0., width);
    let bottom = (region.origin.y.0 + region.size.height.0 + margin)
        .ceil()
        .clamp(0., height);
    [left, top, right - left, bottom - top]
}

/// Plays: the size of a level of what's under backdrops, `level` from 0
/// (half the viewport) to `BACKDROP_LEVELS` - 1.
#[cfg_attr(
    all(
        any(target_os = "linux", target_os = "freebsd"),
        not(any(feature = "x11", feature = "wayland"))
    ),
    allow(dead_code)
)]
pub(crate) fn backdrop_level_size(width: u32, height: u32, level: usize) -> (u32, u32) {
    let texel = 2u32 << level;
    (width.div_ceil(texel).max(1), height.div_ceil(texel).max(1))
}

#[derive(Default, Debug, Clone)]
#[repr(C)]
pub(crate) struct Quad {
    pub order: DrawOrder,
    pub border_style: BorderStyle,
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    pub background: Background,
    pub border_color: Hsla,
    pub corner_radii: Corners<ScaledPixels>,
    pub border_widths: Edges<ScaledPixels>,
    /// Plays: how far rounded corners are superellipses rather than circular
    /// arcs, from 0 (circular). See `squircle_corner` in the shaders.
    pub corner_smoothing: f32,
    /// Plays: keeps the size a multiple of 8, as WGSL lays the struct out.
    pub pad: u32,
}

impl From<Quad> for Primitive {
    fn from(quad: Quad) -> Self {
        Primitive::Quad(quad)
    }
}

#[derive(Debug, Clone)]
#[repr(C)]
pub(crate) struct Underline {
    pub order: DrawOrder,
    pub pad: u32, // align to 8 bytes
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    pub color: Hsla,
    pub thickness: ScaledPixels,
    pub wavy: u32,
}

impl From<Underline> for Primitive {
    fn from(underline: Underline) -> Self {
        Primitive::Underline(underline)
    }
}

#[derive(Debug, Clone)]
#[repr(C)]
pub(crate) struct Shadow {
    pub order: DrawOrder,
    pub blur_radius: ScaledPixels,
    pub bounds: Bounds<ScaledPixels>,
    pub corner_radii: Corners<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    pub color: Hsla,
}

impl From<Shadow> for Primitive {
    fn from(shadow: Shadow) -> Self {
        Primitive::Shadow(shadow)
    }
}

/// Plays: which fragment shader draws a visualizer, and what it's fed (see
/// `Window::paint_visualizer`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum VisualizerShader {
    /// A speaker seen from the front, in the middle, as wide as fits: two
    /// thin rings, its basket's rim, a point wide, and inside it its cone's
    /// edge, two points wide, which moves out with `level`, from 0 to 1.
    Speaker {
        /// How loud it plays, from 0 to 1.
        level: f32,
    },
}

impl VisualizerShader {
    /// Its number in the shaders, and the parameters they read, drawn at
    /// `scale_factor` device pixels a point.
    pub(crate) fn encode(self, scale_factor: f32) -> (VisualizerKind, [f32; 4]) {
        match self {
            Self::Speaker { level } => (
                VisualizerKind::Speaker,
                [level.clamp(0., 1.), scale_factor, 0., 0.],
            ),
        }
    }

    /// The visualizer `kind` and `params` came from.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn decode(kind: VisualizerKind, params: [f32; 4]) -> Self {
        match kind {
            VisualizerKind::Speaker => Self::Speaker { level: params[0] },
        }
    }
}

/// Plays: which fragment shader draws a `Visualizer`, as the shaders number
/// them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(C)]
pub(crate) enum VisualizerKind {
    #[default]
    Speaker = 0,
}

/// Plays: a quad drawn by a fragment shader of its own, which `kind` names,
/// in `color`, fed `params` (see `VisualizerShader`), such as a sound's
/// level; drawn as is, under what's drawn on it.
#[derive(Clone, Debug, Default)]
#[repr(C)]
pub(crate) struct Visualizer {
    pub order: DrawOrder,
    pub kind: VisualizerKind,
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    pub color: Hsla,
    pub params: [f32; 4],
}

// WGSL puts `bounds` on an 8-byte boundary, and so its size on a multiple
// of 8: 72 bytes, as blade checks on Linux.
const _: () = assert!(std::mem::offset_of!(Visualizer, bounds) % 8 == 0);
const _: () = assert!(std::mem::size_of::<Visualizer>() == 72);

impl From<Visualizer> for Primitive {
    fn from(visualizer: Visualizer) -> Self {
        Primitive::Visualizer(visualizer)
    }
}

/// The style of a border.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[repr(C)]
pub enum BorderStyle {
    /// A solid border.
    #[default]
    Solid = 0,
    /// A dashed border.
    Dashed = 1,
}

/// A data type representing a 2 dimensional transformation that can be applied to an element.
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(C)]
pub struct TransformationMatrix {
    /// 2x2 matrix containing rotation and scale,
    /// stored row-major
    pub rotation_scale: [[f32; 2]; 2],
    /// translation vector
    pub translation: [f32; 2],
}

impl Eq for TransformationMatrix {}

impl TransformationMatrix {
    /// The unit matrix, has no effect.
    pub fn unit() -> Self {
        Self {
            rotation_scale: [[1.0, 0.0], [0.0, 1.0]],
            translation: [0.0, 0.0],
        }
    }

    /// Move the origin by a given point
    pub fn translate(mut self, point: Point<ScaledPixels>) -> Self {
        self.compose(Self {
            rotation_scale: [[1.0, 0.0], [0.0, 1.0]],
            translation: [point.x.0, point.y.0],
        })
    }

    /// Clockwise rotation in radians around the origin
    pub fn rotate(self, angle: Radians) -> Self {
        self.compose(Self {
            rotation_scale: [
                [angle.0.cos(), -angle.0.sin()],
                [angle.0.sin(), angle.0.cos()],
            ],
            translation: [0.0, 0.0],
        })
    }

    /// Scale around the origin
    pub fn scale(self, size: Size<f32>) -> Self {
        self.compose(Self {
            rotation_scale: [[size.width, 0.0], [0.0, size.height]],
            translation: [0.0, 0.0],
        })
    }

    /// Perform matrix multiplication with another transformation
    /// to produce a new transformation that is the result of
    /// applying both transformations: first, `other`, then `self`.
    #[inline]
    pub fn compose(self, other: TransformationMatrix) -> TransformationMatrix {
        if other == Self::unit() {
            return self;
        }
        // Perform matrix multiplication
        TransformationMatrix {
            rotation_scale: [
                [
                    self.rotation_scale[0][0] * other.rotation_scale[0][0]
                        + self.rotation_scale[0][1] * other.rotation_scale[1][0],
                    self.rotation_scale[0][0] * other.rotation_scale[0][1]
                        + self.rotation_scale[0][1] * other.rotation_scale[1][1],
                ],
                [
                    self.rotation_scale[1][0] * other.rotation_scale[0][0]
                        + self.rotation_scale[1][1] * other.rotation_scale[1][0],
                    self.rotation_scale[1][0] * other.rotation_scale[0][1]
                        + self.rotation_scale[1][1] * other.rotation_scale[1][1],
                ],
            ],
            translation: [
                self.translation[0]
                    + self.rotation_scale[0][0] * other.translation[0]
                    + self.rotation_scale[0][1] * other.translation[1],
                self.translation[1]
                    + self.rotation_scale[1][0] * other.translation[0]
                    + self.rotation_scale[1][1] * other.translation[1],
            ],
        }
    }

    /// Apply transformation to a point, mainly useful for debugging
    pub fn apply(&self, point: Point<Pixels>) -> Point<Pixels> {
        let input = [point.x.0, point.y.0];
        let mut output = self.translation;
        for (i, output_cell) in output.iter_mut().enumerate() {
            for (k, input_cell) in input.iter().enumerate() {
                *output_cell += self.rotation_scale[i][k] * *input_cell;
            }
        }
        Point::new(output[0].into(), output[1].into())
    }
}

impl Default for TransformationMatrix {
    fn default() -> Self {
        Self::unit()
    }
}

#[derive(Clone, Debug)]
#[repr(C)]
pub(crate) struct MonochromeSprite {
    pub order: DrawOrder,
    pub pad: u32, // align to 8 bytes
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    pub color: Hsla,
    pub tile: AtlasTile,
    pub transformation: TransformationMatrix,
}

impl From<MonochromeSprite> for Primitive {
    fn from(sprite: MonochromeSprite) -> Self {
        Primitive::MonochromeSprite(sprite)
    }
}

#[derive(Clone, Debug)]
#[repr(C)]
pub(crate) struct PolychromeSprite {
    pub order: DrawOrder,
    pub pad: u32, // align to 8 bytes
    pub grayscale: bool,
    pub opacity: f32,
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    pub corner_radii: Corners<ScaledPixels>,
    pub tile: AtlasTile,
    /// Plays: how far rounded corners are superellipses, as for `Quad`.
    pub corner_smoothing: f32,
    /// Plays: keeps the size a multiple of 8, as WGSL lays the struct out.
    pub smoothing_pad: u32,
    /// Plays: the part of the image shown, and turned or mirrored how.
    pub content: ContentTransform,
}

// Plays: WGSL puts `ContentTransform`'s vectors on 8-byte boundaries.
const _: () = assert!(std::mem::offset_of!(PolychromeSprite, content) % 8 == 0);
const _: () = assert!(std::mem::size_of::<PolychromeSprite>() % 8 == 0);

/// Plays: the part of an image or surface a sprite shows, and turned or
/// mirrored how, as where its texture is sampled: at the sprite's top left
/// corner, and how that moves across the sprite's width and down its height,
/// in fractions of the image. `IDENTITY` shows the whole image as it is;
/// another picks a part (cropping) or swaps and reverses the directions
/// (quarter turns and mirroring).
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
pub struct ContentTransform {
    /// Where the top left corner samples.
    pub origin: [f32; 2],
    /// What going from the left edge to the right one adds.
    pub across: [f32; 2],
    /// What going from the top edge to the bottom one adds.
    pub down: [f32; 2],
}

impl ContentTransform {
    /// The whole image, as it is.
    pub const IDENTITY: Self = Self {
        origin: [0., 0.],
        across: [1., 0.],
        down: [0., 1.],
    };
}

impl Default for ContentTransform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl From<PolychromeSprite> for Primitive {
    fn from(sprite: PolychromeSprite) -> Self {
        Primitive::PolychromeSprite(sprite)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct PaintSurface {
    pub order: DrawOrder,
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    #[cfg(target_os = "macos")]
    pub image_buffer: core_video::pixel_buffer::CVPixelBuffer,
    /// Plays: a video frame's planes, which the Linux renderer converts.
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    pub frame: crate::YuvFrame,
    /// Plays: the gamma the frame is drawn with (see PLAYS.md).
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    pub gamma: f32,
    /// Plays: the part of the surface shown, and turned or mirrored how.
    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "freebsd"))]
    pub content: ContentTransform,
}

impl From<PaintSurface> for Primitive {
    fn from(surface: PaintSurface) -> Self {
        Primitive::Surface(surface)
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct PathId(pub(crate) usize);

/// A line made up of a series of vertices and control points.
#[derive(Clone, Debug)]
pub struct Path<P: Clone + Debug + Default + PartialEq> {
    pub(crate) id: PathId,
    pub(crate) order: DrawOrder,
    pub(crate) bounds: Bounds<P>,
    pub(crate) content_mask: ContentMask<P>,
    pub(crate) vertices: Vec<PathVertex<P>>,
    pub(crate) color: Background,
    start: Point<P>,
    current: Point<P>,
    contour_count: usize,
}

impl Path<Pixels> {
    /// Create a new path with the given starting point.
    pub fn new(start: Point<Pixels>) -> Self {
        Self {
            id: PathId(0),
            order: DrawOrder::default(),
            vertices: Vec::new(),
            start,
            current: start,
            bounds: Bounds {
                origin: start,
                size: Default::default(),
            },
            content_mask: Default::default(),
            color: Default::default(),
            contour_count: 0,
        }
    }

    /// Scale this path by the given factor.
    pub fn scale(&self, factor: f32) -> Path<ScaledPixels> {
        Path {
            id: self.id,
            order: self.order,
            bounds: self.bounds.scale(factor),
            content_mask: self.content_mask.scale(factor),
            vertices: self
                .vertices
                .iter()
                .map(|vertex| vertex.scale(factor))
                .collect(),
            start: self.start.map(|start| start.scale(factor)),
            current: self.current.scale(factor),
            contour_count: self.contour_count,
            color: self.color,
        }
    }

    /// Move the start, current point to the given point.
    pub fn move_to(&mut self, to: Point<Pixels>) {
        self.contour_count += 1;
        self.start = to;
        self.current = to;
    }

    /// Draw a straight line from the current point to the given point.
    pub fn line_to(&mut self, to: Point<Pixels>) {
        self.contour_count += 1;
        if self.contour_count > 1 {
            self.push_triangle(
                (self.start, self.current, to),
                (point(0., 1.), point(0., 1.), point(0., 1.)),
            );
        }
        self.current = to;
    }

    /// Draw a curve from the current point to the given point, using the given control point.
    pub fn curve_to(&mut self, to: Point<Pixels>, ctrl: Point<Pixels>) {
        self.contour_count += 1;
        if self.contour_count > 1 {
            self.push_triangle(
                (self.start, self.current, to),
                (point(0., 1.), point(0., 1.), point(0., 1.)),
            );
        }

        self.push_triangle(
            (self.current, ctrl, to),
            (point(0., 0.), point(0.5, 0.), point(1., 1.)),
        );
        self.current = to;
    }

    /// Push a triangle to the Path.
    pub fn push_triangle(
        &mut self,
        xy: (Point<Pixels>, Point<Pixels>, Point<Pixels>),
        st: (Point<f32>, Point<f32>, Point<f32>),
    ) {
        self.bounds = self
            .bounds
            .union(&Bounds {
                origin: xy.0,
                size: Default::default(),
            })
            .union(&Bounds {
                origin: xy.1,
                size: Default::default(),
            })
            .union(&Bounds {
                origin: xy.2,
                size: Default::default(),
            });

        self.vertices.push(PathVertex {
            xy_position: xy.0,
            st_position: st.0,
            content_mask: Default::default(),
        });
        self.vertices.push(PathVertex {
            xy_position: xy.1,
            st_position: st.1,
            content_mask: Default::default(),
        });
        self.vertices.push(PathVertex {
            xy_position: xy.2,
            st_position: st.2,
            content_mask: Default::default(),
        });
    }
}

impl<T> Path<T>
where
    T: Clone + Debug + Default + PartialEq + PartialOrd + Add<T, Output = T> + Sub<Output = T>,
{
    #[allow(unused)]
    pub(crate) fn clipped_bounds(&self) -> Bounds<T> {
        self.bounds.intersect(&self.content_mask.bounds)
    }
}

impl From<Path<ScaledPixels>> for Primitive {
    fn from(path: Path<ScaledPixels>) -> Self {
        Primitive::Path(path)
    }
}

#[derive(Clone, Debug)]
#[repr(C)]
pub(crate) struct PathVertex<P: Clone + Debug + Default + PartialEq> {
    pub(crate) xy_position: Point<P>,
    pub(crate) st_position: Point<f32>,
    pub(crate) content_mask: ContentMask<P>,
}

impl PathVertex<Pixels> {
    pub fn scale(&self, factor: f32) -> PathVertex<ScaledPixels> {
        PathVertex {
            xy_position: self.xy_position.scale(factor),
            st_position: self.st_position,
            content_mask: self.content_mask.scale(factor),
        }
    }
}
