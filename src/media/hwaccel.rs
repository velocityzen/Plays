//! Hardware video decoding through FFmpeg's hwaccel API: VideoToolbox on macOS;
//! VA-API, NVDEC (CUDA) or VDPAU on Linux, whichever works first.
//!
//! On macOS, frames stay on the GPU and are drawn from there (see
//! `videotoolbox`). Elsewhere, and whenever that isn't possible, they are
//! copied back to memory and converted like software-decoded frames.
//!
//! FFmpeg picks libdav1d for AV1, which decodes only in software; its own
//! AV1 decoder decodes only on the GPU, and is taken instead where the GPU
//! can.

use std::collections::HashMap;
use std::ptr;
use std::sync::{Mutex, OnceLock};

use anyhow::{Result, bail};
use ffmpeg::ffi;
use ffmpeg::{Codec, codec, frame};
use ffmpeg_next as ffmpeg;

use ffi::AVHWDeviceType as Kind;

/// Device types to try, best first.
#[cfg(target_os = "macos")]
const KINDS: &[Kind] = &[Kind::AV_HWDEVICE_TYPE_VIDEOTOOLBOX];
#[cfg(not(target_os = "macos"))]
const KINDS: &[Kind] = &[
    Kind::AV_HWDEVICE_TYPE_VAAPI,
    Kind::AV_HWDEVICE_TYPE_CUDA,
    Kind::AV_HWDEVICE_TYPE_VDPAU,
];

/// A device context, created once and shared by every decoder of its type
/// (like the ffmpeg command line tool does). Never freed.
struct Device(*mut ffi::AVBufferRef);

// SAFETY: FFmpeg device contexts may be used from several decoders on
// different threads; we only ever hand out new references to it.
unsafe impl Send for Device {}

/// Opened devices by type; `None` records a type that failed to open, so it
/// isn't retried (and doesn't log an error) for every file.
static DEVICES: OnceLock<Mutex<HashMap<u32, Option<Device>>>> = OnceLock::new();

fn device(kind: Kind) -> Option<*mut ffi::AVBufferRef> {
    let mut devices = DEVICES
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    devices
        .entry(kind as u32)
        .or_insert_with(|| {
            let mut device = ptr::null_mut();
            // SAFETY: `device` receives a new reference or stays null.
            let result = unsafe {
                ffi::av_hwdevice_ctx_create(&mut device, kind, ptr::null(), ptr::null_mut(), 0)
            };
            if result < 0 || device.is_null() {
                log::info!(
                    "{} isn't available: {}",
                    api_name(kind),
                    ffmpeg::Error::from(result)
                );
                None
            } else {
                log::info!("hardware decoding with {} is available", api_name(kind));
                Some(Device(device))
            }
        })
        .as_ref()
        .map(|device| device.0)
}

fn api_name(kind: Kind) -> &'static str {
    match kind {
        Kind::AV_HWDEVICE_TYPE_VIDEOTOOLBOX => "VideoToolbox",
        Kind::AV_HWDEVICE_TYPE_VAAPI => "VA-API",
        Kind::AV_HWDEVICE_TYPE_CUDA => "NVDEC",
        Kind::AV_HWDEVICE_TYPE_VDPAU => "VDPAU",
        _ => "hardware decoding",
    }
}

/// Whether `codec` can decode on `kind` devices given just a device context.
fn supports(codec: &Codec, kind: Kind) -> bool {
    let method = ffi::AV_CODEC_HW_CONFIG_METHOD_HW_DEVICE_CTX as i32;
    (0..)
        // SAFETY: the codec pointer is static; FFmpeg returns null past the last config.
        .map_while(|i| unsafe { ffi::avcodec_get_hw_config(codec.as_ptr(), i).as_ref() })
        .any(|config| config.device_type == kind && config.methods & method != 0)
}

