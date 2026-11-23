use super::metal_atlas::MetalAtlas;
use crate::{
    AtlasTextureId, BACKDROP_LEVELS, Backdrop, Background, Bounds, ContentMask, ContentTransform,
    DevicePixels, MonochromeSprite, PaintSurface, Path, Point, PolychromeSprite, PrimitiveBatch,
    Quad, ScaledPixels, Scene, Shadow, Size, Surface, Underline, Visualizer,
    backdrop_level_region, backdrop_level_size, point, size,
};
use anyhow::Result;
use block::ConcreteBlock;
use cocoa::{
    base::{NO, YES},
    foundation::{NSSize, NSUInteger},
    quartzcore::AutoresizingMask,
};

use core_foundation::base::TCFType;
use core_foundation::data::{CFData, CFDataRef};
use core_foundation::number::{CFNumber, CFNumberRef};
use core_foundation::string::{CFString, CFStringRef};
use core_graphics::color_space::{CGColorSpace, kCGColorSpaceExtendedSRGB};
use core_video::{
    buffer::CVBufferGetAttachment,
    image_buffer::{
        CVColorPrimariesGetIntegerCodePointForString, CVTransferFunctionGetIntegerCodePointForString,
        CVYCbCrMatrixGetIntegerCodePointForString, kCVImageBufferColorPrimariesKey,
        kCVImageBufferTransferFunctionKey, kCVImageBufferYCbCrMatrixKey,
    },
    metal_texture::CVMetalTextureGetTexture,
    metal_texture_cache::CVMetalTextureCache,
    pixel_buffer::{
        CVPixelBuffer, kCVPixelFormatType_420YpCbCr8BiPlanarFullRange,
        kCVPixelFormatType_420YpCbCr10BiPlanarFullRange,
        kCVPixelFormatType_420YpCbCr10BiPlanarVideoRange,
    },
};
use foreign_types::{ForeignType, ForeignTypeRef};
use metal::{
    CAMetalLayer, CommandQueue, MTLPixelFormat, MTLResourceOptions, NSRange,
    RenderPassColorAttachmentDescriptorRef,
};
use objc::{self, msg_send, sel, sel_impl};
use parking_lot::Mutex;

use std::{cell::Cell, ffi::c_void, mem, ptr, sync::Arc};

// Exported to metal
pub(crate) type PointF = crate::Point<f32>;

#[cfg(not(feature = "runtime_shaders"))]
const SHADERS_METALLIB: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/shaders.metallib"));
#[cfg(feature = "runtime_shaders")]
const SHADERS_SOURCE_FILE: &str = include_str!(concat!(env!("OUT_DIR"), "/stitched_shaders.metal"));
// Use 4x MSAA, all devices support it.
// https://developer.apple.com/documentation/metal/mtldevice/1433355-supportstexturesamplecount
const PATH_SAMPLE_COUNT: u32 = 4;
/// Plays: the drawable is in half floats and extended sRGB, so HDR video can
/// be brighter than SDR white. Everything else draws exactly as it did in
/// 8-bit sRGB. See PLAYS.md.
const DRAWABLE_FORMAT: MTLPixelFormat = MTLPixelFormat::RGBA16Float;
/// Plays: what's under backdrops is kept in half floats, as the drawable is.
const BACKDROP_LEVEL_FORMAT: MTLPixelFormat = MTLPixelFormat::RGBA16Float;
/// Plays: frames without an HDR surface before EDR is turned off again, so a
/// paused or briefly hidden HDR video doesn't make the screen ramp up and down.
const EDR_LINGER_FRAMES: u32 = 120;
/// Plays: the pixel buffer attachment (`CFData`) that holds a Dolby Vision
/// frame's reshaping, as `DoviParams` in shaders.metal. Plays sets it.
const DOLBY_VISION_ATTACHMENT: &str = "PlaysDolbyVision";
/// Plays: the pixel buffer attachment (`CFNumber`) with the gamma to draw a
/// surface with; 1 without it. Plays sets it.
const GAMMA_ATTACHMENT: &str = "PlaysGamma";

pub type Context = Arc<Mutex<InstanceBufferPool>>;
pub type Renderer = MetalRenderer;

pub unsafe fn new_renderer(
    context: self::Context,
    _native_window: *mut c_void,
    _native_view: *mut c_void,
    _bounds: crate::Size<f32>,
    _transparent: bool,
) -> Renderer {
    MetalRenderer::new(context)
}

pub(crate) struct InstanceBufferPool {
    buffer_size: usize,
    buffers: Vec<metal::Buffer>,
}

impl Default for InstanceBufferPool {
    fn default() -> Self {
        Self {
            buffer_size: 2 * 1024 * 1024,
            buffers: Vec::new(),
        }
    }
}

pub(crate) struct InstanceBuffer {
    metal_buffer: metal::Buffer,
    size: usize,
}

impl InstanceBufferPool {
    pub(crate) fn reset(&mut self, buffer_size: usize) {
        self.buffer_size = buffer_size;
        self.buffers.clear();
    }

    pub(crate) fn acquire(&mut self, device: &metal::Device) -> InstanceBuffer {
        let buffer = self.buffers.pop().unwrap_or_else(|| {
            device.new_buffer(
                self.buffer_size as u64,
                MTLResourceOptions::StorageModeManaged,
            )
        });
        InstanceBuffer {
            metal_buffer: buffer,
            size: self.buffer_size,
        }
    }

    pub(crate) fn release(&mut self, buffer: InstanceBuffer) {
        if buffer.size == self.buffer_size {
            self.buffers.push(buffer.metal_buffer)
        }
    }
}

pub(crate) struct MetalRenderer {
    device: metal::Device,
    layer: metal::MetalLayer,
    presents_with_transaction: bool,
    command_queue: CommandQueue,
    paths_rasterization_pipeline_state: metal::RenderPipelineState,
    path_sprites_pipeline_state: metal::RenderPipelineState,
    shadows_pipeline_state: metal::RenderPipelineState,
    quads_pipeline_state: metal::RenderPipelineState,
    underlines_pipeline_state: metal::RenderPipelineState,
    monochrome_sprites_pipeline_state: metal::RenderPipelineState,
    polychrome_sprites_pipeline_state: metal::RenderPipelineState,
    surfaces_pipeline_state: metal::RenderPipelineState,
    hdr_surfaces_pipeline_state: metal::RenderPipelineState,
    dovi_surfaces_pipeline_state: metal::RenderPipelineState,
    /// Plays: halves what's under backdrops, a level at a time.
    backdrop_pass_pipeline_state: metal::RenderPipelineState,
    /// Plays: draws backdrops from those levels.
    backdrops_pipeline_state: metal::RenderPipelineState,
    /// Plays: draws visualizers, each by its own shader.
    visualizers_pipeline_state: metal::RenderPipelineState,
    /// Plays: what's under backdrops, from half the viewport down, then a
    /// copy of the frame they read sharp, made for the viewport size it
    /// holds when a frame first has backdrops.
    backdrop_levels: Option<(Size<DevicePixels>, Vec<metal::Texture>)>,
    /// How bright the screen can go now, relative to SDR white; read each frame.
    edr_headroom: f32,
    /// Frames since one had an HDR surface, while EDR is on.
    frames_without_hdr: Option<u32>,
    unit_vertices: metal::Buffer,
    #[allow(clippy::arc_with_non_send_sync)]
    instance_buffer_pool: Arc<Mutex<InstanceBufferPool>>,
    sprite_atlas: Arc<MetalAtlas>,
    core_video_texture_cache: core_video::metal_texture_cache::CVMetalTextureCache,
    path_intermediate_texture: Option<metal::Texture>,
    path_intermediate_msaa_texture: Option<metal::Texture>,
    path_sample_count: u32,
}

#[repr(C)]
pub struct PathRasterizationVertex {
    pub xy_position: Point<ScaledPixels>,
    pub st_position: Point<f32>,
    pub color: Background,
    pub bounds: Bounds<ScaledPixels>,
}

