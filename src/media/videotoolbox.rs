//! Zero-copy video on macOS: frames decoded by VideoToolbox stay in GPU memory
//! and gpui draws them with its `surface()` element, instead of being copied
//! to memory, converted to BGRA and uploaded again as a texture.
//!
//! gpui draws surfaces from full-range NV12 (`420f`) pixel buffers with a fixed
//! BT.601 matrix, while VideoToolbox decodes to video-range buffers in the
//! video's own matrix (usually BT.709 for HD). A `VTPixelTransferSession`
//! converts between the two on the GPU, scaling to the display size on the
//! way, into IOSurface-backed buffers from a pool.

use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Result, anyhow, bail};
use core_foundation::base::{CFRelease, CFType, CFTypeRef, OSStatus, TCFType, kCFAllocatorDefault};
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::CFDictionary;
use core_foundation::number::CFNumber;
use core_foundation::string::{CFString, CFStringRef};
use core_video::buffer::{
    CVBufferHasAttachment, CVBufferSetAttachment, kCVAttachmentMode_ShouldPropagate,
};
use core_video::image_buffer::{CVImageBufferKeys, CVImageBufferYCbCrMatrix};
use core_video::pixel_buffer::{
    CVPixelBuffer, CVPixelBufferKeys, CVPixelBufferRef,
    kCVPixelFormatType_420YpCbCr8BiPlanarFullRange,
};
use core_video::pixel_buffer_pool::CVPixelBufferPool;
use ffmpeg::ffi;
use ffmpeg::frame;
use ffmpeg_next as ffmpeg;

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
}

// SAFETY: CoreVideo buffers are reference counted thread-safely, and nothing
// writes to the pixels after the transfer that fills them.
unsafe impl Send for Surface {}
unsafe impl Sync for Surface {}

impl Surface {
    /// Unique for the life of the process (pooled buffers are reused, so the
    /// buffer's address isn't).
    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn buffer(&self) -> &CVPixelBuffer {
        &self.buffer
    }
}

/// Converts VideoToolbox frames into surfaces. One per decoder.
pub struct Converter {
    session: VTPixelTransferSessionRef,
    pool: Option<Pool>,
}

struct Pool {
    pool: CVPixelBufferPool,
    width: u32,
    height: u32,
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
            pool: None,
        };
        // The matrix gpui's surface shader converts with.
        let matrix = CFString::from(CVImageBufferYCbCrMatrix::ITU_R_601_4);
        // SAFETY: the keys are VideoToolbox constants.
        unsafe {
            converter.set(
                kVTPixelTransferPropertyKey_DestinationYCbCrMatrix,
                matrix.as_CFTypeRef(),
            )?;
            // Only a hint; older systems may not know it.
            let _ = converter.set(
                kVTPixelTransferPropertyKey_RealTime,
                CFBoolean::true_value().as_CFTypeRef(),
            );
        }
        Ok(converter)
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
        // SAFETY: reading fields of a valid frame.
        let (format, source) = unsafe {
            let frame = &*frame.as_ptr();
            (frame.format, frame.data[3] as CVPixelBufferRef)
        };
        if format != ffi::AVPixelFormat::AV_PIX_FMT_VIDEOTOOLBOX as i32 || source.is_null() {
            bail!("not a VideoToolbox frame");
        }
        tag_colors(source, frame);

        let destination = self
            .pool(width, height)?
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
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        Ok(Surface {
            buffer: destination,
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
        })
    }

    /// Buffers for `width` x `height` surfaces, made again when the size changes.
    fn pool(&mut self, width: u32, height: u32) -> Result<&CVPixelBufferPool> {
        if self
            .pool
            .as_ref()
            .is_none_or(|p| (p.width, p.height) != (width, height))
        {
            let number = |n: i64| CFNumber::from(n).as_CFType();
            let no_properties = CFDictionary::<CFString, CFType>::from_CFType_pairs(&[]);
            let attributes = CFDictionary::from_CFType_pairs(&[
                (
                    CFString::from(CVPixelBufferKeys::PixelFormatType),
                    number(kCVPixelFormatType_420YpCbCr8BiPlanarFullRange.into()),
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