/// Another decoder of `default`'s codec, to decode on the GPU with where
/// `default` (FFmpeg's pick) can't and it can on this machine: FFmpeg's own
/// AV1 decoder over libdav1d. It decodes nothing without the GPU.
pub fn instead_of(default: &Codec) -> Option<Codec> {
    if KINDS.iter().any(|&kind| supports(default, kind)) {
        return None;
    }
    decoders(default.id()).find(|codec| {
        KINDS.iter().any(|&kind| {
            supports(codec, kind) && decodes(codec.id(), kind) && device(kind).is_some()
        })
    })
}

/// Every decoder FFmpeg has for `id`, in its order.
fn decoders(id: codec::Id) -> impl Iterator<Item = Codec> {
    let id = ffi::AVCodecID::from(id);
    let mut opaque = ptr::null_mut();
    // SAFETY: FFmpeg's codecs are static, and it returns null past the
    // last one.
    std::iter::from_fn(move || unsafe { ffi::av_codec_iterate(&mut opaque).as_ref() })
        .filter(move |codec| {
            // SAFETY: the codec is one of FFmpeg's.
            codec.id == id && unsafe { ffi::av_codec_is_decoder(*codec) } != 0
        })
        // SAFETY: as above.
        .map(|codec| unsafe { Codec::wrap(codec) })
}

/// Whether the hardware of `kind` decodes `id`, where it says: on macOS,
/// VideoToolbox does AV1 only on Apple chips from the M3 on.
#[cfg(target_os = "macos")]
fn decodes(id: codec::Id, kind: Kind) -> bool {
    #[link(name = "VideoToolbox", kind = "framework")]
    unsafe extern "C" {
        fn VTIsHardwareDecodeSupported(codec_type: u32) -> u8;
    }
    let codec_type = match id {
        codec::Id::AV1 => u32::from_be_bytes(*b"av01"),
        _ => return true,
    };
    // SAFETY: a plain query.
    kind != Kind::AV_HWDEVICE_TYPE_VIDEOTOOLBOX
        || unsafe { VTIsHardwareDecodeSupported(codec_type) } != 0
}

/// Elsewhere it isn't known before decoding, which falls back to software
/// at the first error.
#[cfg(not(target_os = "macos"))]
fn decodes(_: codec::Id, _: Kind) -> bool {
    true
}

/// Give an unopened decoder context a hardware device, if one supports the
/// codec on this machine. Returns the name of the API.
///
/// FFmpeg's default format negotiation then picks the hardware format, and
/// falls back to software by itself when the hardware can't take a stream
/// (an unsupported profile or size, for example).
pub fn attach(context: &mut codec::context::Context, codec: &Codec) -> Option<&'static str> {
    for &kind in KINDS {
        if !supports(codec, kind) {
            continue;
        }
        let Some(device) = device(kind) else {
            continue;
        };
        // SAFETY: the context isn't open yet, and it owns (and later frees) the
        // new reference.
        unsafe {
            let reference = ffi::av_buffer_ref(device);
            if reference.is_null() {
                continue;
            }
            (*context.as_mut_ptr()).hw_device_ctx = reference;
        }
        return Some(api_name(kind));
    }
    None
}

/// Whether a decoded frame lives in GPU memory.
pub fn is_hardware_frame(frame: &frame::Video) -> bool {
    // SAFETY: reading a field of a valid frame.
    unsafe { !(*frame.as_ptr()).hw_frames_ctx.is_null() }
}

/// Copy a GPU frame into memory (usually as NV12, or P010 for 10-bit video).
pub fn download(source: &frame::Video, target: &mut frame::Video) -> Result<()> {
    // SAFETY: both frames are valid; the target is reset first so FFmpeg
    // allocates fresh buffers of the right format for it.
    unsafe {
        ffi::av_frame_unref(target.as_mut_ptr());
        let result = ffi::av_hwframe_transfer_data(target.as_mut_ptr(), source.as_ptr(), 0);
        if result < 0 {
            bail!(
                "copying a frame from the GPU failed: {}",
                ffmpeg::Error::from(result)
            );
        }
        // Timestamps, aspect ratio and color information.
        ffi::av_frame_copy_props(target.as_mut_ptr(), source.as_ptr());
    }
    Ok(())
}

#[cfg(test)]
#[path = "tests/hwaccel.rs"]
mod tests;
