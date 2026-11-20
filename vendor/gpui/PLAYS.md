# gpui 0.2.2, patched for Plays

This is gpui 0.2.2 from crates.io, without its examples, with HDR video on
macOS, video frames converted on the GPU on Linux, turned and cropped
pictures, backdrops, and a few small fixes. Plays uses it through `[patch.crates-io]` in its
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

Turned, mirrored and cropped pictures (`src/scene.rs`, `src/window.rs`,
`build.rs`, the sprite and surface vertex shaders in `shaders.metal`, and
the sprite ones in `shaders.wgsl` and `shaders.hlsl`):

- `ContentTransform` says how an image or surface is sampled where it's
  drawn: at the top left corner (`origin`), and what going across and down
  adds (`across`, `down`), in fractions of the image. `IDENTITY` shows the
  whole image as it is; others pick a part of it (cropping), or swap and
  reverse the directions (quarter turns and mirroring).
- `PolychromeSprite` and `PaintSurface` (and Metal's `SurfaceBounds`) carry
  one. `Window::paint_image_with_content` and
  `Window::paint_surface_with_content` take it, and everything else paints
  with `IDENTITY`. The vertex shaders run each corner of the unit square
  through it (`content_position`) before finding the atlas tile's or the
  surface's texture coordinates, so it costs nothing per pixel.
- cbindgen writes the Metal header's copy (`build.rs` lists the type). WGSL
  puts its vectors on 8-byte boundaries, so asserts in `scene.rs` keep
  `PolychromeSprite`'s `content` on one and its size a multiple of 8.

Backdrops (`src/scene.rs`, `src/style.rs`, `src/styled.rs`,
`src/window.rs`, `build.rs`, both renderers and their shaders; Windows
draws none):

- `.backdrop(BackdropStyle)` (`Style::backdrop`) draws, under an element's
  background and in its shape, what was drawn under it: blurred `blur`
  pixels, with `saturation` and `brightness`, a `tint` mixed in by its
  alpha, and `average` of the colors around in place of it. Along its edge
  a bevel `edge` wide bends what shows through it by up to `refraction`,
  catches a `rim` of light, and with `glow` glows with the colors around,
  inside and `halo` pixels outside. `opacity` fades it, as does the
  element's. `Window::paint_backdrop` paints one directly, as a `Backdrop`
  primitive (cbindgen writes the Metal header's copy; `pad2` keeps its size
  a multiple of 8, as WGSL lays it out).
- A backdrop's draw order is above everything drawn before it as far
  around as it reads (`Backdrop::reach`), not just under it, so all it
  shows is drawn first. Within one order, as in a layer, backdrops go
  first, under what's drawn on them.
- For each batch of backdrops the render pass ends. What's drawn so far is
  copied (Metal's layer is no longer `framebufferOnly`, and blade's surface
  is made with `COPY` usage too) and halved six times into `Rgba16Float`
  levels, each from the one before with the 13 taps of Jimenez's "Next
  generation post processing in Call of Duty: Advanced Warfare", only as
  far around the batch as later levels and the backdrops read
  (`Backdrop::region`, `backdrop_level_region`). The pass then goes on,
  loading what's drawn, and each backdrop is drawn from the levels: its
  blur mixes the two nearest, read through a tent filter, or the copy
  itself under 2 pixels; the last level gives the colors around. The
  textures are made when a frame first has backdrops, and again for a new
  size.
- The bevel's slope comes from the shape's distance field, and
  `backdrop_bend` is Snell's law through a quarter-circle bevel seen from
  above: what's just inside the edge shows magnified across it. A bevel
  that bends is polished, read from the sharp copy whatever the blur.
- Test support: `Window::painted_backdrops` lists the backdrops the last
  frame drew, and `gpui::render_offscreen` (macOS) draws quads and
  backdrops off screen with the Metal renderer and reads the pixels back.

`over()` in the quad shaders (Metal, WGSL and HLSL) returns clear when both
colors are clear. It divided by their combined alpha, so the corners of a
fully see-through rounded quad (any rounded element drawn at opacity 0, for
example while it fades in) came out 0 / 0. The NaN stayed in the
`RGBA16Float` drawable and showed as dashed arcs and marks.

`src/text_system/line.rs`: a line of text paints in a layer as wide as
the width it is aligned in, not just as wide as the text. Centered text
(Plays' subtitles) used to fall outside its layer, so a popover drawn over
it later could end up beneath it.

Text drawn into memory, for Plays' screenshots with subtitles:
`WrappedLine::glyphs` (`src/text_system/line.rs`) lists the glyphs `paint`
would draw, where, and in what color, the same way `paint_line` places
them; `TextSystem::rasterize_line_glyph` (`src/text_system.rs`) rasterizes
one at a scale factor as `Window::paint_glyph` or `paint_emoji` would, into
a `GlyphRaster` of coverage (or BGRA for emoji) and where it goes.

Video frames as Y'CbCr planes on Linux (`src/yuv_frame.rs`, `src/scene.rs`,
`src/window.rs`, `src/platform/blade/blade_yuv.rs`, `blade_renderer.rs`
and `shaders.wgsl`):

- A `YuvFrame` holds a frame's 4:2:0 planes as the decoder left them: Y,
  then Cb and Cr apart (`yuv420p`) or interleaved (`nv12`, `p010`), 8 or
  16 bits a sample, tightly packed, with a `YuvColor`: the matrix from
  samples to R'G'B' (the YCbCr matrix, range and where 10 bits sit in 16),
  the one from the video's primaries to sRGB's, the transfer (SDR, PQ or
  HLG), and where its chroma samples sit. Each frame gets an id.
- `Window::paint_yuv_frame` paints one, with a gamma and a
  `ContentTransform`; on Linux `PaintSurface` carries the frame and gamma
  instead of a `CVPixelBuffer`.
- Before the frame's render pass, the blade renderer copies each frame not
  yet on the GPU into textures of its own (`YuvTextures`), once per frame
  id, from an upload belt in a transfer pass. A frame not drawn in a frame
  gives its textures back once the GPU is done with it, to be reused for
  the next frames of the same size; spares beyond those in use (or 12) are
  destroyed.
- blade has no 16-bit unorm formats, so 16-bit samples go up as their two
  bytes in `Rg8Unorm` (or `Rgba8Unorm` for interleaved Cb and Cr), and
  `fs_yuv` filters them bilinearly itself, putting the bytes back together
  first. 8-bit ones use the sampler.
- `fs_yuv` samples chroma where the frame says its samples sit (in line
  with the even columns for most video, rather than between them), applies
  the matrix, then for HDR decodes PQ or HLG to light, converts BT.2020 to
  sRGB primaries, tone maps it to SDR with reference white (203 nits) on
  SDR white, and encodes it as sRGB, with the same math as Plays'
  `src/media/hdr.rs`. Last comes the gamma, `signal^(1/gamma)`. The
  swapchain isn't sRGB, so the encoded values are written as they are.

`src/platform/app_menu.rs` and `src/platform/mac/platform.rs`: a menu
item for an action can be `checked(true)`, which macOS shows with a check
mark (`setState:`), for the output device picked in the Audio menu. Other
platforms keep the flag and show nothing.

`src/platform/test/window.rs`: a test window's `window_handle` returns
`HandleError::Unavailable` instead of panicking, so Plays' views, whose
native helpers give up on errors, can run in `#[gpui::test]`s.
`src/window.rs`: `Frame::clear` clears `debug_bounds` (test builds only),
so they hold what the last frame drew, and a test can tell an element is
gone rather than finding where it was.
`src/app/test_context.rs` and `src/platform/test/platform.rs`:
`TestAppContext::with_native_text()` builds a test context whose text is
shaped and rasterized by the system's own text system (Core Text on macOS,
cosmic-text on Linux) rather than the no-op one, so Plays' tests can check
subtitles as they are drawn into screenshots. `TestPlatform::with_text_system`
takes the text system, and `TestPlatform::new` passes the no-op one as
before. The test platform counts `activate` calls, and
`TestAppContext::app_activations()` reads the count, so a test can tell the
app was brought forward. `TestAppContext::add_display` connects
another display, after the main one; `displays()` lists them all, and a
window opens on the one its options ask for (`src/platform/test/display.rs`
too).

`src/platform.rs`, `src/window.rs`, and the macOS, X11 and test windows:
`Window::set_on_top` keeps a window above other apps' windows, or puts it
back among them, and `Window::is_on_top` says whether it is there now. On
macOS it sets the window's level to floating, and the window delegate
drops it to normal while the window is full screen and raises it again
after. On X11 it asks the window manager for `_NET_WM_STATE_ABOVE` and
reads it back from `_NET_WM_STATE`. The test window keeps the flag;
Wayland has no protocol for it, so there it does nothing.

`src/taffy.rs`: two float literals are typed as `f32`, which newer Rust
warns about otherwise.

The tone mapping matches Plays' `src/media/hdr.rs`, and the Dolby Vision
reshaping its `src/media/dovi.rs`, which do the same on the CPU for software
decoding on macOS, and for Dolby Vision on Linux.

## Updating gpui

Copy the new version from crates.io over this folder (leaving out
`examples/` and their `[[example]]` entries in Cargo.toml), then apply the
changes above again.
