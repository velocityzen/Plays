//! Zero-copy video on macOS: frames decoded by VideoToolbox stay in GPU memory
//! and gpui draws them with its `surface()` element, instead of being copied
//! to memory, converted to BGRA and uploaded again as a texture.
//!
//! gpui draws surfaces from full-range NV12 (`420f`) pixel buffers with a fixed
//! BT.601 matrix, while VideoToolbox decodes to video-range buffers in the
//! video's own matrix (usually BT.709 for HD). A `VTPixelTransferSession`
//! converts between the two on the GPU, scaling to the display size on the
//! way, into IOSurface-backed buffers from a pool.
//!
//! HDR video (PQ or HLG) goes to 10-bit BT.2020 buffers (`x420`) instead,
//! tagged with its primaries and transfer function. Plays' copy of gpui
//! (vendor/gpui) draws those with its HDR shader, brighter than SDR white
//! where the screen allows.
//!
//! Dolby Vision profile 5 isn't YCbCr, so it must not be converted at all:
//! its frames are only scaled, into buffers of their own format, and carry
//! the frame's reshaping (`dovi`) for gpui's Dolby Vision shader under
//! [`DOLBY_VISION_ATTACHMENT`].

use std::ffi::c_void;
use std::sync::atomic::{AtomicU64, Ordering};
use std::{ptr, slice};

use anyhow::{Context as _, Result, anyhow, bail, ensure};
use core_foundation::base::{CFRelease, CFType, CFTypeRef, OSStatus, TCFType, kCFAllocatorDefault};
use core_foundation::boolean::CFBoolean;
use core_foundation::data::CFData;
use core_foundation::dictionary::CFDictionary;
use core_foundation::number::CFNumber;
use core_foundation::string::{CFString, CFStringRef};
use core_video::buffer::{
    CVBufferHasAttachment, CVBufferPropagateAttachments, CVBufferSetAttachment, TCVBuffer,
    kCVAttachmentMode_ShouldNotPropagate, kCVAttachmentMode_ShouldPropagate,
};
use core_video::image_buffer::{
    CVImageBufferColorPrimaries, CVImageBufferKeys, CVImageBufferTransferFunction,
    CVImageBufferYCbCrMatrix,
};
use core_video::pixel_buffer::{
    CVPixelBuffer, CVPixelBufferGetPixelFormatType, CVPixelBufferKeys, CVPixelBufferRef,
    kCVPixelBufferLock_ReadOnly, kCVPixelFormatType_420YpCbCr8BiPlanarFullRange,
    kCVPixelFormatType_420YpCbCr10BiPlanarFullRange,
    kCVPixelFormatType_420YpCbCr10BiPlanarVideoRange,
};
use core_video::pixel_buffer_pool::CVPixelBufferPool;
use ffmpeg::ffi;
use ffmpeg::frame;
use ffmpeg::util::color;
use ffmpeg::util::format::Pixel;
use ffmpeg_next as ffmpeg;

use super::colors::Colors;
use super::dovi::Reshaping;
use super::hdr::Transfer;
use super::{picture, still};

/// The pixel buffer attachment (a `CFData` of `dovi::ShaderParams`) that
/// makes vendor/gpui draw a surface as Dolby Vision. Matches
/// `DOLBY_VISION_ATTACHMENT` in its metal_renderer.rs.
pub const DOLBY_VISION_ATTACHMENT: &str = "PlaysDolbyVision";
/// The pixel buffer attachment (a `CFNumber`) with the gamma vendor/gpui
/// draws a surface with. Matches `GAMMA_ATTACHMENT` in its metal_renderer.rs.
pub(super) const GAMMA_ATTACHMENT: &str = "PlaysGamma";

type VTPixelTransferSessionRef = *mut c_void;