impl MetalRenderer {
    pub fn new(instance_buffer_pool: Arc<Mutex<InstanceBufferPool>>) -> Self {
        // Prefer low‐power integrated GPUs on Intel Mac. On Apple
        // Silicon, there is only ever one GPU, so this is equivalent to
        // `metal::Device::system_default()`.
        let mut devices = metal::Device::all();
        devices.sort_by_key(|device| (device.is_removable(), device.is_low_power()));
        let Some(device) = devices.pop() else {
            log::error!("unable to access a compatible graphics device");
            std::process::exit(1);
        };

        let layer = metal::MetalLayer::new();
        layer.set_device(&device);
        layer.set_pixel_format(DRAWABLE_FORMAT);
        if let Some(extended_srgb) =
            CGColorSpace::create_with_name(unsafe { kCGColorSpaceExtendedSRGB })
        {
            unsafe {
                let _: () = msg_send![&*layer, setColorspace: extended_srgb.as_ptr()];
            }
        }
        layer.set_opaque(false);
        layer.set_maximum_drawable_count(3);
        // Plays: backdrops read what was drawn so far.
        layer.set_framebuffer_only(false);
        unsafe {
            let _: () = msg_send![&*layer, setAllowsNextDrawableTimeout: NO];
            let _: () = msg_send![&*layer, setNeedsDisplayOnBoundsChange: YES];
            let _: () = msg_send![
                &*layer,
                setAutoresizingMask: AutoresizingMask::WIDTH_SIZABLE
                    | AutoresizingMask::HEIGHT_SIZABLE
            ];
        }
        #[cfg(feature = "runtime_shaders")]
        let library = device
            .new_library_with_source(&SHADERS_SOURCE_FILE, &metal::CompileOptions::new())
            .expect("error building metal library");
        #[cfg(not(feature = "runtime_shaders"))]
        let library = device
            .new_library_with_data(SHADERS_METALLIB)
            .expect("error building metal library");

        fn to_float2_bits(point: PointF) -> u64 {
            let mut output = point.y.to_bits() as u64;
            output <<= 32;
            output |= point.x.to_bits() as u64;
            output
        }

        let unit_vertices = [
            to_float2_bits(point(0., 0.)),
            to_float2_bits(point(1., 0.)),
            to_float2_bits(point(0., 1.)),
            to_float2_bits(point(0., 1.)),
            to_float2_bits(point(1., 0.)),
            to_float2_bits(point(1., 1.)),
        ];
        let unit_vertices = device.new_buffer_with_data(
            unit_vertices.as_ptr() as *const c_void,
            mem::size_of_val(&unit_vertices) as u64,
            MTLResourceOptions::StorageModeManaged,
        );

        let paths_rasterization_pipeline_state = build_path_rasterization_pipeline_state(
            &device,
            &library,
            "paths_rasterization",
            "path_rasterization_vertex",
            "path_rasterization_fragment",
            MTLPixelFormat::BGRA8Unorm,
            PATH_SAMPLE_COUNT,
        );
        let path_sprites_pipeline_state = build_path_sprite_pipeline_state(
            &device,
            &library,
            "path_sprites",
            "path_sprite_vertex",
            "path_sprite_fragment",
            DRAWABLE_FORMAT,
        );
        let shadows_pipeline_state = build_pipeline_state(
            &device,
            &library,
            "shadows",
            "shadow_vertex",
            "shadow_fragment",
            DRAWABLE_FORMAT,
        );
        let quads_pipeline_state = build_pipeline_state(
            &device,
            &library,
            "quads",
            "quad_vertex",
            "quad_fragment",
            DRAWABLE_FORMAT,
        );
        // Plays: see `Visualizer`.
        let visualizers_pipeline_state = build_pipeline_state(
            &device,
            &library,
            "visualizers",
            "visualizer_vertex",
            "visualizer_fragment",
            DRAWABLE_FORMAT,
        );
        let underlines_pipeline_state = build_pipeline_state(
            &device,
            &library,
            "underlines",
            "underline_vertex",
            "underline_fragment",
            DRAWABLE_FORMAT,
        );
        let monochrome_sprites_pipeline_state = build_pipeline_state(
            &device,
            &library,
            "monochrome_sprites",
            "monochrome_sprite_vertex",
            "monochrome_sprite_fragment",
            DRAWABLE_FORMAT,
        );
        let polychrome_sprites_pipeline_state = build_pipeline_state(
            &device,
            &library,
            "polychrome_sprites",
            "polychrome_sprite_vertex",
            "polychrome_sprite_fragment",
            DRAWABLE_FORMAT,
        );
        let surfaces_pipeline_state = build_pipeline_state(
            &device,
            &library,
            "surfaces",
            "surface_vertex",
            "surface_fragment",
            DRAWABLE_FORMAT,
        );
        let hdr_surfaces_pipeline_state = build_pipeline_state(
            &device,
            &library,
            "hdr_surfaces",
            "surface_vertex",
            "surface_hdr_fragment",
            DRAWABLE_FORMAT,
        );
        let dovi_surfaces_pipeline_state = build_pipeline_state(
            &device,
            &library,
            "dovi_surfaces",
            "surface_vertex",
            "surface_dovi_fragment",
            DRAWABLE_FORMAT,
        );

        let backdrop_pass_pipeline_state = build_backdrop_pass_pipeline_state(&device, &library);
        let backdrops_pipeline_state = build_pipeline_state(
            &device,
            &library,
            "backdrops",
            "backdrop_vertex",
            "backdrop_fragment",
            DRAWABLE_FORMAT,
        );

        let command_queue = device.new_command_queue();
        let sprite_atlas = Arc::new(MetalAtlas::new(device.clone()));
        let core_video_texture_cache =
            CVMetalTextureCache::new(None, device.clone(), None).unwrap();

        Self {
            device,
            layer,
            presents_with_transaction: false,
            command_queue,
            paths_rasterization_pipeline_state,
            path_sprites_pipeline_state,
            shadows_pipeline_state,
            quads_pipeline_state,
            underlines_pipeline_state,
            monochrome_sprites_pipeline_state,
            polychrome_sprites_pipeline_state,
            surfaces_pipeline_state,
            hdr_surfaces_pipeline_state,
            dovi_surfaces_pipeline_state,
            backdrop_pass_pipeline_state,
            backdrops_pipeline_state,
            visualizers_pipeline_state,
            backdrop_levels: None,
            edr_headroom: 1.0,
            frames_without_hdr: None,
            unit_vertices,
            instance_buffer_pool,
            sprite_atlas,
            core_video_texture_cache,
            path_intermediate_texture: None,
            path_intermediate_msaa_texture: None,
            path_sample_count: PATH_SAMPLE_COUNT,
        }
    }

    pub fn layer(&self) -> &metal::MetalLayerRef {
        &self.layer
    }

    pub fn layer_ptr(&self) -> *mut CAMetalLayer {
        self.layer.as_ptr()
    }

    pub fn sprite_atlas(&self) -> &Arc<MetalAtlas> {
        &self.sprite_atlas
    }

    pub fn set_presents_with_transaction(&mut self, presents_with_transaction: bool) {
        self.presents_with_transaction = presents_with_transaction;
        self.layer
            .set_presents_with_transaction(presents_with_transaction);
    }

    pub fn update_drawable_size(&mut self, size: Size<DevicePixels>) {
        let size = NSSize {
            width: size.width.0 as f64,
            height: size.height.0 as f64,
        };
        unsafe {
            let _: () = msg_send![
                self.layer(),
                setDrawableSize: size
            ];
        }
        let device_pixels_size = Size {
            width: DevicePixels(size.width as i32),
            height: DevicePixels(size.height as i32),
        };
        self.update_path_intermediate_textures(device_pixels_size);
    }

    fn update_path_intermediate_textures(&mut self, size: Size<DevicePixels>) {
        // We are uncertain when this happens, but sometimes size can be 0 here. Most likely before
        // the layout pass on window creation. Zero-sized texture creation causes SIGABRT.
        // https://github.com/zed-industries/zed/issues/36229
        if size.width.0 <= 0 || size.height.0 <= 0 {
            self.path_intermediate_texture = None;
            self.path_intermediate_msaa_texture = None;
            return;
        }

        let texture_descriptor = metal::TextureDescriptor::new();
        texture_descriptor.set_width(size.width.0 as u64);
        texture_descriptor.set_height(size.height.0 as u64);
        texture_descriptor.set_pixel_format(metal::MTLPixelFormat::BGRA8Unorm);
        texture_descriptor
            .set_usage(metal::MTLTextureUsage::RenderTarget | metal::MTLTextureUsage::ShaderRead);
        self.path_intermediate_texture = Some(self.device.new_texture(&texture_descriptor));

        if self.path_sample_count > 1 {
            let mut msaa_descriptor = texture_descriptor;
            msaa_descriptor.set_texture_type(metal::MTLTextureType::D2Multisample);
            msaa_descriptor.set_storage_mode(metal::MTLStorageMode::Private);
            msaa_descriptor.set_sample_count(self.path_sample_count as _);
            self.path_intermediate_msaa_texture = Some(self.device.new_texture(&msaa_descriptor));
        } else {
            self.path_intermediate_msaa_texture = None;
        }
    }

    pub fn update_transparency(&self, _transparent: bool) {
        // todo(mac)?
    }

    pub fn destroy(&self) {
        // nothing to do
    }

    pub fn draw(&mut self, scene: &Scene) {
        self.edr_headroom = self.read_edr_headroom();
        let has_hdr = scene.surfaces.iter().any(|s| is_hdr(&s.image_buffer));
        self.update_edr(has_hdr);
        let layer = self.layer.clone();
        let viewport_size = layer.drawable_size();
        let viewport_size: Size<DevicePixels> = size(
            (viewport_size.width.ceil() as i32).into(),
            (viewport_size.height.ceil() as i32).into(),
        );
        let drawable = if let Some(drawable) = layer.next_drawable() {
            drawable
        } else {
            log::error!(
                "failed to retrieve next drawable, drawable size: {:?}",
                viewport_size
            );
            return;
        };

        loop {
            let mut instance_buffer = self.instance_buffer_pool.lock().acquire(&self.device);

            let command_buffer = self.draw_primitives(
                scene,
                &mut instance_buffer,
                drawable.texture(),
                viewport_size,
            );

            match command_buffer {
                Ok(command_buffer) => {
                    let instance_buffer_pool = self.instance_buffer_pool.clone();
                    let instance_buffer = Cell::new(Some(instance_buffer));
                    let block = ConcreteBlock::new(move |_| {
                        if let Some(instance_buffer) = instance_buffer.take() {
                            instance_buffer_pool.lock().release(instance_buffer);
                        }
                    });
                    let block = block.copy();
                    command_buffer.add_completed_handler(&block);

                    if self.presents_with_transaction {
                        command_buffer.commit();
                        command_buffer.wait_until_scheduled();
                        drawable.present();
                    } else {
                        command_buffer.present_drawable(drawable);
                        command_buffer.commit();
                    }
                    return;
                }
                Err(err) => {
                    log::error!(
                        "failed to render: {}. retrying with larger instance buffer size",
                        err
                    );
                    let mut instance_buffer_pool = self.instance_buffer_pool.lock();
                    let buffer_size = instance_buffer_pool.buffer_size;
                    if buffer_size >= 256 * 1024 * 1024 {
                        log::error!("instance buffer size grew too large: {}", buffer_size);
                        break;
                    }
                    instance_buffer_pool.reset(buffer_size * 2);
                    log::info!(
                        "increased instance buffer size to {}",
                        instance_buffer_pool.buffer_size
                    );
                }
            }
        }
    }

