//! Plays: video frames' planes in textures of their own (see PLAYS.md),
//! uploaded once for each frame while it is on screen, and the parameters
//! `fs_yuv` converts them with.

use crate::{PaintSurface, YuvFrame, YuvLayout, YuvTransfer};
use blade_graphics as gpu;
use blade_util::{BufferBelt, BufferBeltDescriptor};
use bytemuck::{Pod, Zeroable};
use collections::FxHashMap;
use std::sync::Arc;

use super::blade_renderer::PodBounds;

/// Matches `YuvParams` in shaders.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(super) struct YuvParams {
    bounds: PodBounds,
    content_mask: PodBounds,
    content_origin_across: [f32; 4],
    content_down: [f32; 4],
    ycbcr_to_rgb: [[f32; 4]; 4],
    to_srgb_primaries: [[f32; 4]; 4],
    transfer: u32,
    gamma: f32,
    planar: u32,
    wide: u32,
    chroma_siting: [f32; 4],
}

impl YuvParams {
    pub(super) fn new(surface: &PaintSurface) -> Self {
        let frame = &surface.frame;
        let color = frame.color();
        let content = surface.content;
        Self {
            bounds: surface.bounds.into(),
            content_mask: surface.content_mask.bounds.into(),
            content_origin_across: [
                content.origin[0],
                content.origin[1],
                content.across[0],
                content.across[1],
            ],
            content_down: [content.down[0], content.down[1], 0., 0.],
            ycbcr_to_rgb: color.ycbcr_to_rgb,
            to_srgb_primaries: color.to_srgb_primaries,
            transfer: match color.transfer {
                YuvTransfer::Sdr => 0,
                YuvTransfer::Pq => 1,
                YuvTransfer::Hlg => 2,
            },
            gamma: surface.gamma,
            planar: (frame.layout() == YuvLayout::Planar) as u32,
            wide: frame.is_wide() as u32,
            chroma_siting: [color.chroma_siting[0], color.chroma_siting[1], 0., 0.],
        }
    }
}

/// A texture for one plane.
struct PlaneTexture {
    texture: gpu::Texture,
    view: gpu::TextureView,
    format: gpu::TextureFormat,
    size: (u32, u32),
}

/// A frame's planes on the GPU, and whether it was drawn this frame.
struct Uploaded {
    planes: Vec<PlaneTexture>,
    drawn: bool,
}

/// The frames on screen, uploaded, and textures to upload others into.
pub(super) struct YuvTextures {
    gpu: Arc<gpu::Context>,
    upload_belt: BufferBelt,
    frames: FxHashMap<u64, Uploaded>,
    /// Textures no frame on screen uses any more, to use again.
    spare: Vec<PlaneTexture>,
}

/// How a plane's samples go in a texture: 8-bit ones as they are, 16-bit
/// ones as their two bytes in two channels (blade has no 16-bit unorm
/// formats), which `fs_yuv` puts back together.
fn plane_format(wide: bool, channels: u32) -> (gpu::TextureFormat, u32) {
    match (wide, channels) {
        (false, 1) => (gpu::TextureFormat::R8Unorm, 1),
        (false, _) => (gpu::TextureFormat::Rg8Unorm, 2),
        (true, 1) => (gpu::TextureFormat::Rg8Unorm, 2),
        (true, _) => (gpu::TextureFormat::Rgba8Unorm, 4),
    }
}

impl YuvTextures {
    pub(super) fn new(gpu: &Arc<gpu::Context>) -> Self {
        Self {
            gpu: Arc::clone(gpu),
            upload_belt: BufferBelt::new(BufferBeltDescriptor {
                memory: gpu::Memory::Upload,
                min_chunk_size: 0x100000,
                alignment: 64,
            }),
            frames: FxHashMap::default(),
            spare: Vec::new(),
        }
    }

    /// Upload the frames in `surfaces` that aren't yet, with copies on
    /// `encoder` before the frame's render pass.
    pub(super) fn prepare(&mut self, surfaces: &[PaintSurface], encoder: &mut gpu::CommandEncoder) {
        for uploaded in self.frames.values_mut() {
            uploaded.drawn = false;
        }
        let mut created = Vec::new();
        let mut copies = Vec::new();
        for surface in surfaces {
            let frame = &surface.frame;
            if let Some(uploaded) = self.frames.get_mut(&frame.id()) {
                uploaded.drawn = true;
                continue;
            }
            let planes = self.upload(frame, &mut created, &mut copies);
            self.frames.insert(
                frame.id(),
                Uploaded {
                    planes,
                    drawn: true,
                },
            );
        }
        for texture in created {
            encoder.init_texture(texture);
        }
        if copies.is_empty() {
            return;
        }
        let mut transfers = encoder.transfer("video frames");
        for (data, bytes_per_row, texture, (width, height)) in copies {
            transfers.copy_buffer_to_texture(
                data,
                bytes_per_row,
                gpu::TexturePiece {
                    texture,
                    mip_level: 0,
                    array_layer: 0,
                    origin: [0, 0, 0],
                },
                gpu::Extent {
                    width,
                    height,
                    depth: 1,
                },
            );
        }
    }