#[link(name = "VideoToolbox", kind = "framework")]
unsafe extern "C" {
    fn VTPixelTransferSessionCreate(
        allocator: *const c_void,
        session: *mut VTPixelTransferSessionRef,
    ) -> OSStatus;
    fn VTPixelTransferSessionTransferImage(
        session: VTPixelTransferSessionRef,
        source: CVPixelBufferRef,
        destination: CVPixelBufferRef,
    ) -> OSStatus;
    fn VTPixelTransferSessionInvalidate(session: VTPixelTransferSessionRef);
    fn VTSessionSetProperty(session: CFTypeRef, key: CFStringRef, value: CFTypeRef) -> OSStatus;

    static kVTPixelTransferPropertyKey_DestinationYCbCrMatrix: CFStringRef;
    static kVTPixelTransferPropertyKey_RealTime: CFStringRef;
}

/// A decoded picture in GPU memory, in the format gpui's `surface()` draws.
#[derive(Clone)]
pub struct Surface {
    buffer: CVPixelBuffer,
    id: u64,
    hdr: bool,
    /// How an HDR frame's samples encode light; none for Dolby Vision.
    transfer: Option<Transfer>,
    dolby_vision: bool,
}

// SAFETY: CoreVideo buffers are reference counted thread-safely, and nothing
// writes to the pixels after the transfer that fills them. The one thing
// written later is the gamma attachment, by `for_drawing` alone: on the main
// thread as it paints, which stops while Picture in Picture hands the buffer
// to AVKit, and only when the gamma changed.
unsafe impl Send for Surface {}
// SAFETY: as for `Send`.
unsafe impl Sync for Surface {}

impl Surface {
    /// Unique for the life of the process (pooled buffers are reused, so the
    /// buffer's address isn't).
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Its width and height in pixels.
    pub fn size(&self) -> (u32, u32) {
        (
            self.buffer.get_width() as u32,
            self.buffer.get_height() as u32,
        )
    }

    /// The buffer as it is, tagged with its colors: for Picture in
    /// Picture, and to read back in tests.
    pub fn buffer(&self) -> &CVPixelBuffer {
        &self.buffer
    }

    /// Dolby Vision, which only gpui's own shader shows right.
    pub fn is_dolby_vision(&self) -> bool {
        self.dolby_vision
    }

    /// The gamma it is drawn with now, for its kind of video.
    pub fn gamma(&self) -> f32 {
        picture::gamma(self.hdr)
    }

    /// The buffer to draw now, with the current gamma for its kind of video
    /// attached. Set as it is drawn, so a change shows on every surface at
    /// once, paused ones included, and only when it changed.
    pub fn for_drawing(&self) -> CVPixelBuffer {
        let key = CFString::from_static_string(GAMMA_ATTACHMENT);
        let gamma = self.gamma();
        let attached = self
            .buffer
            .as_buffer()
            .get_attachment(&key, None)
            .and_then(|value| value.downcast::<CFNumber>())
            .and_then(|number| number.to_f32());
        if attached != Some(gamma) {
            let gamma = CFNumber::from(gamma);
            // SAFETY: the buffer is valid; gpui reads attachments on this
            // thread, when it draws.
            unsafe {
                CVBufferSetAttachment(
                    self.buffer.as_concrete_TypeRef(),
                    key.as_concrete_TypeRef(),
                    gamma.as_CFTypeRef(),
                    kCVAttachmentMode_ShouldNotPropagate,
                );
            }
        }
        self.buffer.clone()
    }