    /// The current EDR headroom of the screen the window is on: how many
    /// times brighter than SDR white it can show right now, or 1.
    fn read_edr_headroom(&self) -> f32 {
        unsafe {
            // A layer-backed view is its layer's delegate.
            let view: cocoa::base::id = msg_send![self.layer(), delegate];
            if view.is_null() {
                return 1.0;
            }
            let responds: cocoa::base::BOOL = msg_send![view, respondsToSelector: sel!(window)];
            if responds != YES {
                return 1.0;
            }
            let window: cocoa::base::id = msg_send![view, window];
            if window.is_null() {
                return 1.0;
            }
            let screen: cocoa::base::id = msg_send![window, screen];
            if screen.is_null() {
                return 1.0;
            }
            let headroom: f64 = msg_send![screen, maximumExtendedDynamicRangeColorComponentValue];
            (headroom as f32).max(1.0)
        }
    }

    /// Ask for EDR while HDR video is on screen, and a little after. macOS
    /// only raises the headroom for layers that want it, and doing so costs
    /// power, so it's off otherwise.
    fn update_edr(&mut self, has_hdr: bool) {
        let wants = match (has_hdr, self.frames_without_hdr) {
            (true, _) => {
                self.frames_without_hdr = Some(0);
                true
            }
            (false, Some(frames)) if frames < EDR_LINGER_FRAMES => {
                self.frames_without_hdr = Some(frames + 1);
                true
            }
            (false, _) => {
                self.frames_without_hdr = None;
                false
            }
        };
        if self.layer.wants_extended_dynamic_range_content() != wants {
            self.layer.set_wants_extended_dynamic_range_content(wants);
        }
    }

    fn draw_primitives(
        &mut self,
        scene: &Scene,
        instance_buffer: &mut InstanceBuffer,
        target: &metal::TextureRef,
        viewport_size: Size<DevicePixels>,
    ) -> Result<metal::CommandBuffer> {
        let command_queue = self.command_queue.clone();
        let command_buffer = command_queue.new_command_buffer();
        let alpha = if self.layer.is_opaque() { 1. } else { 0. };
        let mut instance_offset = 0;

        let mut command_encoder =
            new_command_encoder(command_buffer, target, viewport_size, |color_attachment| {
                color_attachment.set_load_action(metal::MTLLoadAction::Clear);
                color_attachment.set_clear_color(metal::MTLClearColor::new(0., 0., 0., alpha));
            });

        for batch in scene.batches() {
            let ok = match batch {
                PrimitiveBatch::Shadows(shadows) => self.draw_shadows(
                    shadows,
                    instance_buffer,
                    &mut instance_offset,
                    viewport_size,
                    command_encoder,
                ),
                PrimitiveBatch::Quads(quads) => self.draw_quads(
                    quads,
                    instance_buffer,
                    &mut instance_offset,
                    viewport_size,
                    command_encoder,
                ),
                PrimitiveBatch::Visualizers(visualizers) => self.draw_visualizers(
                    visualizers,
                    instance_buffer,
                    &mut instance_offset,
                    viewport_size,
                    command_encoder,
                ),
                PrimitiveBatch::Paths(paths) => {
                    command_encoder.end_encoding();

                    let did_draw = self.draw_paths_to_intermediate(
                        paths,
                        instance_buffer,
                        &mut instance_offset,
                        viewport_size,
                        command_buffer,
                    );

                    command_encoder = new_command_encoder(
                        command_buffer,
                        target,
                        viewport_size,
                        |color_attachment| {
                            color_attachment.set_load_action(metal::MTLLoadAction::Load);
                        },
                    );

                    if did_draw {
                        self.draw_paths_from_intermediate(
                            paths,
                            instance_buffer,
                            &mut instance_offset,
                            viewport_size,
                            command_encoder,
                        )
                    } else {
                        false
                    }
                }
                PrimitiveBatch::Underlines(underlines) => self.draw_underlines(
                    underlines,
                    instance_buffer,
                    &mut instance_offset,
                    viewport_size,
                    command_encoder,
                ),
                PrimitiveBatch::MonochromeSprites {
                    texture_id,
                    sprites,
                } => self.draw_monochrome_sprites(
                    texture_id,
                    sprites,
                    instance_buffer,
                    &mut instance_offset,
                    viewport_size,
                    command_encoder,
                ),
                PrimitiveBatch::PolychromeSprites {
                    texture_id,
                    sprites,
                } => self.draw_polychrome_sprites(
                    texture_id,
                    sprites,
                    instance_buffer,
                    &mut instance_offset,
                    viewport_size,
                    command_encoder,
                ),
                PrimitiveBatch::Surfaces(surfaces) => self.draw_surfaces(
                    surfaces,
                    instance_buffer,
                    &mut instance_offset,
                    viewport_size,
                    command_encoder,
                ),
                // Plays: what's drawn so far is read and halved, then the
                // backdrops drawn from it.
                PrimitiveBatch::Backdrops(backdrops) => {
                    command_encoder.end_encoding();
                    let levels = self.halve_under_backdrops(
                        backdrops,
                        target,
                        viewport_size,
                        command_buffer,
                    );
                    command_encoder = new_command_encoder(
                        command_buffer,
                        target,
                        viewport_size,
                        |color_attachment| {
                            color_attachment.set_load_action(metal::MTLLoadAction::Load);
                        },
                    );
                    match levels {
                        Some(levels) => self.draw_backdrops(
                            backdrops,
                            &levels,
                            instance_buffer,
                            &mut instance_offset,
                            viewport_size,
                            command_encoder,
                        ),
                        None => true,
                    }
                }
            };
            if !ok {
                command_encoder.end_encoding();
                anyhow::bail!(
                    "scene too large: {} paths, {} shadows, {} quads, {} underlines, {} mono, {} poly, {} surfaces, {} backdrops, {} visualizers",
                    scene.paths.len(),
                    scene.shadows.len(),
                    scene.quads.len(),
                    scene.underlines.len(),
                    scene.monochrome_sprites.len(),
                    scene.polychrome_sprites.len(),
                    scene.surfaces.len(),
                    scene.backdrops.len(),
                    scene.visualizers.len(),
                );
            }
        }

        command_encoder.end_encoding();

        instance_buffer.metal_buffer.did_modify_range(NSRange {
            location: 0,
            length: instance_offset as NSUInteger,
        });
        Ok(command_buffer.to_owned())
    }

    fn draw_paths_to_intermediate(
        &self,
        paths: &[Path<ScaledPixels>],
        instance_buffer: &mut InstanceBuffer,
        instance_offset: &mut usize,
        viewport_size: Size<DevicePixels>,
        command_buffer: &metal::CommandBufferRef,
    ) -> bool {
        if paths.is_empty() {
            return true;
        }
        let Some(intermediate_texture) = &self.path_intermediate_texture else {
            return false;
        };

        let render_pass_descriptor = metal::RenderPassDescriptor::new();
        let color_attachment = render_pass_descriptor
            .color_attachments()
            .object_at(0)
            .unwrap();
        color_attachment.set_load_action(metal::MTLLoadAction::Clear);
        color_attachment.set_clear_color(metal::MTLClearColor::new(0., 0., 0., 0.));

        if let Some(msaa_texture) = &self.path_intermediate_msaa_texture {
            color_attachment.set_texture(Some(msaa_texture));
            color_attachment.set_resolve_texture(Some(intermediate_texture));
            color_attachment.set_store_action(metal::MTLStoreAction::MultisampleResolve);
        } else {
            color_attachment.set_texture(Some(intermediate_texture));
            color_attachment.set_store_action(metal::MTLStoreAction::Store);
        }

        let command_encoder = command_buffer.new_render_command_encoder(render_pass_descriptor);
        command_encoder.set_render_pipeline_state(&self.paths_rasterization_pipeline_state);

        align_offset(instance_offset);
        let mut vertices = Vec::new();
        for path in paths {
            vertices.extend(path.vertices.iter().map(|v| PathRasterizationVertex {
                xy_position: v.xy_position,
                st_position: v.st_position,
                color: path.color,
                bounds: path.bounds.intersect(&path.content_mask.bounds),
            }));
        }
        let vertices_bytes_len = mem::size_of_val(vertices.as_slice());
        let next_offset = *instance_offset + vertices_bytes_len;
        if next_offset > instance_buffer.size {
            command_encoder.end_encoding();
            return false;
        }
        command_encoder.set_vertex_buffer(
            PathRasterizationInputIndex::Vertices as u64,
            Some(&instance_buffer.metal_buffer),
            *instance_offset as u64,
        );
        command_encoder.set_vertex_bytes(
            PathRasterizationInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );
        command_encoder.set_fragment_buffer(
            PathRasterizationInputIndex::Vertices as u64,
            Some(&instance_buffer.metal_buffer),
            *instance_offset as u64,
        );
        let buffer_contents =
            unsafe { (instance_buffer.metal_buffer.contents() as *mut u8).add(*instance_offset) };
        unsafe {
            ptr::copy_nonoverlapping(
                vertices.as_ptr() as *const u8,
                buffer_contents,
                vertices_bytes_len,
            );
        }
        command_encoder.draw_primitives(
            metal::MTLPrimitiveType::Triangle,
            0,
            vertices.len() as u64,
        );
        *instance_offset = next_offset;

        command_encoder.end_encoding();
        true
    }

