# gpui 0.2.2, patched for Plays

This is gpui 0.2.2 from crates.io, without its examples, with HDR video on
macOS and a few small fixes. Plays uses it through `[patch.crates-io]` in its
Cargo.toml. Everything Plays changed is marked "Plays" in the code.

## What changed

`src/platform/mac/metal_renderer.rs` and `src/platform/mac/shaders.metal`:

- The window's `CAMetalLayer` draws in `RGBA16Float` in the extended sRGB
  color space, instead of `BGRA8Unorm`. Values from 0 to 1 mean the same as
  before, so everything gpui draws looks exactly as it did; only HDR video
  goes above 1. Path rasterization still uses its own 8-bit texture.
- `surface()` also takes 10-bit biplanar YCbCr pixel buffers
  (`kCVPixelFormatType_420YpCbCr10BiPlanarVideoRange` and `...FullRange`).
  Those are drawn by a new `surface_hdr_fragment` shader, which reads the
  buffer's color tags (YCbCr matrix, primaries, transfer function), decodes
  PQ or HLG to light, converts BT.2020 or P3 to sRGB primaries, and tone maps
  it under the screen's current EDR headroom (`NSScreen
  maximumExtendedDynamicRangeColorComponentValue`), read every frame.
  HDR reference white (203 nits) lands on SDR white. 8-bit surfaces are drawn
  as before.
- 10-bit surfaces that carry a `PlaysDolbyVision` attachment (a `CFData`
  that Plays sets) are Dolby Vision profile 5, drawn by a third shader,
  `surface_dovi_fragment`. The attachment holds the frame's reshaping curves
  and matrices exactly as the shader's `DoviParams`, passed with
  `set_fragment_bytes`. The shader turns the samples into BT.2020 light with
  them, then tone maps like `surface_hdr_fragment`.
- Every surface is drawn with the gamma in its `PlaysGamma` attachment (a
  `CFNumber`, 1 without it), which Plays sets as it draws. It bends the
  encoded signal between black and SDR white, `signal^(1/gamma)`; for HDR,
  tone mapping leaves room so highlights still end under the headroom.
- The layer asks for EDR (`wantsExtendedDynamicRangeContent`) only while HDR
  surfaces are on screen, and for 120 frames after, since EDR costs power.

Superellipse corners (`src/styled.rs`, `src/style.rs`, `src/window.rs`,
`src/scene.rs`, `src/elements/img.rs`, and the quad and image shaders in
`shaders.metal` and `shaders.wgsl`):

- `.superellipse(radius)` rounds corners like `.rounded(radius)`, but as
  superellipses (squircles): the curve starts with no curvature and spreads
  along more of each side, so there is no kink where it meets them, like
  Apple's continuous corners. It sets the radii and `Style::corner_smoothing`
  to 0.6; `PaintQuad::superellipse` (or `corner_smoothing` for another
  amount) does the same for a quad painted directly.
- `Quad` carries `corner_smoothing`, and `pad` so its size stays a multiple
  of 8 as WGSL lays it out. In the quad shaders, `squircle_corner` turns a
  radius and smoothing into the corner's extent and exponent, less when the
  quad has no room, so a corner as round as it allows (a pill) stays
  circular. `squircle_sdf` is its distance field, divided by its gradient so
  antialiasing and borders are even. Quads without smoothing, and so
  everything `.rounded`, take the old code and are drawn exactly as before.
- Images too: `PolychromeSprite` carries the smoothing (and a pad), `img`
  passes its style's to the new `Window::paint_image_with_smoothing`, and
  the sprite shader cuts the corners with `quad_sdf_smoothed`, which is
  `quad_sdf` without smoothing.
- Not smoothed: shadows, the layout of dashes on dashed borders, and
  Windows, whose shaders only have the new fields.

`over()` in the quad shaders (Metal, WGSL and HLSL) returns clear when both
colors are clear. It divided by their combined alpha, so the corners of a
fully see-through rounded quad (any rounded element drawn at opacity 0, for
example while it fades in) came out 0 / 0. The NaN stayed in the
`RGBA16Float` drawable and showed as dashed arcs and marks.

`src/text_system/line.rs`: a line of text paints in a layer as wide as
the width it is aligned in, not just as wide as the text. Centered text
(Plays' subtitles) used to fall outside its layer, so a popover drawn over
it later could end up beneath it.

`src/taffy.rs`: two float literals are typed as `f32`, which newer Rust
warns about otherwise.

The tone mapping matches Plays' `src/media/hdr.rs`, and the Dolby Vision
reshaping its `src/media/dovi.rs`, which do the same on the CPU for software
decoding and Linux.

## Updating gpui

Copy the new version from crates.io over this folder (leaving out
`examples/` and their `[[example]]` entries in Cargo.toml), then apply the
changes above again.