    /// Its samples, read back from GPU memory into a frame tagged with the
    /// colors they're drawn in: for a screenshot of what shows.
    pub fn read(&self) -> Result<frame::Video> {
        ensure!(
            !self.dolby_vision,
            "Dolby Vision can't be read back as it shows"
        );
        let colors = |range, space, primaries, transfer| Colors {
            range,
            space,
            primaries,
            transfer,
        };
        let buffer = &self.buffer;
        // CoreVideo's names, as they are.
        #[allow(non_upper_case_globals)]
        let (format, colors) = match buffer.get_pixel_format() {
            // gpui draws these with BT.601's matrix.
            kCVPixelFormatType_420YpCbCr8BiPlanarFullRange => (
                Pixel::NV12,
                colors(
                    color::Range::JPEG,
                    color::Space::SMPTE170M,
                    color::Primaries::Unspecified,
                    color::TransferCharacteristic::Unspecified,
                ),
            ),
            kCVPixelFormatType_420YpCbCr10BiPlanarVideoRange => (
                Pixel::P010LE,
                colors(
                    color::Range::MPEG,
                    color::Space::BT2020NCL,
                    color::Primaries::BT2020,
                    match self.transfer {
                        Some(Transfer::Pq) => color::TransferCharacteristic::SMPTE2084,
                        Some(Transfer::Hlg) => color::TransferCharacteristic::ARIB_STD_B67,
                        None => color::TransferCharacteristic::Unspecified,
                    },
                ),
            ),
            format => bail!("a surface in pixel format {format:#x}"),
        };
        let status = buffer.lock_base_address(kCVPixelBufferLock_ReadOnly);
        ensure!(status == 0, "couldn't lock a pixel buffer (error {status})");
        let planes: Option<Vec<(&[u8], usize)>> = (0..2)
            // SAFETY: locked, a two-plane buffer's planes are their rows
            // times the bytes a row, which nothing writes to after the
            // transfer that filled them.
            .map(|index| unsafe {
                let base = buffer.get_base_address_of_plane(index).cast::<u8>();
                let stride = buffer.get_bytes_per_row_of_plane(index);
                let rows = buffer.get_height_of_plane(index);
                (!base.is_null()).then(|| (slice::from_raw_parts(base, stride * rows), stride))
            })
            .collect();
        let frame = planes
            .context("a pixel buffer has no memory")
            .and_then(|planes| still::frame_of(format, self.size(), &planes, colors));
        buffer.unlock_base_address(kCVPixelBufferLock_ReadOnly);
        frame
    }
}

/// Converts VideoToolbox frames into surfaces. One per decoder.
pub struct Converter {
    session: VTPixelTransferSessionRef,
    /// What the session is set up for: HDR frames or not.
    hdr: Option<bool>,
    pool: Option<Pool>,
}

struct Pool {
    pool: CVPixelBufferPool,
    width: u32,
    height: u32,
    format: u32,
}

impl Converter {
    pub fn new() -> Result<Self> {
        let mut session = ptr::null_mut();
        // SAFETY: `session` receives a new reference or stays null.
        let status = unsafe { VTPixelTransferSessionCreate(kCFAllocatorDefault, &mut session) };
        if status != 0 || session.is_null() {
            bail!("couldn't create a pixel transfer session (error {status})");
        }
        let converter = Self {
            session,
            hdr: None,
            pool: None,
        };
        // SAFETY: the key is a VideoToolbox constant.
        unsafe {
            // Only a hint; older systems may not know it.
            let _ = converter.set(
                kVTPixelTransferPropertyKey_RealTime,
                CFBoolean::true_value().as_CFTypeRef(),
            );
        }
        Ok(converter)
    }

    /// Set the session up for HDR frames or SDR ones: the matrix to convert to
    /// is the one gpui's shader for that kind of surface expects.
    fn set_hdr(&mut self, hdr: bool) -> Result<()> {
        if self.hdr == Some(hdr) {
            return Ok(());
        }
        let matrix = CFString::from(if hdr {
            CVImageBufferYCbCrMatrix::ITU_R_2020
        } else {
            CVImageBufferYCbCrMatrix::ITU_R_601_4
        });
        // SAFETY: the key is a VideoToolbox constant.
        unsafe {
            self.set(
                kVTPixelTransferPropertyKey_DestinationYCbCrMatrix,
                matrix.as_CFTypeRef(),
            )?;
        }
        self.hdr = Some(hdr);
        Ok(())
    }

    fn set(&self, key: CFStringRef, value: CFTypeRef) -> Result<()> {
        // SAFETY: the session is valid until dropped; the value is retained by it.
        let status = unsafe { VTSessionSetProperty(self.session, key, value) };
        if status != 0 {
            bail!("couldn't configure the pixel transfer session (error {status})");
        }
        Ok(())
    }