    fn draw_shadows(
        &self,
        shadows: &[Shadow],
        instance_buffer: &mut InstanceBuffer,
        instance_offset: &mut usize,
        viewport_size: Size<DevicePixels>,
        command_encoder: &metal::RenderCommandEncoderRef,
    ) -> bool {
        if shadows.is_empty() {
            return true;
        }
        align_offset(instance_offset);

        command_encoder.set_render_pipeline_state(&self.shadows_pipeline_state);
        command_encoder.set_vertex_buffer(
            ShadowInputIndex::Vertices as u64,
            Some(&self.unit_vertices),
            0,
        );
        command_encoder.set_vertex_buffer(
            ShadowInputIndex::Shadows as u64,
            Some(&instance_buffer.metal_buffer),
            *instance_offset as u64,
        );
        command_encoder.set_fragment_buffer(
            ShadowInputIndex::Shadows as u64,
            Some(&instance_buffer.metal_buffer),
            *instance_offset as u64,
        );

        command_encoder.set_vertex_bytes(
            ShadowInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );

        let shadow_bytes_len = mem::size_of_val(shadows);
        let buffer_contents =
            unsafe { (instance_buffer.metal_buffer.contents() as *mut u8).add(*instance_offset) };

        let next_offset = *instance_offset + shadow_bytes_len;
        if next_offset > instance_buffer.size {
            return false;
        }

        unsafe {
            ptr::copy_nonoverlapping(
                shadows.as_ptr() as *const u8,
                buffer_contents,
                shadow_bytes_len,
            );
        }

        command_encoder.draw_primitives_instanced(
            metal::MTLPrimitiveType::Triangle,
            0,
            6,
            shadows.len() as u64,
        );
        *instance_offset = next_offset;
        true
    }

    /// Plays: see `Visualizer`.
    fn draw_visualizers(
        &self,
        visualizers: &[Visualizer],
        instance_buffer: &mut InstanceBuffer,
        instance_offset: &mut usize,
        viewport_size: Size<DevicePixels>,
        command_encoder: &metal::RenderCommandEncoderRef,
    ) -> bool {
        if visualizers.is_empty() {
            return true;
        }
        align_offset(instance_offset);

        command_encoder.set_render_pipeline_state(&self.visualizers_pipeline_state);
        command_encoder.set_vertex_buffer(
            VisualizerInputIndex::Vertices as u64,
            Some(&self.unit_vertices),
            0,
        );
        command_encoder.set_vertex_buffer(
            VisualizerInputIndex::Visualizers as u64,
            Some(&instance_buffer.metal_buffer),
            *instance_offset as u64,
        );
        command_encoder.set_fragment_buffer(
            VisualizerInputIndex::Visualizers as u64,
            Some(&instance_buffer.metal_buffer),
            *instance_offset as u64,
        );
        command_encoder.set_vertex_bytes(
            VisualizerInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );

        let visualizer_bytes_len = mem::size_of_val(visualizers);
        let next_offset = *instance_offset + visualizer_bytes_len;
        if next_offset > instance_buffer.size {
            return false;
        }
        let buffer_contents =
            unsafe { (instance_buffer.metal_buffer.contents() as *mut u8).add(*instance_offset) };
        unsafe {
            ptr::copy_nonoverlapping(
                visualizers.as_ptr() as *const u8,
                buffer_contents,
                visualizer_bytes_len,
            );
        }

        command_encoder.draw_primitives_instanced(
            metal::MTLPrimitiveType::Triangle,
            0,
            6,
            visualizers.len() as u64,
        );
        *instance_offset = next_offset;
        true
    }

    fn draw_quads(
        &self,
        quads: &[Quad],
        instance_buffer: &mut InstanceBuffer,
        instance_offset: &mut usize,
        viewport_size: Size<DevicePixels>,
        command_encoder: &metal::RenderCommandEncoderRef,
    ) -> bool {
        if quads.is_empty() {
            return true;
        }
        align_offset(instance_offset);

        command_encoder.set_render_pipeline_state(&self.quads_pipeline_state);
        command_encoder.set_vertex_buffer(
            QuadInputIndex::Vertices as u64,
            Some(&self.unit_vertices),
            0,
        );
        command_encoder.set_vertex_buffer(
            QuadInputIndex::Quads as u64,
            Some(&instance_buffer.metal_buffer),
            *instance_offset as u64,
        );
        command_encoder.set_fragment_buffer(
            QuadInputIndex::Quads as u64,
            Some(&instance_buffer.metal_buffer),
            *instance_offset as u64,
        );

        command_encoder.set_vertex_bytes(
            QuadInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );

        let quad_bytes_len = mem::size_of_val(quads);
        let buffer_contents =
            unsafe { (instance_buffer.metal_buffer.contents() as *mut u8).add(*instance_offset) };

        let next_offset = *instance_offset + quad_bytes_len;
        if next_offset > instance_buffer.size {
            return false;
        }

        unsafe {
            ptr::copy_nonoverlapping(quads.as_ptr() as *const u8, buffer_contents, quad_bytes_len);
        }

        command_encoder.draw_primitives_instanced(
            metal::MTLPrimitiveType::Triangle,
            0,
            6,
            quads.len() as u64,
        );
        *instance_offset = next_offset;
        true
    }

    fn draw_paths_from_intermediate(
        &self,
        paths: &[Path<ScaledPixels>],
        instance_buffer: &mut InstanceBuffer,
        instance_offset: &mut usize,
        viewport_size: Size<DevicePixels>,
        command_encoder: &metal::RenderCommandEncoderRef,
    ) -> bool {
        let Some(first_path) = paths.first() else {
            return true;
        };

        let Some(ref intermediate_texture) = self.path_intermediate_texture else {
            return false;
        };

        command_encoder.set_render_pipeline_state(&self.path_sprites_pipeline_state);
        command_encoder.set_vertex_buffer(
            SpriteInputIndex::Vertices as u64,
            Some(&self.unit_vertices),
            0,
        );
        command_encoder.set_vertex_bytes(
            SpriteInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );

        command_encoder.set_fragment_texture(
            SpriteInputIndex::AtlasTexture as u64,
            Some(intermediate_texture),
        );

        // When copying paths from the intermediate texture to the drawable,
        // each pixel must only be copied once, in case of transparent paths.
        //
        // If all paths have the same draw order, then their bounds are all
        // disjoint, so we can copy each path's bounds individually. If this
        // batch combines different draw orders, we perform a single copy
        // for a minimal spanning rect.
        let sprites;
        if paths.last().unwrap().order == first_path.order {
            sprites = paths
                .iter()
                .map(|path| PathSprite {
                    bounds: path.clipped_bounds(),
                })
                .collect();
        } else {
            let mut bounds = first_path.clipped_bounds();
            for path in paths.iter().skip(1) {
                bounds = bounds.union(&path.clipped_bounds());
            }
            sprites = vec![PathSprite { bounds }];
        }

        align_offset(instance_offset);
        let sprite_bytes_len = mem::size_of_val(sprites.as_slice());
        let next_offset = *instance_offset + sprite_bytes_len;
        if next_offset > instance_buffer.size {
            return false;
        }

        command_encoder.set_vertex_buffer(
            SpriteInputIndex::Sprites as u64,
            Some(&instance_buffer.metal_buffer),
            *instance_offset as u64,
        );

        let buffer_contents =
            unsafe { (instance_buffer.metal_buffer.contents() as *mut u8).add(*instance_offset) };
        unsafe {
            ptr::copy_nonoverlapping(
                sprites.as_ptr() as *const u8,
                buffer_contents,
                sprite_bytes_len,
            );
        }

        command_encoder.draw_primitives_instanced(
            metal::MTLPrimitiveType::Triangle,
            0,
            6,
            sprites.len() as u64,
        );
        *instance_offset = next_offset;

        true
    }

    fn draw_underlines(
        &self,
        underlines: &[Underline],
        instance_buffer: &mut InstanceBuffer,
        instance_offset: &mut usize,
        viewport_size: Size<DevicePixels>,
        command_encoder: &metal::RenderCommandEncoderRef,
    ) -> bool {
        if underlines.is_empty() {
            return true;
        }
        align_offset(instance_offset);

        command_encoder.set_render_pipeline_state(&self.underlines_pipeline_state);
        command_encoder.set_vertex_buffer(
            UnderlineInputIndex::Vertices as u64,
            Some(&self.unit_vertices),
            0,
        );
        command_encoder.set_vertex_buffer(
            UnderlineInputIndex::Underlines as u64,
            Some(&instance_buffer.metal_buffer),
            *instance_offset as u64,
        );
        command_encoder.set_fragment_buffer(
            UnderlineInputIndex::Underlines as u64,
            Some(&instance_buffer.metal_buffer),
            *instance_offset as u64,
        );

        command_encoder.set_vertex_bytes(
            UnderlineInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );

        let underline_bytes_len = mem::size_of_val(underlines);
        let buffer_contents =
            unsafe { (instance_buffer.metal_buffer.contents() as *mut u8).add(*instance_offset) };

        let next_offset = *instance_offset + underline_bytes_len;
        if next_offset > instance_buffer.size {
            return false;
        }

        unsafe {
            ptr::copy_nonoverlapping(
                underlines.as_ptr() as *const u8,
                buffer_contents,
                underline_bytes_len,
            );
        }

        command_encoder.draw_primitives_instanced(
            metal::MTLPrimitiveType::Triangle,
            0,
            6,
            underlines.len() as u64,
        );
        *instance_offset = next_offset;
        true
    }

