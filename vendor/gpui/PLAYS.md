# gpui 0.2.2, patched for Plays

This is gpui 0.2.2 from crates.io, without its examples, with HDR video on
macOS, video frames converted on the GPU on Linux, turned and cropped
pictures, backdrops, visualizers, and a few small fixes. Plays uses it through `[patch.crates-io]` in its
Cargo.toml. Everything Plays changed is marked "Plays" in the code;
`script/gpui-fork diff` shows it all, as a patch to crates.io's.

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

Visualizers (`src/scene.rs`, `src/window.rs`, `build.rs`, both renderers
and their shaders; Windows draws none):

- `Window::paint_visualizer(bounds, VisualizerShader, color)` paints a
  shader drawn whole on the GPU, as a `Visualizer` primitive: its kind,
  bounds, content mask, color and four parameters (`params`), which
  `VisualizerShader::encode` fills, with the window's scale factor.
  cbindgen writes the Metal header's
  copy; asserts in `scene.rs` keep `bounds` on an 8-byte boundary and its
  size 72 bytes, as WGSL lays it out.
- `VisualizerShader::Speaker { level }`: a loudspeaker seen from the
  front, as large as fits in the middle of its bounds: two thin rings in
  the color, blurred over a point, which move out with `level` (0 to 1):
  the basket's rim, a point wide, and inside it the cone's edge, two
  points wide, which moves twice as far.
- `fs_visualizer` (WGSL) and `visualizer_fragment` (Metal) pick the shader
  by kind; the color's alpha is the rings' coverage times its own.
- Test support: `Window::painted_visualizers` lists the visualizers the
  last frame drew, and `gpui::render_offscreen` draws them too, over its
  quads.

`over()` in the quad shaders (Metal, WGSL and HLSL) returns clear when both
colors are clear. It divided by their combined alpha, so the corners of a
fully see-through rounded quad (any rounded element drawn at opacity 0, for
example while it fades in) came out 0 / 0. The NaN stayed in the
`RGBA16Float` drawable and showed as dashed arcs and marks.