    /// Textures holding `frame`'s planes, once the `copies` are made.
    fn upload(
        &mut self,
        frame: &YuvFrame,
        created: &mut Vec<gpu::Texture>,
        copies: &mut Vec<(gpu::BufferPiece, u32, gpu::Texture, (u32, u32))>,
    ) -> Vec<PlaneTexture> {
        frame
            .plane_sizes()
            .into_iter()
            .zip(frame.planes())
            .map(|((width, height, channels), bytes)| {
                let (format, texel_bytes) = plane_format(frame.is_wide(), channels);
                let plane = self.texture(format, (width, height), created);
                let data = self.upload_belt.alloc_bytes(bytes, &self.gpu);
                copies.push((data, width * texel_bytes, plane.texture, (width, height)));
                plane
            })
            .collect()
    }

    /// A spare texture of this format and size, or a new one.
    fn texture(
        &mut self,
        format: gpu::TextureFormat,
        size: (u32, u32),
        created: &mut Vec<gpu::Texture>,
    ) -> PlaneTexture {
        if let Some(index) = self
            .spare
            .iter()
            .position(|t| t.format == format && t.size == size)
        {
            return self.spare.swap_remove(index);
        }
        let texture = self.gpu.create_texture(gpu::TextureDesc {
            name: "video plane",
            format,
            size: gpu::Extent {
                width: size.0,
                height: size.1,
                depth: 1,
            },
            array_layer_count: 1,
            mip_level_count: 1,
            sample_count: 1,
            dimension: gpu::TextureDimension::D2,
            usage: gpu::TextureUsage::COPY | gpu::TextureUsage::RESOURCE,
            external: None,
        });
        let view = self.gpu.create_texture_view(
            texture,
            gpu::TextureViewDesc {
                name: "video plane",
                format,
                dimension: gpu::ViewDimension::D2,
                subresources: &Default::default(),
            },
        );
        created.push(texture);
        PlaneTexture {
            texture,
            view,
            format,
            size,
        }
    }

    /// The views of frame `id`'s Y, Cb and Cr planes; Cb and Cr are the same
    /// when they are interleaved in one.
    pub(super) fn views(&self, id: u64) -> Option<[gpu::TextureView; 3]> {
        let planes = &self.frames.get(&id)?.planes;
        let view = |index: usize| planes[index.min(planes.len() - 1)].view;
        Some([view(0), view(1), view(2)])
    }

    /// Once the frame is submitted and the one before it is done: frames
    /// not drawn in it give their textures back, as nothing uses them now.
    pub(super) fn after_frame(&mut self, sync_point: &gpu::SyncPoint) {
        self.upload_belt.flush(sync_point);
        let gone: Vec<u64> = self
            .frames
            .iter()
            .filter(|(_, uploaded)| !uploaded.drawn)
            .map(|(&id, _)| id)
            .collect();
        for id in gone {
            if let Some(uploaded) = self.frames.remove(&id) {
                self.spare.extend(uploaded.planes);
            }
        }
        // Keep enough to reuse for the next frames of the videos on screen,
        // which each replace theirs.
        const SPARE: usize = 12;
        let in_use: usize = self.frames.values().map(|u| u.planes.len()).sum();
        while self.spare.len() > SPARE.max(in_use) {
            let plane = self.spare.remove(0);
            self.destroy_plane(plane);
        }
    }

    fn destroy_plane(&self, plane: PlaneTexture) {
        self.gpu.destroy_texture_view(plane.view);
        self.gpu.destroy_texture(plane.texture);
    }

    pub(super) fn destroy(&mut self) {
        let planes: Vec<PlaneTexture> = self
            .frames
            .drain()
            .flat_map(|(_, uploaded)| uploaded.planes)
            .chain(self.spare.drain(..))
            .collect();
        for plane in planes {
            self.destroy_plane(plane);
        }
        self.upload_belt.destroy(&self.gpu);
    }
}