    fn draw_monochrome_sprites(
        &self,
        texture_id: AtlasTextureId,
        sprites: &[MonochromeSprite],
        instance_buffer: &mut InstanceBuffer,
        instance_offset: &mut usize,
        viewport_size: Size<DevicePixels>,
        command_encoder: &metal::RenderCommandEncoderRef,
    ) -> bool {
        if sprites.is_empty() {
            return true;
        }
        align_offset(instance_offset);

        let sprite_bytes_len = mem::size_of_val(sprites);
        let buffer_contents =
            unsafe { (instance_buffer.metal_buffer.contents() as *mut u8).add(*instance_offset) };

        let next_offset = *instance_offset + sprite_bytes_len;
        if next_offset > instance_buffer.size {
            return false;
        }

        let texture = self.sprite_atlas.metal_texture(texture_id);
        let texture_size = size(
            DevicePixels(texture.width() as i32),
            DevicePixels(texture.height() as i32),
        );
        command_encoder.set_render_pipeline_state(&self.monochrome_sprites_pipeline_state);
        command_encoder.set_vertex_buffer(
            SpriteInputIndex::Vertices as u64,
            Some(&self.unit_vertices),
            0,
        );
        command_encoder.set_vertex_buffer(
            SpriteInputIndex::Sprites as u64,
            Some(&instance_buffer.metal_buffer),
            *instance_offset as u64,
        );
        command_encoder.set_vertex_bytes(
            SpriteInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );
        command_encoder.set_vertex_bytes(
            SpriteInputIndex::AtlasTextureSize as u64,
            mem::size_of_val(&texture_size) as u64,
            &texture_size as *const Size<DevicePixels> as *const _,
        );
        command_encoder.set_fragment_buffer(
            SpriteInputIndex::Sprites as u64,
            Some(&instance_buffer.metal_buffer),
            *instance_offset as u64,
        );
        command_encoder.set_fragment_texture(SpriteInputIndex::AtlasTexture as u64, Some(&texture));

        unsafe {
            ptr::copy_nonoverlapping(
                sprites.as_ptr() as *const u8,
                buffer_contents,
                sprite_bytes_len,
            );
        }

        command_encoder.draw_primitives_instanced(
            metal::MTLPrimitiveType::Triangle,
            0,
            6,
            sprites.len() as u64,
        );
        *instance_offset = next_offset;
        true
    }

    fn draw_polychrome_sprites(
        &self,
        texture_id: AtlasTextureId,
        sprites: &[PolychromeSprite],
        instance_buffer: &mut InstanceBuffer,
        instance_offset: &mut usize,
        viewport_size: Size<DevicePixels>,
        command_encoder: &metal::RenderCommandEncoderRef,
    ) -> bool {
        if sprites.is_empty() {
            return true;
        }
        align_offset(instance_offset);

        let texture = self.sprite_atlas.metal_texture(texture_id);
        let texture_size = size(
            DevicePixels(texture.width() as i32),
            DevicePixels(texture.height() as i32),
        );
        command_encoder.set_render_pipeline_state(&self.polychrome_sprites_pipeline_state);
        command_encoder.set_vertex_buffer(
            SpriteInputIndex::Vertices as u64,
            Some(&self.unit_vertices),
            0,
        );
        command_encoder.set_vertex_buffer(
            SpriteInputIndex::Sprites as u64,
            Some(&instance_buffer.metal_buffer),
            *instance_offset as u64,
        );
        command_encoder.set_vertex_bytes(
            SpriteInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );
        command_encoder.set_vertex_bytes(
            SpriteInputIndex::AtlasTextureSize as u64,
            mem::size_of_val(&texture_size) as u64,
            &texture_size as *const Size<DevicePixels> as *const _,
        );
        command_encoder.set_fragment_buffer(
            SpriteInputIndex::Sprites as u64,
            Some(&instance_buffer.metal_buffer),
            *instance_offset as u64,
        );
        command_encoder.set_fragment_texture(SpriteInputIndex::AtlasTexture as u64, Some(&texture));

        let sprite_bytes_len = mem::size_of_val(sprites);
        let buffer_contents =
            unsafe { (instance_buffer.metal_buffer.contents() as *mut u8).add(*instance_offset) };

        let next_offset = *instance_offset + sprite_bytes_len;
        if next_offset > instance_buffer.size {
            return false;
        }

        unsafe {
            ptr::copy_nonoverlapping(
                sprites.as_ptr() as *const u8,
                buffer_contents,
                sprite_bytes_len,
            );
        }

        command_encoder.draw_primitives_instanced(
            metal::MTLPrimitiveType::Triangle,
            0,
            6,
            sprites.len() as u64,
        );
        *instance_offset = next_offset;
        true
    }

    fn draw_surfaces(
        &mut self,
        surfaces: &[PaintSurface],
        instance_buffer: &mut InstanceBuffer,
        instance_offset: &mut usize,
        viewport_size: Size<DevicePixels>,
        command_encoder: &metal::RenderCommandEncoderRef,
    ) -> bool {
        command_encoder.set_vertex_buffer(
            SurfaceInputIndex::Vertices as u64,
            Some(&self.unit_vertices),
            0,
        );
        command_encoder.set_vertex_bytes(
            SurfaceInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );

        for surface in surfaces {
            let texture_size = size(
                DevicePixels::from(surface.image_buffer.get_width() as i32),
                DevicePixels::from(surface.image_buffer.get_height() as i32),
            );

            // Plays: 10-bit surfaces are HDR video (see PLAYS.md).
            let hdr = is_hdr(&surface.image_buffer);
            if !hdr {
                assert_eq!(
                    surface.image_buffer.get_pixel_format(),
                    kCVPixelFormatType_420YpCbCr8BiPlanarFullRange
                );
            }
            let (y_format, cb_cr_format) = if hdr {
                (MTLPixelFormat::R16Unorm, MTLPixelFormat::RG16Unorm)
            } else {
                (MTLPixelFormat::R8Unorm, MTLPixelFormat::RG8Unorm)
            };
            if hdr {
                let dovi = dolby_vision_params(&surface.image_buffer);
                command_encoder.set_render_pipeline_state(if dovi.is_some() {
                    &self.dovi_surfaces_pipeline_state
                } else {
                    &self.hdr_surfaces_pipeline_state
                });
                let params = HdrSurfaceParams::new(&surface.image_buffer, self.edr_headroom);
                command_encoder.set_fragment_bytes(
                    SurfaceInputIndex::HdrParams as u64,
                    mem::size_of_val(&params) as u64,
                    &params as *const HdrSurfaceParams as *const _,
                );
                if let Some(dovi) = dovi {
                    let bytes = dovi.bytes();
                    command_encoder.set_fragment_bytes(
                        SurfaceInputIndex::DoviParams as u64,
                        bytes.len() as u64,
                        bytes.as_ptr() as *const _,
                    );
                }
            } else {
                command_encoder.set_render_pipeline_state(&self.surfaces_pipeline_state);
            }
            let gamma = surface_gamma(&surface.image_buffer);
            command_encoder.set_fragment_bytes(
                SurfaceInputIndex::Gamma as u64,
                mem::size_of_val(&gamma) as u64,
                &gamma as *const f32 as *const _,
            );

            let y_texture = self
                .core_video_texture_cache
                .create_texture_from_image(
                    surface.image_buffer.as_concrete_TypeRef(),
                    None,
                    y_format,
                    surface.image_buffer.get_width_of_plane(0),
                    surface.image_buffer.get_height_of_plane(0),
                    0,
                )
                .unwrap();
            let cb_cr_texture = self
                .core_video_texture_cache
                .create_texture_from_image(
                    surface.image_buffer.as_concrete_TypeRef(),
                    None,
                    cb_cr_format,
                    surface.image_buffer.get_width_of_plane(1),
                    surface.image_buffer.get_height_of_plane(1),
                    1,
                )
                .unwrap();

            align_offset(instance_offset);
            let next_offset = *instance_offset + mem::size_of::<Surface>();
            if next_offset > instance_buffer.size {
                return false;
            }

            command_encoder.set_vertex_buffer(
                SurfaceInputIndex::Surfaces as u64,
                Some(&instance_buffer.metal_buffer),
                *instance_offset as u64,
            );
            command_encoder.set_vertex_bytes(
                SurfaceInputIndex::TextureSize as u64,
                mem::size_of_val(&texture_size) as u64,
                &texture_size as *const Size<DevicePixels> as *const _,
            );
            // let y_texture = y_texture.get_texture().unwrap().
            command_encoder.set_fragment_texture(SurfaceInputIndex::YTexture as u64, unsafe {
                let texture = CVMetalTextureGetTexture(y_texture.as_concrete_TypeRef());
                Some(metal::TextureRef::from_ptr(texture as *mut _))
            });
            command_encoder.set_fragment_texture(SurfaceInputIndex::CbCrTexture as u64, unsafe {
                let texture = CVMetalTextureGetTexture(cb_cr_texture.as_concrete_TypeRef());
                Some(metal::TextureRef::from_ptr(texture as *mut _))
            });

            unsafe {
                let buffer_contents = (instance_buffer.metal_buffer.contents() as *mut u8)
                    .add(*instance_offset)
                    as *mut SurfaceBounds;
                ptr::write(
                    buffer_contents,
                    SurfaceBounds {
                        bounds: surface.bounds,
                        content_mask: surface.content_mask.clone(),
                        content: surface.content,
                    },
                );
            }

            command_encoder.draw_primitives(metal::MTLPrimitiveType::Triangle, 0, 6);
            *instance_offset = next_offset;
        }
        true
    }