    /// Convert a VideoToolbox frame into a `width` x `height` surface.
    pub fn convert(&mut self, frame: &frame::Video, width: u32, height: u32) -> Result<Surface> {
        // SAFETY: reading fields of a valid frame, through the pointer,
        // never a reference: it holds the file's numbers, which its Rust
        // enums may have no variant for.
        let (format, source) = unsafe {
            let raw = frame.as_ptr();
            ((*raw).format, (*raw).data[3] as CVPixelBufferRef)
        };
        if format != ffi::AVPixelFormat::AV_PIX_FMT_VIDEOTOOLBOX as i32 || source.is_null() {
            bail!("not a VideoToolbox frame");
        }
        let reshaping = Reshaping::of(frame);
        let transfer = Transfer::of(frame).filter(|_| reshaping.is_none());
        let hdr = reshaping.is_some() || transfer.is_some();
        let dolby_vision = reshaping.is_some();
        let format = if reshaping.is_some() {
            // SAFETY: the frame keeps the buffer alive.
            let format = unsafe { CVPixelBufferGetPixelFormatType(source) };
            if format != kCVPixelFormatType_420YpCbCr10BiPlanarVideoRange
                && format != kCVPixelFormatType_420YpCbCr10BiPlanarFullRange
            {
                bail!("Dolby Vision frames came in pixel format {format:#x}");
            }
            format
        } else if hdr {
            kCVPixelFormatType_420YpCbCr10BiPlanarVideoRange
        } else {
            kCVPixelFormatType_420YpCbCr8BiPlanarFullRange
        };
        if reshaping.is_some() {
            tag_bt2020_pq(source);
        } else {
            tag_colors(source, frame);
        }
        self.set_hdr(hdr)?;

        let destination = self
            .pool(width, height, format)?
            .create_pixel_buffer()
            .map_err(|status| anyhow!("couldn't get a pixel buffer (error {status})"))?;
        // SAFETY: both buffers are valid; the frame keeps the source alive.
        let status = unsafe {
            VTPixelTransferSessionTransferImage(
                self.session,
                source,
                destination.as_concrete_TypeRef(),
            )
        };
        if status != 0 {
            bail!("converting a frame on the GPU failed (error {status})");
        }
        if let Some(reshaping) = reshaping {
            tag_dolby_vision(&destination, &reshaping);
        } else {
            tag_converted(source, &destination, hdr);
        }
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        Ok(Surface {
            buffer: destination,
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            hdr,
            transfer,
            dolby_vision,
        })
    }

    /// Buffers for `width` x `height` surfaces in pixel format `format`,
    /// made again when any of them changes.
    fn pool(&mut self, width: u32, height: u32, format: u32) -> Result<&CVPixelBufferPool> {
        if self
            .pool
            .as_ref()
            .is_none_or(|p| (p.width, p.height, p.format) != (width, height, format))
        {
            let number = |n: i64| CFNumber::from(n).as_CFType();
            let no_properties = CFDictionary::<CFString, CFType>::from_CFType_pairs(&[]);
            let attributes = CFDictionary::from_CFType_pairs(&[
                (
                    CFString::from(CVPixelBufferKeys::PixelFormatType),
                    number(format.into()),
                ),
                (
                    CFString::from(CVPixelBufferKeys::Width),
                    number(width.into()),
                ),
                (
                    CFString::from(CVPixelBufferKeys::Height),
                    number(height.into()),
                ),
                // Backed by an IOSurface, so Metal can use it without a copy.
                (
                    CFString::from(CVPixelBufferKeys::IOSurfaceProperties),
                    no_properties.as_CFType(),
                ),
                (
                    CFString::from(CVPixelBufferKeys::MetalCompatibility),
                    CFBoolean::true_value().as_CFType(),
                ),
            ]);
            let pool = CVPixelBufferPool::new(None, Some(&attributes)).map_err(|status| {
                anyhow!("couldn't create a pixel buffer pool (error {status})")
            })?;
            self.pool = Some(Pool {
                pool,
                width,
                height,
                format,
            });
        }
        Ok(&self.pool.as_ref().expect("pool was just created").pool)
    }
}

impl Drop for Converter {
    fn drop(&mut self) {
        // SAFETY: the session is valid and owned by us.
        unsafe {
            VTPixelTransferSessionInvalidate(self.session);
            CFRelease(self.session as CFTypeRef);
        }
    }
}