`src/text_system/line.rs`: a line of text paints in a layer as wide as
the width it is aligned in, not just as wide as the text. Centered text
(Plays' subtitles) used to fall outside its layer, so a popover drawn over
it later could end up beneath it.

`src/elements/text.rs`: text is measured again for another width to
truncate to, as it is for another width to wrap to, and again whole once
no width is known (`TextLayoutInner` keeps `truncate_width`). Text that
doesn't wrap, such as a `truncate()`d label, was first measured whole with
no width known, and kept that size and its whole line once its width was
known, so a label in a row too narrow for it was clipped where the row
ended, without its "…". Its runs are cut short on a copy each time, as
runs cut for one width don't fit the text cut for another.

`src/text_system/line_layout.rs`: runs that don't cover a line exactly,
or end inside a character, are fitted to it, with an error logged, before
the platform's text system lays it out (`fit_runs`): it slices the text by
them, and panicked.

`src/text_system/line_wrapper.rs`: `truncate_line` cuts a line only where
a cluster of characters starts (`unicode-segmentation`, a dependency now,
not a dev-dependency), as long as fits with its suffix as drawn. It added
up each character's width alone, which cut joined letters (Arabic) and
emoji made of several too early, and could cut a letter from its mark or
a family emoji to one person. The line is laid out whole once, each
cluster as wide as its glyphs; the cut so found is laid out with its suffix
and moved back while it doesn't fit, or on while the next does. A wrapper
keeps these widths for up to 1024 lines (`Cuts`), so a line is laid out
again only for a cut not yet tried.
`Window::painted_text` (test builds only, `src/window.rs`) lists each line
of text the window drew last, as it reads once truncated, and where, so
a test can tell.

Text drawn into memory, for Plays' screenshots with subtitles:
`WrappedLine::glyphs` (`src/text_system/line.rs`) lists the glyphs `paint`
would draw, where, and in what color, the same way `paint_line` places
them; `TextSystem::rasterize_line_glyph` (`src/text_system.rs`) rasterizes
one at a scale factor as `Window::paint_glyph` or `paint_emoji` would, into
a `GlyphRaster` of coverage (or BGRA for emoji) and where it goes.

`TextSystem::font_id` (`src/text_system.rs`) is public, so Plays can tell
whether a font a subtitle file names is installed: it fails for one that
isn't, where `resolve_font` falls back to another.

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

`src/app.rs` and `src/platform/app_menu.rs`: the app can say whether menu
items are enabled (`App::validate_menu_actions_with`), which
`App::is_menu_action_enabled` asks first. Plays' keys and menus all
dispatch one action, which carries the command to run, and a globally
handled action is otherwise always available; the validator asks Plays'
API whether that command can run now.

`src/keymap/context.rs`: a binding's context that holds of an empty
context, such as `!Recording`, holds in a window that sets no context, as
a binding without one does; before, any context failed there. Plays binds
the keys that work anywhere so (but while Settings records keys), and an
empty player window sets none.

`src/platform/test/window.rs`: a test window's `window_handle` returns
`HandleError::Unavailable` instead of panicking, so Plays' views, whose
native helpers give up on errors, can run in `#[gpui::test]`s. Its
`zoom` zooms it or puts it back, nothing moved, and `is_maximized` says
which, so a test can tell what a zoomed window does.
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
another display, after the main one, and `remove_display` disconnects it;
`displays()` lists them all, and a window opens on the one its options ask
for (`src/platform/test/display.rs` too).
`src/interactive.rs`: `ExternalPaths::for_test` (test builds only) makes
the files the system drags in, whose paths are gpui's own to set, so a
test can drop them on a window with `FileDropEvent`, as the system does.

`src/platform.rs`, `src/window.rs`, and the macOS, X11 and test windows:
`Window::set_on_top` keeps a window above other apps' windows, or puts it
back among them, and `Window::is_on_top` says whether it is there now. On
macOS it sets the window's level to floating, and the window delegate
drops it to normal while the window is full screen and raises it again
after. macOS leaves a window above the normal level out of Mission
Control and ⌘`'s round of the app's windows, and won't let it go full
screen, unless it asks, so a floating window asks for all three in its
collection behavior (managed, taking part in the round, and, resizable,
full screen primary); `Window::is_in_mission_control` says whether it
has them, by its level and collection behavior (always elsewhere). On X11 it asks the window manager for `_NET_WM_STATE_ABOVE` and
reads it back from `_NET_WM_STATE`. The test window keeps the flag;
Wayland has no protocol for it, so there it does nothing.

`src/platform.rs`, `src/window.rs` and the X11 window:
`WindowOptions::keep_position` opens a window where its bounds say and
asks the window manager to keep it there: on X11 the window's size hints
carry its position as the user's (`USPosition`), which window managers
keep rather than place the window themselves, with static gravity, so the
position is the window's own inside their frame, as gpui reports it; and
the window opens there without the 2 px nudge to the right gpui gives
other windows. The X11 window asks the server where it is on the screen
whenever it is moved or resized: a window manager's frame around it makes
the position in the events relative to the frame, so windows reported
where they are inside it, and opened there again, crept down and right. macOS always opens windows
where they are asked to; Wayland lets the compositor place them.

`src/platform.rs`, `src/window.rs`, and the macOS and test windows:
`Window::set_fullscreen_style` picks how `toggle_fullscreen` goes full
screen on macOS. `FullscreenStyle::OwnSpace` is macOS's own full screen, in
a Space of its own. With `FullscreenStyle::OverDisplay` the window covers
its display's whole frame instead, in the Space it is in, at once. While
it does, it can't be moved or resized, has no shadow or traffic lights,
and stays at the normal level. While it is key, the app's presentation
options auto-hide the menu bar and Dock; the window delegate restores them
when it isn't key. Going back restores the window's frame, style, shadow,
level and the app's presentation options. `is_fullscreen` is true either
way, and `window_bounds` gives the bounds from before. One full screen
already leaves the way it came. A window covering a display that goes
away, which macOS moves to another, covers that one instead; going back,
a frame on no screen any more is centered on the window's screen, as much
of it as fits. The test window keeps the style.

`src/platform.rs`, `src/window.rs`, and the X11, Wayland and test
platforms: on X11 a display is a RandR monitor rather than an X screen,
which spans every monitor. Its id is the atom of the monitor's name, its
bounds are where it is on the screen (as windows' are), and a window's
display is the monitor with its middle; the screen as a whole is a display
only where RandR lists no monitors. A window asked to open on a monitor
opens on the main screen. `PlatformDisplay::name` gives a display's name:
on X11 the product name in the EDID of the monitor's first output, or else
the monitor's own (its connector's), with the connector added where two
have the same name; on Wayland the output's description, with its
connector's name unless it has it; `None` elsewhere.
`Window::fullscreen_on(display)` goes full screen on a display: X11 moves
the window there first, its middle to the monitor's, as window managers
put a window full screen on the monitor it is on; Wayland passes the
output to `xdg_toplevel.set_fullscreen`, which the compositor may ignore;
macOS, and any other platform, go full screen where the window is. The
test window moves to the display, and tells of the move once the update
asking for it is done, as a system would. `Window::set_frame(bounds)`
moves and sizes a window: on X11 with `_NET_MOVERESIZE_WINDOW` and static
gravity, so the bounds are the window's own inside its frame; the test
window takes the bounds; elsewhere it does nothing. The X11 window keeps
where it was when asked to go full screen, and `window_bounds` gives that
as `WindowBounds::Fullscreen` until it is out again, as window managers
say a window is full screen only after they have moved it. The Wayland
client forgets an output once the compositor removes its global, and
releases it, so a display unplugged leaves `displays()` too.

`src/platform/linux/wayland/window.rs`: a window leaving full screen takes
back the size it had before when the compositor gives it no size: Weston
gives none (0×0) after `unset_fullscreen`, leaving the size to the window,
and it stayed as big as the output. Given a size, it takes that, as from
any configure. Leaving maximized, it already took back its size from
before, whatever it was given. `floating_size` decides, and its tests are
gpui's own, which cargo doesn't run, gpui being outside Plays' workspace.

The macOS and test windows: `Window::start_window_move` moves a macOS
window with the mouse while its button is down, as its title bar does
(`performWindowDragWithEvent` with the event being handled, once it is
done); the test window has nowhere to move, where it panicked.

`src/platform/mac/metal_renderer.rs` and `src/platform/mac/window.rs`: the
renderer asks the layer for drawables on a thread of its own, and a frame
waits for one at most 50 ms. With every drawable holding a frame the
screen hasn't taken, `nextDrawable` blocks until it takes one: up to a
second for a window just gone under another, before macOS says it is
covered and gpui stops drawing it, as Plays' window does when it pauses
and floats only while playing. A frame with no drawable in time is left
out, as are those after it until one comes, and the window's next step
presents it again (`require_presentation`). A drawable that comes after
its frame was left out is let go and asked for again, as it may be for
another size. Its tests are gpui's own, which cargo doesn't run.

`src/window.rs` and `src/app.rs`: a platform window can still call once
its window has closed (macOS closes its own on the run loop's next turn).
The window's callbacks then do nothing (`update_from_platform`) rather than
log "window not found", and `App::dispatch_action`, as a menu does, takes a
closed window in front as none.

`Cargo.toml`: stacksafe 1.0, not 0.1, whose macros used proc-macro-error2,
which a future Rust rejects.

`src/taffy.rs`: two float literals are typed as `f32`, which newer Rust
warns about otherwise.

The tone mapping matches Plays' `src/media/hdr.rs`, and the Dolby Vision
reshaping its `src/media/dovi.rs`, which do the same on the CPU for software
decoding on macOS, and for Dolby Vision on Linux.

## Updating gpui

`script/gpui-fork upgrade <version>` merges a release from crates.io into
this folder, three ways from the one it's patched from, and names the files
it left conflicts in.
