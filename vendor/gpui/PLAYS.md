# gpui 0.2.2, patched for Plays

This is gpui 0.2.2 from crates.io, without its examples, with one change:
HDR video on macOS. Plays uses it through `[patch.crates-io]` in its
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

`src/taffy.rs`: two float literals are typed as `f32`, which newer Rust
warns about otherwise.

The tone mapping matches Plays' `src/media/hdr.rs`, and the Dolby Vision
reshaping its `src/media/dovi.rs`, which do the same on the CPU for software
decoding and Linux.

## Updating gpui

Copy the new version from crates.io over this folder (leaving out
`examples/` and their `[[example]]` entries in Cargo.toml), then apply the
changes above again.