    /// Plays: the levels of what's under backdrops, then the frame's copy,
    /// made for the viewport.
    fn backdrop_levels(
        &mut self,
        viewport_size: Size<DevicePixels>,
    ) -> Option<Vec<metal::Texture>> {
        if viewport_size.width.0 <= 0 || viewport_size.height.0 <= 0 {
            return None;
        }
        if let Some((made_for, levels)) = &self.backdrop_levels
            && *made_for == viewport_size
        {
            return Some(levels.clone());
        }
        let texture = |width: u32, height: u32, format, usage| {
            let descriptor = metal::TextureDescriptor::new();
            descriptor.set_width(width as u64);
            descriptor.set_height(height as u64);
            descriptor.set_pixel_format(format);
            descriptor.set_storage_mode(metal::MTLStorageMode::Private);
            descriptor.set_usage(usage);
            self.device.new_texture(&descriptor)
        };
        let mut levels: Vec<metal::Texture> = (0..BACKDROP_LEVELS)
            .map(|level| {
                let (width, height) = backdrop_level_size(
                    viewport_size.width.0 as u32,
                    viewport_size.height.0 as u32,
                    level,
                );
                texture(
                    width,
                    height,
                    BACKDROP_LEVEL_FORMAT,
                    metal::MTLTextureUsage::RenderTarget | metal::MTLTextureUsage::ShaderRead,
                )
            })
            .collect();
        levels.push(texture(
            viewport_size.width.0 as u32,
            viewport_size.height.0 as u32,
            DRAWABLE_FORMAT,
            metal::MTLTextureUsage::ShaderRead,
        ));
        self.backdrop_levels = Some((viewport_size, levels.clone()));
        Some(levels)
    }

    /// Plays: copy what `target` has under `backdrops`, and halve it into
    /// the levels, each from the one before; hand them back, the copy last.
    fn halve_under_backdrops(
        &mut self,
        backdrops: &[Backdrop],
        target: &metal::TextureRef,
        viewport_size: Size<DevicePixels>,
        command_buffer: &metal::CommandBufferRef,
    ) -> Option<Vec<metal::Texture>> {
        let (width, height) = (viewport_size.width.0 as f32, viewport_size.height.0 as f32);
        let region = Backdrop::region(backdrops)?;
        let levels = self.backdrop_levels(viewport_size)?;

        // Backdrops read the copy where they draw, between its pixels.
        let copied = region.dilate(ScaledPixels(1.));
        let (left, top) = (
            copied.left().0.max(0.).floor(),
            copied.top().0.max(0.).floor(),
        );
        let (right, bottom) = (
            copied.right().0.min(width).ceil(),
            copied.bottom().0.min(height).ceil(),
        );
        if right > left && bottom > top {
            let origin = metal::MTLOrigin {
                x: left as u64,
                y: top as u64,
                z: 0,
            };
            let blit = command_buffer.new_blit_command_encoder();
            blit.copy_from_texture(
                target,
                0,
                0,
                origin,
                metal::MTLSize {
                    width: (right - left) as u64,
                    height: (bottom - top) as u64,
                    depth: 1,
                },
                &levels[BACKDROP_LEVELS],
                0,
                0,
                origin,
            );
            blit.end_encoding();
        }

        for (level, texture) in levels[..BACKDROP_LEVELS].iter().enumerate() {
            let region = backdrop_level_region(region, level, width, height);
            let (source, source_texel): (&metal::TextureRef, f32) = match level {
                0 => (target, 1.),
                _ => (&levels[level - 1], (1u32 << level) as f32),
            };
            let params = BackdropPassParams {
                region,
                texels: [
                    source_texel,
                    (2u32 << level) as f32,
                    texture.width() as f32,
                    texture.height() as f32,
                ],
            };

            let render_pass_descriptor = metal::RenderPassDescriptor::new();
            let color_attachment = render_pass_descriptor
                .color_attachments()
                .object_at(0)
                .unwrap();
            color_attachment.set_texture(Some(texture));
            color_attachment.set_load_action(metal::MTLLoadAction::Load);
            color_attachment.set_store_action(metal::MTLStoreAction::Store);
            let command_encoder = command_buffer.new_render_command_encoder(render_pass_descriptor);
            command_encoder.set_render_pipeline_state(&self.backdrop_pass_pipeline_state);
            command_encoder.set_vertex_buffer(
                BackdropPassInputIndex::Vertices as u64,
                Some(&self.unit_vertices),
                0,
            );
            command_encoder.set_vertex_bytes(
                BackdropPassInputIndex::Params as u64,
                mem::size_of_val(&params) as u64,
                &params as *const BackdropPassParams as *const _,
            );
            command_encoder.set_fragment_bytes(
                BackdropPassInputIndex::Params as u64,
                mem::size_of_val(&params) as u64,
                &params as *const BackdropPassParams as *const _,
            );
            command_encoder
                .set_fragment_texture(BackdropPassInputIndex::Source as u64, Some(source));
            command_encoder.draw_primitives(metal::MTLPrimitiveType::Triangle, 0, 6);
            command_encoder.end_encoding();
        }
        Some(levels)
    }

    /// Plays: draw `backdrops` from the levels of what's under them, and
    /// its copy.
    fn draw_backdrops(
        &self,
        backdrops: &[Backdrop],
        levels: &[metal::Texture],
        instance_buffer: &mut InstanceBuffer,
        instance_offset: &mut usize,
        viewport_size: Size<DevicePixels>,
        command_encoder: &metal::RenderCommandEncoderRef,
    ) -> bool {
        if backdrops.is_empty() {
            return true;
        }
        align_offset(instance_offset);

        command_encoder.set_render_pipeline_state(&self.backdrops_pipeline_state);
        command_encoder.set_vertex_buffer(
            BackdropInputIndex::Vertices as u64,
            Some(&self.unit_vertices),
            0,
        );
        command_encoder.set_vertex_buffer(
            BackdropInputIndex::Backdrops as u64,
            Some(&instance_buffer.metal_buffer),
            *instance_offset as u64,
        );
        command_encoder.set_fragment_buffer(
            BackdropInputIndex::Backdrops as u64,
            Some(&instance_buffer.metal_buffer),
            *instance_offset as u64,
        );
        command_encoder.set_vertex_bytes(
            BackdropInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );
        for (index, level) in levels.iter().enumerate() {
            command_encoder.set_fragment_texture(index as u64, Some(level));
        }

        let backdrop_bytes_len = mem::size_of_val(backdrops);
        let next_offset = *instance_offset + backdrop_bytes_len;
        if next_offset > instance_buffer.size {
            return false;
        }
        unsafe {
            let buffer_contents =
                (instance_buffer.metal_buffer.contents() as *mut u8).add(*instance_offset);
            ptr::copy_nonoverlapping(
                backdrops.as_ptr() as *const u8,
                buffer_contents,
                backdrop_bytes_len,
            );
        }

        command_encoder.draw_primitives_instanced(
            metal::MTLPrimitiveType::Triangle,
            0,
            6,
            backdrops.len() as u64,
        );
        *instance_offset = next_offset;
        true
    }
}

fn new_command_encoder<'a>(
    command_buffer: &'a metal::CommandBufferRef,
    target: &'a metal::TextureRef,
    viewport_size: Size<DevicePixels>,
    configure_color_attachment: impl Fn(&RenderPassColorAttachmentDescriptorRef),
) -> &'a metal::RenderCommandEncoderRef {
    let render_pass_descriptor = metal::RenderPassDescriptor::new();
    let color_attachment = render_pass_descriptor
        .color_attachments()
        .object_at(0)
        .unwrap();
    color_attachment.set_texture(Some(target));
    color_attachment.set_store_action(metal::MTLStoreAction::Store);
    configure_color_attachment(color_attachment);

    let command_encoder = command_buffer.new_render_command_encoder(render_pass_descriptor);
    command_encoder.set_viewport(metal::MTLViewport {
        originX: 0.0,
        originY: 0.0,
        width: i32::from(viewport_size.width) as f64,
        height: i32::from(viewport_size.height) as f64,
        znear: 0.0,
        zfar: 1.0,
    });
    command_encoder
}