/// Give a surface the source's color tags (primaries, transfer function and
/// HDR metadata), with the matrix the session converted to. gpui's HDR
/// shader reads them; Picture in Picture shows any surface by them.
fn tag_converted(source: CVPixelBufferRef, destination: &CVPixelBuffer, hdr: bool) {
    let matrix_key = CFString::from(CVImageBufferKeys::YCbCrMatrix);
    let matrix = CFString::from(if hdr {
        CVImageBufferYCbCrMatrix::ITU_R_2020
    } else {
        CVImageBufferYCbCrMatrix::ITU_R_601_4
    });
    // SAFETY: both buffers are valid, and nothing reads the destination yet.
    unsafe {
        CVBufferPropagateAttachments(source, destination.as_concrete_TypeRef());
        CVBufferSetAttachment(
            destination.as_concrete_TypeRef(),
            matrix_key.as_concrete_TypeRef(),
            matrix.as_CFTypeRef(),
            kCVAttachmentMode_ShouldPropagate,
        );
    }
}

/// Tag a Dolby Vision buffer as BT.2020 PQ. On the source, its matrix is
/// the one the session converts to, so the transfer leaves the samples be.
/// On a surface, it tells gpui to show the light that comes out of the
/// reshaping as such.
fn tag_bt2020_pq(buffer: CVPixelBufferRef) {
    let tags = [
        (
            CVImageBufferKeys::YCbCrMatrix,
            CFString::from(CVImageBufferYCbCrMatrix::ITU_R_2020),
        ),
        (
            CVImageBufferKeys::ColorPrimaries,
            CFString::from(CVImageBufferColorPrimaries::ITU_R_2020),
        ),
        (
            CVImageBufferKeys::TransferFunction,
            CFString::from(CVImageBufferTransferFunction::SMPTE_ST_2084_PQ),
        ),
    ];
    for (key, value) in tags {
        // SAFETY: the buffer is valid, and nothing reads it during this.
        unsafe {
            CVBufferSetAttachment(
                buffer,
                CFString::from(key).as_concrete_TypeRef(),
                value.as_CFTypeRef(),
                kCVAttachmentMode_ShouldPropagate,
            );
        }
    }
}

/// Give a Dolby Vision surface its tags and the frame's reshaping.
fn tag_dolby_vision(destination: &CVPixelBuffer, reshaping: &Reshaping) {
    let buffer = destination.as_concrete_TypeRef();
    tag_bt2020_pq(buffer);
    let key = CFString::from_static_string(DOLBY_VISION_ATTACHMENT);
    let params = CFData::from_buffer(reshaping.shader_params().as_bytes());
    // SAFETY: the buffer is valid, and nothing reads it yet.
    unsafe {
        CVBufferSetAttachment(
            buffer,
            key.as_concrete_TypeRef(),
            params.as_CFTypeRef(),
            kCVAttachmentMode_ShouldNotPropagate,
        );
    }
}

/// Describe the frame's colors on its pixel buffer, which is what the transfer
/// session converts from. Videos that don't say which matrix they use get the
/// same guess as in software (`decoder::set_colorspace`): BT.709 for HD.
fn tag_colors(source: CVPixelBufferRef, frame: &frame::Video) {
    let matrix_key = CFString::from(CVImageBufferKeys::YCbCrMatrix);
    // SAFETY: the buffer and frame are valid; FFmpeg only sets attachments.
    unsafe {
        ffi::av_vt_pixbuf_set_attachments(ptr::null_mut(), source.cast(), frame.as_ptr());
        if CVBufferHasAttachment(source, matrix_key.as_concrete_TypeRef()) == 0 {
            let matrix = CFString::from(if frame.height() >= 720 {
                CVImageBufferYCbCrMatrix::ITU_R_709_2
            } else {
                CVImageBufferYCbCrMatrix::ITU_R_601_4
            });
            CVBufferSetAttachment(
                source,
                matrix_key.as_concrete_TypeRef(),
                matrix.as_CFTypeRef(),
                kCVAttachmentMode_ShouldPropagate,
            );
        }
    }
}