fn build_pipeline_state(
    device: &metal::DeviceRef,
    library: &metal::LibraryRef,
    label: &str,
    vertex_fn_name: &str,
    fragment_fn_name: &str,
    pixel_format: metal::MTLPixelFormat,
) -> metal::RenderPipelineState {
    let vertex_fn = library
        .get_function(vertex_fn_name, None)
        .expect("error locating vertex function");
    let fragment_fn = library
        .get_function(fragment_fn_name, None)
        .expect("error locating fragment function");

    let descriptor = metal::RenderPipelineDescriptor::new();
    descriptor.set_label(label);
    descriptor.set_vertex_function(Some(vertex_fn.as_ref()));
    descriptor.set_fragment_function(Some(fragment_fn.as_ref()));
    let color_attachment = descriptor.color_attachments().object_at(0).unwrap();
    color_attachment.set_pixel_format(pixel_format);
    color_attachment.set_blending_enabled(true);
    color_attachment.set_rgb_blend_operation(metal::MTLBlendOperation::Add);
    color_attachment.set_alpha_blend_operation(metal::MTLBlendOperation::Add);
    color_attachment.set_source_rgb_blend_factor(metal::MTLBlendFactor::SourceAlpha);
    color_attachment.set_source_alpha_blend_factor(metal::MTLBlendFactor::One);
    color_attachment.set_destination_rgb_blend_factor(metal::MTLBlendFactor::OneMinusSourceAlpha);
    color_attachment.set_destination_alpha_blend_factor(metal::MTLBlendFactor::One);

    device
        .new_render_pipeline_state(&descriptor)
        .expect("could not create render pipeline state")
}

fn build_path_sprite_pipeline_state(
    device: &metal::DeviceRef,
    library: &metal::LibraryRef,
    label: &str,
    vertex_fn_name: &str,
    fragment_fn_name: &str,
    pixel_format: metal::MTLPixelFormat,
) -> metal::RenderPipelineState {
    let vertex_fn = library
        .get_function(vertex_fn_name, None)
        .expect("error locating vertex function");
    let fragment_fn = library
        .get_function(fragment_fn_name, None)
        .expect("error locating fragment function");

    let descriptor = metal::RenderPipelineDescriptor::new();
    descriptor.set_label(label);
    descriptor.set_vertex_function(Some(vertex_fn.as_ref()));
    descriptor.set_fragment_function(Some(fragment_fn.as_ref()));
    let color_attachment = descriptor.color_attachments().object_at(0).unwrap();
    color_attachment.set_pixel_format(pixel_format);
    color_attachment.set_blending_enabled(true);
    color_attachment.set_rgb_blend_operation(metal::MTLBlendOperation::Add);
    color_attachment.set_alpha_blend_operation(metal::MTLBlendOperation::Add);
    color_attachment.set_source_rgb_blend_factor(metal::MTLBlendFactor::One);
    color_attachment.set_source_alpha_blend_factor(metal::MTLBlendFactor::One);
    color_attachment.set_destination_rgb_blend_factor(metal::MTLBlendFactor::OneMinusSourceAlpha);
    color_attachment.set_destination_alpha_blend_factor(metal::MTLBlendFactor::One);

    device
        .new_render_pipeline_state(&descriptor)
        .expect("could not create render pipeline state")
}

fn build_path_rasterization_pipeline_state(
    device: &metal::DeviceRef,
    library: &metal::LibraryRef,
    label: &str,
    vertex_fn_name: &str,
    fragment_fn_name: &str,
    pixel_format: metal::MTLPixelFormat,
    path_sample_count: u32,
) -> metal::RenderPipelineState {
    let vertex_fn = library
        .get_function(vertex_fn_name, None)
        .expect("error locating vertex function");
    let fragment_fn = library
        .get_function(fragment_fn_name, None)
        .expect("error locating fragment function");

    let descriptor = metal::RenderPipelineDescriptor::new();
    descriptor.set_label(label);
    descriptor.set_vertex_function(Some(vertex_fn.as_ref()));
    descriptor.set_fragment_function(Some(fragment_fn.as_ref()));
    if path_sample_count > 1 {
        descriptor.set_raster_sample_count(path_sample_count as _);
        descriptor.set_alpha_to_coverage_enabled(false);
    }
    let color_attachment = descriptor.color_attachments().object_at(0).unwrap();
    color_attachment.set_pixel_format(pixel_format);
    color_attachment.set_blending_enabled(true);
    color_attachment.set_rgb_blend_operation(metal::MTLBlendOperation::Add);
    color_attachment.set_alpha_blend_operation(metal::MTLBlendOperation::Add);
    color_attachment.set_source_rgb_blend_factor(metal::MTLBlendFactor::One);
    color_attachment.set_source_alpha_blend_factor(metal::MTLBlendFactor::One);
    color_attachment.set_destination_rgb_blend_factor(metal::MTLBlendFactor::OneMinusSourceAlpha);
    color_attachment.set_destination_alpha_blend_factor(metal::MTLBlendFactor::OneMinusSourceAlpha);

    device
        .new_render_pipeline_state(&descriptor)
        .expect("could not create render pipeline state")
}

/// Plays: the pipeline that halves what's under backdrops into a level,
/// replacing what was there.
fn build_backdrop_pass_pipeline_state(
    device: &metal::DeviceRef,
    library: &metal::LibraryRef,
) -> metal::RenderPipelineState {
    let vertex_fn = library
        .get_function("backdrop_pass_vertex", None)
        .expect("error locating vertex function");
    let fragment_fn = library
        .get_function("backdrop_pass_fragment", None)
        .expect("error locating fragment function");

    let descriptor = metal::RenderPipelineDescriptor::new();
    descriptor.set_label("backdrop_pass");
    descriptor.set_vertex_function(Some(vertex_fn.as_ref()));
    descriptor.set_fragment_function(Some(fragment_fn.as_ref()));
    let color_attachment = descriptor.color_attachments().object_at(0).unwrap();
    color_attachment.set_pixel_format(BACKDROP_LEVEL_FORMAT);
    color_attachment.set_blending_enabled(false);

    device
        .new_render_pipeline_state(&descriptor)
        .expect("could not create render pipeline state")
}

// Align to multiples of 256 make Metal happy.
fn align_offset(offset: &mut usize) {
    *offset = (*offset).div_ceil(256) * 256;
}

#[repr(C)]
enum ShadowInputIndex {
    Vertices = 0,
    Shadows = 1,
    ViewportSize = 2,
}

#[repr(C)]
enum QuadInputIndex {
    Vertices = 0,
    Quads = 1,
    ViewportSize = 2,
}

/// Plays: see `Visualizer`.
#[repr(C)]
enum VisualizerInputIndex {
    Vertices = 0,
    Visualizers = 1,
    ViewportSize = 2,
}

#[repr(C)]
enum UnderlineInputIndex {
    Vertices = 0,
    Underlines = 1,
    ViewportSize = 2,
}

#[repr(C)]
enum SpriteInputIndex {
    Vertices = 0,
    Sprites = 1,
    ViewportSize = 2,
    AtlasTextureSize = 3,
    AtlasTexture = 4,
}

#[repr(C)]
enum BackdropInputIndex {
    Vertices = 0,
    Backdrops = 1,
    ViewportSize = 2,
}

#[repr(C)]
enum BackdropPassInputIndex {
    Vertices = 0,
    Params = 1,
    Source = 2,
}

/// Plays: what a pass halving what's under backdrops makes, in the
/// viewport's pixels: `region` (origin and size), and `texels`, a texel of
/// the source and of the level it makes, then that level's size in texels.
#[repr(C)]
struct BackdropPassParams {
    region: [f32; 4],
    texels: [f32; 4],
}

#[repr(C)]
enum SurfaceInputIndex {
    Vertices = 0,
    Surfaces = 1,
    ViewportSize = 2,
    TextureSize = 3,
    YTexture = 4,
    CbCrTexture = 5,
    HdrParams = 6,
    DoviParams = 7,
    Gamma = 8,
}

/// Plays: whether a surface is HDR video, which comes as 10-bit YCbCr.
fn is_hdr(buffer: &CVPixelBuffer) -> bool {
    let format = buffer.get_pixel_format();
    format == kCVPixelFormatType_420YpCbCr10BiPlanarVideoRange
        || format == kCVPixelFormatType_420YpCbCr10BiPlanarFullRange
}

/// Plays: a Dolby Vision surface's reshaping, which `surface_dovi_fragment`
/// takes as it is. Checked against the size of `DoviParams`, the one thing
/// that can be checked here.
fn dolby_vision_params(buffer: &CVPixelBuffer) -> Option<CFData> {
    let key = CFString::from_static_string(DOLBY_VISION_ATTACHMENT);
    let value = unsafe {
        CVBufferGetAttachment(
            buffer.as_concrete_TypeRef(),
            key.as_concrete_TypeRef(),
            ptr::null_mut(),
        )
    };
    if value.is_null() {
        return None;
    }
    let data = unsafe { CFData::wrap_under_get_rule(value as CFDataRef) };
    (data.len() == DOVI_PARAMS_SIZE as isize).then_some(data)
}

/// Plays: the gamma a surface is drawn with, which Plays attaches to it.
fn surface_gamma(buffer: &CVPixelBuffer) -> f32 {
    let key = CFString::from_static_string(GAMMA_ATTACHMENT);
    let value = unsafe {
        CVBufferGetAttachment(
            buffer.as_concrete_TypeRef(),
            key.as_concrete_TypeRef(),
            ptr::null_mut(),
        )
    };
    if value.is_null() {
        return 1.;
    }
    let number = unsafe { CFNumber::wrap_under_get_rule(value as CFNumberRef) };
    number
        .to_f32()
        .filter(|gamma| gamma.is_finite() && *gamma > 0.)
        .unwrap_or(1.)
}

/// Plays: the size of `DoviParams` in shaders.metal: six rows of matrices,
/// then three curves of three `float4`s and eight pieces of seven.
const DOVI_PARAMS_SIZE: usize = 16 * (6 + 3 * (3 + 8 * 7));

/// Plays: how `surface_hdr_fragment` turns an HDR surface into light, read
/// from the color tags on its pixel buffer. Matches `HdrSurfaceParams` in
/// shaders.metal.
#[repr(C)]
struct HdrSurfaceParams {
    /// Columns of a matrix from 16-bit texture samples (y, cb, cr, 1) to
    /// nonlinear R'G'B' in 0..1.
    ycbcr_to_rgb: [[f32; 4]; 4],
    /// Columns of a matrix from the video's linear RGB to BT.709/sRGB's.
    to_srgb_primaries: [[f32; 4]; 4],
    /// 1 for PQ, 2 for HLG, 0 for anything else (taken as BT.1886 SDR).
    transfer: u32,
    edr_headroom: f32,
    _padding: [f32; 2],
}

impl HdrSurfaceParams {
    fn new(buffer: &CVPixelBuffer, edr_headroom: f32) -> Self {
        // ITU-T H.273 code points.
        let matrix = code_point(buffer, unsafe { kCVImageBufferYCbCrMatrixKey }, |s| unsafe {
            CVYCbCrMatrixGetIntegerCodePointForString(s)
        });
        let primaries =
            code_point(buffer, unsafe { kCVImageBufferColorPrimariesKey }, |s| unsafe {
                CVColorPrimariesGetIntegerCodePointForString(s)
            });
        let transfer =
            code_point(buffer, unsafe { kCVImageBufferTransferFunctionKey }, |s| unsafe {
                CVTransferFunctionGetIntegerCodePointForString(s)
            });
        let (kr, kb) = match matrix {
            Some(1) => (0.2126, 0.0722),
            Some(5) | Some(6) => (0.299, 0.114),
            _ => (0.2627, 0.0593),
        };
        let full_range =
            buffer.get_pixel_format() == kCVPixelFormatType_420YpCbCr10BiPlanarFullRange;
        let identity = [
            [1., 0., 0., 0.],
            [0., 1., 0., 0.],
            [0., 0., 1., 0.],
            [0., 0., 0., 1.],
        ];
        Self {
            ycbcr_to_rgb: ycbcr_to_rgb(kr, kb, full_range),
            to_srgb_primaries: match primaries {
                Some(1) => identity,
                Some(12) => P3_TO_SRGB,
                _ => BT2020_TO_SRGB,
            },
            transfer: match transfer {
                Some(16) => 1,
                Some(18) => 2,
                _ => 0,
            },
            edr_headroom,
            _padding: [0.; 2],
        }
    }
}

/// Plays: linear BT.2020 RGB to linear sRGB RGB, as matrix columns.
const BT2020_TO_SRGB: [[f32; 4]; 4] = [
    [1.660_491, -0.124_550, -0.018_151, 0.],
    [-0.587_641, 1.132_900, -0.100_579, 0.],
    [-0.072_850, -0.008_349, 1.118_730, 0.],
    [0., 0., 0., 1.],
];

/// Plays: linear Display P3 RGB to linear sRGB RGB, as matrix columns.
const P3_TO_SRGB: [[f32; 4]; 4] = [
    [1.224_940, -0.042_057, -0.019_638, 0.],
    [-0.224_940, 1.042_057, -0.078_636, 0.],
    [0., 0., 1.098_274, 0.],
    [0., 0., 0., 1.],
];

/// Plays: a pixel buffer's color tag as an ITU-T H.273 code point.
fn code_point(
    buffer: &CVPixelBuffer,
    key: CFStringRef,
    to_code_point: impl Fn(CFStringRef) -> i32,
) -> Option<i32> {
    let value = unsafe {
        CVBufferGetAttachment(buffer.as_concrete_TypeRef(), key, ptr::null_mut())
    };
    (!value.is_null()).then(|| to_code_point(value as CFStringRef))
}

/// Plays: the matrix from 10-bit YCbCr, read as 16-bit unorm samples (the 10
/// bits are the top ones), to nonlinear R'G'B', for the given luma weights.
fn ycbcr_to_rgb(kr: f32, kb: f32, full_range: bool) -> [[f32; 4]; 4] {
    let kg = 1. - kr - kb;
    // A 16-bit sample of 10-bit code c reads as c * 64 / 65535.
    let to_code = 65535. / 64.;
    let (y_scale, y_offset, c_scale, c_offset) = if full_range {
        (to_code / 1023., 0., to_code / 1023., -512. / 1023.)
    } else {
        (to_code / 876., -64. / 876., to_code / 896., -512. / 896.)
    };
    let cb = [0., -2. * kb * (1. - kb) / kg, 2. * (1. - kb)];
    let cr = [2. * (1. - kr), -2. * kr * (1. - kr) / kg, 0.];
    let offset = |i: usize| y_offset + c_offset * (cb[i] + cr[i]);
    [
        [y_scale, y_scale, y_scale, 0.],
        [cb[0] * c_scale, cb[1] * c_scale, cb[2] * c_scale, 0.],
        [cr[0] * c_scale, cr[1] * c_scale, cr[2] * c_scale, 0.],
        [offset(0), offset(1), offset(2), 1.],
    ]
}

#[repr(C)]
enum PathRasterizationInputIndex {
    Vertices = 0,
    ViewportSize = 1,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct PathSprite {
    pub bounds: Bounds<ScaledPixels>,
}

#[derive(Clone, Debug, PartialEq)]
#[repr(C)]
pub struct SurfaceBounds {
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    /// Plays: the part of the surface shown, and turned or mirrored how.
    pub content: ContentTransform,
}

/// Plays: draws `quads` (each its bounds and color), then `visualizers`
/// (each its bounds, shader and color), then `backdrops` (each its bounds,
/// corners and style) over them, with the renderer windows use but off
/// screen, at a scale of 1, and reads back the `size` pixels it made, row
/// by row, as the drawable has them: extended sRGB, unmultiplied.
#[cfg(any(test, feature = "test-support"))]
pub fn render_offscreen(
    size: Size<DevicePixels>,
    quads: &[(Bounds<crate::Pixels>, crate::Background)],
    visualizers: &[(
        Bounds<crate::Pixels>,
        crate::VisualizerShader,
        crate::Hsla,
    )],
    backdrops: &[(
        Bounds<crate::Pixels>,
        crate::Corners<crate::Pixels>,
        crate::BackdropStyle,
    )],
) -> Vec<[f32; 4]> {
    let mut renderer = MetalRenderer::new(Arc::new(Mutex::new(InstanceBufferPool::default())));
    let viewport = Bounds::new(
        point(crate::px(0.), crate::px(0.)),
        size.map(|v| crate::px(v.0 as f32)),
    );
    let content_mask = ContentMask { bounds: viewport };
    let mut scene = Scene::default();
    for (bounds, background) in quads {
        scene.insert_primitive(Quad {
            order: 0,
            border_style: Default::default(),
            bounds: bounds.scale(1.),
            content_mask: content_mask.scale(1.),
            background: *background,
            border_color: Default::default(),
            corner_radii: Default::default(),
            border_widths: Default::default(),
            corner_smoothing: 0.,
            pad: 0,
        });
    }
    for (bounds, shader, color) in visualizers {
        let (kind, params) = shader.encode(1.);
        scene.insert_primitive(Visualizer {
            order: 0,
            kind,
            bounds: bounds.scale(1.),
            content_mask: content_mask.scale(1.),
            color: *color,
            params,
        });
    }
    for (bounds, corners, style) in backdrops {
        scene.insert_primitive(Backdrop::new(
            *bounds,
            *corners,
            crate::styled::SUPERELLIPSE_SMOOTHING,
            *style,
            content_mask.clone(),
            1.,
            1.,
        ));
    }
    scene.finish();

    let descriptor = metal::TextureDescriptor::new();
    descriptor.set_width(size.width.0 as u64);
    descriptor.set_height(size.height.0 as u64);
    descriptor.set_pixel_format(DRAWABLE_FORMAT);
    descriptor.set_storage_mode(metal::MTLStorageMode::Managed);
    descriptor.set_usage(metal::MTLTextureUsage::RenderTarget | metal::MTLTextureUsage::ShaderRead);
    let target = renderer.device.new_texture(&descriptor);

    let mut instance_buffer = renderer
        .instance_buffer_pool
        .lock()
        .acquire(&renderer.device);
    let command_buffer = renderer
        .draw_primitives(&scene, &mut instance_buffer, &target, size)
        .expect("the scene fits");
    let blit = command_buffer.new_blit_command_encoder();
    blit.synchronize_resource(&target);
    blit.end_encoding();
    command_buffer.commit();
    command_buffer.wait_until_completed();

    let (width, height) = (size.width.0 as usize, size.height.0 as usize);
    let mut halves = vec![0u16; width * height * 4];
    target.get_bytes(
        halves.as_mut_ptr() as *mut c_void,
        (width * 8) as u64,
        metal::MTLRegion::new_2d(0, 0, width as u64, height as u64),
        0,
    );
    halves
        .chunks_exact(4)
        .map(|pixel| {
            let [r, g, b, a] = [pixel[0], pixel[1], pixel[2], pixel[3]].map(half_to_f32);
            // Quads blend as unmultiplied colors; so do backdrops.
            [r, g, b, a]
        })
        .collect()
}

/// Plays: an IEEE half float as an f32.
#[cfg(any(test, feature = "test-support"))]
fn half_to_f32(bits: u16) -> f32 {
    let sign = if bits & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exponent = ((bits >> 10) & 0x1f) as i32;
    let mantissa = (bits & 0x3ff) as f32;
    sign * match exponent {
        0 => mantissa * 2f32.powi(-24),
        31 => f32::INFINITY,
        _ => (1.0 + mantissa / 1024.0) * 2f32.powi(exponent - 15),
    }
}
