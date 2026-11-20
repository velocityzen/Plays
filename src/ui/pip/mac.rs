//! The AppKit side. The layer sits over the tile in the window's layer
//! tree, where AVKit wants it; `AVPictureInPictureController` shows what it
//! is given in the floating window, and calls back a delegate of Plays' own
//! (`PlaysPictureInPictureDelegate`) for its buttons and what happens to it.
//!
//! AVKit moves the layer's video into that window (one of this app's own),
//! but at a size of its own rather than fitted to it, so while it is open
//! the video is kept filling the window; the layer stays, hidden, over the
//! tile.
//!
//! Frames go over as sample buffers marked to show at once, as the tile's
//! player says they are due. The layer's timebase only follows playback for
//! the window's progress bar. VideoToolbox surfaces go as they are, tagged
//! with their colors (HDR included); converted frames are copied into BGRA
//! buffers. Turned or mirrored videos are turned on the GPU first, from
//! macOS 13. While subtitles show, frames are scaled to the window's pixels
//! on the GPU and the subtitles drawn into them (`burn_in`). Dolby Vision
//! surfaces need Plays' own shader, so they aren't shown.

// objc's message macros test a `cargo-clippy` feature this crate doesn't have.
#![allow(unexpected_cfgs)]

use std::ffi::{CStr, c_char, c_int, c_void};
use std::sync::{Arc, OnceLock};
use std::{ptr, slice};

use anyhow::{Context as _, Result, anyhow, bail, ensure};
use core_foundation::array::{CFArrayGetCount, CFArrayGetValueAtIndex, CFArrayRef};
use core_foundation::base::{
    CFAllocatorRef, CFRelease, CFType, CFTypeRef, OSStatus, TCFType, kCFAllocatorDefault,
};
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::{CFDictionary, CFDictionarySetValue, CFMutableDictionaryRef};
use core_foundation::number::CFNumber;
use core_foundation::string::{CFString, CFStringRef};
use core_video::buffer::{
    CVBufferGetAttachment, CVBufferPropagateAttachments, CVBufferSetAttachment,
    kCVAttachmentMode_ShouldPropagate,
};
use core_video::image_buffer::{
    CVImageBufferColorPrimaries, CVImageBufferKeys, kCVImageBufferTransferFunction_ITU_R_2100_HLG,
    kCVImageBufferTransferFunction_SMPTE_ST_2084_PQ, kCVImageBufferTransferFunction_sRGB,
    kCVImageBufferYCbCrMatrix_ITU_R_709_2, kCVImageBufferYCbCrMatrix_ITU_R_2020,
};
use core_video::pixel_buffer::{
    CVPixelBuffer, CVPixelBufferKeys, CVPixelBufferRef, kCVPixelFormatType_32BGRA,
    kCVPixelFormatType_420YpCbCr8BiPlanarFullRange,
    kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
    kCVPixelFormatType_420YpCbCr10BiPlanarFullRange,
    kCVPixelFormatType_420YpCbCr10BiPlanarVideoRange,
};
use core_video::pixel_buffer_pool::CVPixelBufferPool;
use futures::channel::mpsc::UnboundedSender;
use gpui::{Bounds, Pixels, RenderImage, Window};
use objc::declare::ClassDecl;
use objc::runtime::{BOOL, Class, NO, Object, Protocol, Sel, YES};
use objc::{Encode, Encoding, class, msg_send, sel, sel_impl};
use parking_lot::Mutex;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

use super::burn_in::{Overlay, Sample, Ycc};
use super::{PipEvent, Playback, Subtitles};
use crate::media::hdr::Transfer;
use crate::media::picture::PictureId;
use crate::media::{Orientation, Picture};
use crate::ui::overlay;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct CMTime {
    value: i64,
    timescale: i32,
    flags: u32,
    epoch: i64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct CMTimeRange {
    start: CMTime,
    duration: CMTime,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct CMVideoDimensions {
    width: i32,
    height: i32,
}

#[repr(C)]
struct CMSampleTimingInfo {
    duration: CMTime,
    presentation: CMTime,
    decode: CMTime,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct CGRect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
struct CATransform3D([f64; 16]);

impl CATransform3D {
    const IDENTITY: Self = Self([
        1.0, 0.0, 0.0, 0.0, //
        0.0, 1.0, 0.0, 0.0, //
        0.0, 0.0, 1.0, 0.0, //
        0.0, 0.0, 0.0, 1.0,
    ]);
}

impl CGRect {
    /// The same, give or take rounding.
    fn close_to(&self, other: &Self) -> bool {
        [
            (self.x, other.x),
            (self.y, other.y),
            (self.width, other.width),
            (self.height, other.height),
        ]
        .iter()
        .all(|(a, b)| (a - b).abs() < 0.5)
    }
}

// SAFETY: these are the Objective-C type encodings of the C structs above,
// which only describe the methods to the runtime.
unsafe impl Encode for CMTime {
    fn encode() -> Encoding {
        unsafe { Encoding::from_str("{?=qiIq}") }
    }
}
unsafe impl Encode for CMTimeRange {
    fn encode() -> Encoding {
        unsafe { Encoding::from_str("{?={?=qiIq}{?=qiIq}}") }
    }
}
unsafe impl Encode for CMVideoDimensions {
    fn encode() -> Encoding {
        unsafe { Encoding::from_str("{?=ii}") }
    }
}

type CMFormatDescriptionRef = *const c_void;
type CMSampleBufferRef = *const c_void;
type CMTimebaseRef = *const c_void;
type CMClockRef = *const c_void;

#[link(name = "CoreMedia", kind = "framework")]
unsafe extern "C" {
    fn CMVideoFormatDescriptionCreateForImageBuffer(
        allocator: CFAllocatorRef,
        image: CVPixelBufferRef,
        description: *mut CMFormatDescriptionRef,
    ) -> OSStatus;
    fn CMVideoFormatDescriptionMatchesImageBuffer(
        description: CMFormatDescriptionRef,
        image: CVPixelBufferRef,
    ) -> u8;
    fn CMSampleBufferCreateReadyWithImageBuffer(
        allocator: CFAllocatorRef,
        image: CVPixelBufferRef,
        description: CMFormatDescriptionRef,
        timing: *const CMSampleTimingInfo,
        sample: *mut CMSampleBufferRef,
    ) -> OSStatus;
    fn CMSampleBufferGetSampleAttachmentsArray(
        sample: CMSampleBufferRef,
        create_if_necessary: u8,
    ) -> CFArrayRef;
    fn CMClockGetHostTimeClock() -> CMClockRef;
    fn CMTimebaseGetTime(timebase: CMTimebaseRef) -> CMTime;
    fn CMTimebaseSetTime(timebase: CMTimebaseRef, time: CMTime) -> OSStatus;
    fn CMTimebaseSetRate(timebase: CMTimebaseRef, rate: f64) -> OSStatus;
    fn CMTimeMakeWithSeconds(seconds: f64, timescale: i32) -> CMTime;
    fn CMTimeGetSeconds(time: CMTime) -> f64;

    static kCMSampleAttachmentKey_DisplayImmediately: CFStringRef;
    static kCMTimeInvalid: CMTime;
    static kCMTimeZero: CMTime;
    static kCMTimePositiveInfinity: CMTime;
    static kCMTimeNegativeInfinity: CMTime;
}

// Referenced so the frameworks are linked, and their classes there to find.
#[link(name = "AVFoundation", kind = "framework")]
unsafe extern "C" {
    #[link_name = "OBJC_CLASS_$_AVSampleBufferDisplayLayer"]
    static SAMPLE_BUFFER_DISPLAY_LAYER: u8;
    static AVLayerVideoGravityResizeAspect: *mut Object;
}

#[link(name = "AVKit", kind = "framework")]
unsafe extern "C" {
    #[link_name = "OBJC_CLASS_$_AVPictureInPictureController"]
    static PICTURE_IN_PICTURE_CONTROLLER: u8;
}

#[link(name = "VideoToolbox", kind = "framework")]
unsafe extern "C" {
    fn VTSessionSetProperty(session: CFTypeRef, key: CFStringRef, value: CFTypeRef) -> OSStatus;
    fn VTPixelTransferSessionCreate(
        allocator: CFAllocatorRef,
        session: *mut *mut c_void,
    ) -> OSStatus;
    fn VTPixelTransferSessionTransferImage(
        session: *mut c_void,
        source: CVPixelBufferRef,
        destination: CVPixelBufferRef,
    ) -> OSStatus;
    fn VTPixelTransferSessionInvalidate(session: *mut c_void);

    static kVTPixelTransferPropertyKey_RealTime: CFStringRef;
}

unsafe extern "C" {
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}

/// `RTLD_DEFAULT`: every image loaded.
const RTLD_DEFAULT: *mut c_void = -2isize as *mut c_void;

/// A function or variable newer systems have, found as the app runs, so the
/// app still starts on systems without it.
fn symbol(name: &CStr) -> Option<*mut c_void> {
    // SAFETY: looking a name up has no other effect.
    let found = unsafe { dlsym(RTLD_DEFAULT, name.as_ptr()) };
    (!found.is_null()).then_some(found)
}

/// `AVQueuedSampleBufferRenderingStatusFailed`.
const RENDERING_FAILED: isize = 2;

fn layer_class() -> &'static Class {
    // SAFETY: the symbol is the class object.
    unsafe { &*ptr::addr_of!(SAMPLE_BUFFER_DISPLAY_LAYER).cast::<Class>() }
}

fn controller_class() -> &'static Class {
    // SAFETY: the symbol is the class object.
    unsafe { &*ptr::addr_of!(PICTURE_IN_PICTURE_CONTROLLER).cast::<Class>() }
}

/// Whether this Mac can show Picture in Picture of sample buffers: macOS 12
/// and later.
pub fn supported() -> bool {
    static SUPPORTED: OnceLock<bool> = OnceLock::new();
    *SUPPORTED.get_or_init(|| {
        if Class::get("AVPictureInPictureControllerContentSource").is_none() {
            return false;
        }
        // SAFETY: a class method without arguments.
        let supported: BOOL = unsafe { msg_send![controller_class(), isPictureInPictureSupported] };
        supported == YES
    })
}

/// What the delegate shares with the tile.
struct Shared {
    playback: Mutex<Playback>,
    events: UnboundedSender<PipEvent>,
}

impl Shared {
    fn send(&self, event: PipEvent) {
        let _ = self.events.unbounded_send(event);
    }
}

/// The delegate's instance variable pointing at its `Shared`, null once the
/// tile is done with it.
const SHARED: &str = "playsShared";

/// Its `Shared`, while the tile has it.
fn shared(this: &Object) -> Option<&Shared> {
    // SAFETY: the variable is declared on the class, and points at a live
    // `Shared` until `Pip::drop` sets it to null.
    unsafe {
        let shared: *const c_void = *this.get_ivar(SHARED);
        shared.cast::<Shared>().as_ref()
    }
}

/// The start of an Objective-C block, enough to call it.
#[repr(C)]
struct Block {
    isa: *const c_void,
    flags: c_int,
    reserved: c_int,
    invoke: *const c_void,
}

/// Call a `void (^)(void)` block.
///
/// # Safety
/// `block` must be such a block, or null.
unsafe fn call_block(block: *mut Block) {
    if let Some(literal) = unsafe { block.as_ref() } {
        // SAFETY: a block's function takes the block, then its arguments.
        let invoke: unsafe extern "C" fn(*mut Block) =
            unsafe { std::mem::transmute(literal.invoke) };
        unsafe { invoke(block) };
    }
}

/// Call a `void (^)(BOOL)` block.
///
/// # Safety
/// `block` must be such a block, or null.
unsafe fn call_block_with(block: *mut Block, value: BOOL) {
    if let Some(literal) = unsafe { block.as_ref() } {
        // SAFETY: as in `call_block`.
        let invoke: unsafe extern "C" fn(*mut Block, BOOL) =
            unsafe { std::mem::transmute(literal.invoke) };
        unsafe { invoke(block, value) };
    }
}

fn seconds(time: CMTime) -> f64 {
    // SAFETY: a pure function of its argument.
    unsafe { CMTimeGetSeconds(time) }
}

fn time(seconds: f64) -> CMTime {
    // SAFETY: a pure function of its arguments.
    unsafe { CMTimeMakeWithSeconds(seconds, 600) }
}

// The delegate's methods, which AVKit calls on the main thread.

extern "C" fn set_playing(this: &Object, _: Sel, _: *mut Object, playing: BOOL) {
    if let Some(shared) = shared(this) {
        let playing = playing == YES;
        shared.playback.lock().playing = playing;
        shared.send(PipEvent::SetPlaying(playing));
    }
}

extern "C" fn time_range(this: &Object, _: Sel, _: *mut Object) -> CMTimeRange {
    match shared(this).and_then(|shared| shared.playback.lock().duration) {
        // SAFETY: reading CoreMedia's constants.
        Some(duration) => CMTimeRange {
            start: unsafe { kCMTimeZero },
            duration: time(duration),
        },
        // Live: no end, and nothing to skip through.
        None => unsafe {
            CMTimeRange {
                start: kCMTimeNegativeInfinity,
                duration: kCMTimePositiveInfinity,
            }
        },
    }
}

extern "C" fn is_paused(this: &Object, _: Sel, _: *mut Object) -> BOOL {
    if shared(this).is_some_and(|shared| shared.playback.lock().playing) {
        NO
    } else {
        YES
    }
}

/// The window's size in pixels, which `Pip::follow_window` reads itself.
extern "C" fn render_size(_: &Object, _: Sel, _: *mut Object, _: CMVideoDimensions) {}

extern "C" fn skip(this: &Object, _: Sel, _: *mut Object, interval: CMTime, done: *mut c_void) {
    if let Some(shared) = shared(this) {
        shared.send(PipEvent::Skip(seconds(interval)));
    }
    // SAFETY: AVKit passes a `void (^)(void)`.
    unsafe { call_block(done.cast()) };
}

extern "C" fn did_start(this: &Object, _: Sel, _: *mut Object) {
    if let Some(shared) = shared(this) {
        shared.send(PipEvent::Started);
    }
}

extern "C" fn did_stop(this: &Object, _: Sel, _: *mut Object) {
    if let Some(shared) = shared(this) {
        shared.send(PipEvent::Stopped);
    }
}

extern "C" fn failed(this: &Object, _: Sel, _: *mut Object, error: *mut Object) {
    if let Some(shared) = shared(this) {
        shared.send(PipEvent::Failed(describe(error)));
    }
}

extern "C" fn restore(this: &Object, _: Sel, _: *mut Object, done: *mut c_void) {
    if let Some(shared) = shared(this) {
        shared.send(PipEvent::Restore);
    }
    // SAFETY: AVKit passes a `void (^)(BOOL)`.
    unsafe { call_block_with(done.cast(), YES) };
}

/// An `NSError`'s description.
fn describe(error: *mut Object) -> String {
    if error.is_null() {
        return "no reason given".into();
    }
    // SAFETY: `error` is an NSError; its description is an NSString, whose
    // UTF-8 lives as long as it.
    unsafe {
        let description: *mut Object = msg_send![error, localizedDescription];
        if description.is_null() {
            return "no reason given".into();
        }
        let utf8: *const c_char = msg_send![description, UTF8String];
        if utf8.is_null() {
            return "no reason given".into();
        }
        CStr::from_ptr(utf8).to_string_lossy().into_owned()
    }
}

/// The delegate class, made the first time.
fn delegate_class() -> &'static Class {
    static CLASS: OnceLock<usize> = OnceLock::new();
    let class = *CLASS.get_or_init(|| {
        let mut decl = ClassDecl::new("PlaysPictureInPictureDelegate", class!(NSObject))
            .expect("the delegate class is only made once");
        decl.add_ivar::<*const c_void>(SHARED);
        // SAFETY: each function takes the receiver, the selector and the
        // arguments the selector says, as AVKit's protocols declare them.
        unsafe {
            decl.add_method(
                sel!(pictureInPictureController:setPlaying:),
                set_playing as extern "C" fn(&Object, Sel, *mut Object, BOOL),
            );
            decl.add_method(
                sel!(pictureInPictureControllerTimeRangeForPlayback:),
                time_range as extern "C" fn(&Object, Sel, *mut Object) -> CMTimeRange,
            );
            decl.add_method(
                sel!(pictureInPictureControllerIsPlaybackPaused:),
                is_paused as extern "C" fn(&Object, Sel, *mut Object) -> BOOL,
            );
            decl.add_method(
                sel!(pictureInPictureController:didTransitionToRenderSize:),
                render_size as extern "C" fn(&Object, Sel, *mut Object, CMVideoDimensions),
            );
            decl.add_method(
                sel!(pictureInPictureController:skipByInterval:completionHandler:),
                skip as extern "C" fn(&Object, Sel, *mut Object, CMTime, *mut c_void),
            );
            decl.add_method(
                sel!(pictureInPictureControllerDidStartPictureInPicture:),
                did_start as extern "C" fn(&Object, Sel, *mut Object),
            );
            decl.add_method(
                sel!(pictureInPictureControllerDidStopPictureInPicture:),
                did_stop as extern "C" fn(&Object, Sel, *mut Object),
            );
            decl.add_method(
                sel!(pictureInPictureController:failedToStartPictureInPictureWithError:),
                failed as extern "C" fn(&Object, Sel, *mut Object, *mut Object),
            );
            decl.add_method(
                sel!(pictureInPictureController:restoreUserInterfaceForPictureInPictureStopWithCompletionHandler:),
                restore as extern "C" fn(&Object, Sel, *mut Object, *mut c_void),
            );
        }
        for protocol in [
            "AVPictureInPictureSampleBufferPlaybackDelegate",
            "AVPictureInPictureControllerDelegate",
        ] {
            if let Some(protocol) = Protocol::get(protocol) {
                decl.add_protocol(protocol);
            }
        }
        decl.register() as *const Class as usize
    });
    // SAFETY: a registered class lives as long as the process.
    unsafe { &*(class as *const Class) }
}

/// A tile in Picture in Picture: the layer its frames go to, and AVKit's
/// controller showing it. Closes the window when dropped.
pub struct Pip {
    layer: *mut Object,
    controller: *mut Object,
    delegate: *mut Object,
    /// From `Arc::into_raw`, released when dropped.
    shared: *const Shared,
    /// Follows playback, for the window's progress bar; null if it couldn't
    /// be made.
    timebase: CMTimebaseRef,
    /// Whether the timebase was last set to run, if it has been.
    running: Option<bool>,
    /// The last frame's format, for the next of the same.
    format: CMFormatDescriptionRef,
    /// The last frame handed over, how it was turned, and with which
    /// subtitles (`revision`).
    shown: Option<(PictureId, Orientation, u64)>,
    /// BGRA buffers for converted frames.
    images: Buffers,
    turner: Option<Turner>,
    /// `startPictureInPicture` was called.
    asked: bool,
    /// The layer's own layer showing the video, retained once AVKit is
    /// asked to start: it moves to the window, and back.
    video: *mut Object,
    /// The window's content width and height in points, and its scale.
    window: Option<(f64, f64, f64)>,
    /// The subtitles showing, counted each time they change.
    subtitles: Option<Subtitles>,
    revision: u64,
    /// The subtitles drawn for frames of this HDR signal (or SDR).
    burned: Option<(Option<Transfer>, Overlay)>,
    /// Frames scaled to the subtitles' size, to draw them into.
    scaled: Buffers,
    scaler: Option<Scaler>,
}

impl Pip {
    /// Picture in Picture for a tile whose picture shows at `frame` in
    /// `window`, which plays a live stream if `live`. It shows once it has
    /// frames (`show`) and AVKit is ready (`start_when_possible`); what
    /// happens then comes through `events`.
    pub fn new(
        window: &Window,
        frame: Bounds<Pixels>,
        live: bool,
        events: UnboundedSender<PipEvent>,
    ) -> Result<Self> {
        ensure!(supported(), "Picture in Picture needs macOS 12 or later");
        let view = ns_view(window).context("the window has no view")?;
        let source_class = Class::get("AVPictureInPictureControllerContentSource")
            .context("AVKit has no content source for sample buffers")?;
        // SAFETY: Objective-C objects made and configured on the main
        // thread; each `new` and `alloc` here is owned by `Pip` and released
        // when it is dropped.
        unsafe {
            let superlayer: *mut Object = msg_send![view, layer];
            ensure!(!superlayer.is_null(), "the window's view has no layer");
            let layer: *mut Object = msg_send![layer_class(), new];
            ensure!(!layer.is_null(), "couldn't make a sample buffer layer");
            let _: () = msg_send![layer, setVideoGravity: AVLayerVideoGravityResizeAspect];
            let timebase = make_timebase();
            if !timebase.is_null() {
                let _: () = msg_send![layer, setControlTimebase: timebase];
            }

            let shared = Arc::into_raw(Arc::new(Shared {
                playback: Mutex::new(Playback::default()),
                events,
            }));
            let delegate: *mut Object = msg_send![delegate_class(), new];
            (*delegate).set_ivar(SHARED, shared.cast::<c_void>());

            let source: *mut Object = msg_send![source_class, alloc];
            let source: *mut Object = msg_send![source,
                initWithSampleBufferDisplayLayer: layer
                playbackDelegate: delegate];
            let controller: *mut Object = msg_send![controller_class(), alloc];
            let controller: *mut Object = msg_send![controller, initWithContentSource: source];
            let _: () = msg_send![source, release];
            let pip = Self {
                layer,
                controller,
                delegate,
                shared,
                timebase,
                running: None,
                format: ptr::null(),
                shown: None,
                images: Buffers::default(),
                turner: None,
                asked: false,
                video: ptr::null_mut(),
                window: None,
                subtitles: None,
                revision: 0,
                burned: None,
                scaled: Buffers::default(),
                scaler: None,
            };
            ensure!(
                !controller.is_null(),
                "couldn't make a Picture in Picture controller"
            );
            let _: () = msg_send![controller, setDelegate: delegate];
            if live {
                let _: () = msg_send![controller, setRequiresLinearPlayback: YES];
            }
            place(layer, superlayer, frame);
            let _: () = msg_send![superlayer, addSublayer: layer];
            Ok(pip)
        }
    }

    /// Hand over `picture`, turned to `orientation`, unless it is the one
    /// already showing.
    pub fn show(&mut self, picture: &Picture, orientation: Orientation) {
        let shown = (picture.id(), orientation, self.revision);
        if self.shown == Some(shown) {
            return;
        }
        self.shown = Some(shown);
        let buffer = match picture {
            Picture::Surface(surface) if surface.is_dolby_vision() => return,
            Picture::Surface(surface) => surface.buffer().clone(),
            Picture::Image(image) => match self.copy_image(image) {
                Ok(buffer) => buffer,
                Err(err) => {
                    log::warn!("Picture in Picture: {err:#}");
                    return;
                }
            },
        };
        let buffer = self.turn(buffer, orientation);
        let buffer = match self.burn_in(&buffer) {
            Ok(Some(burned)) => burned,
            Ok(None) => buffer,
            Err(err) => {
                log::warn!("Picture in Picture shows no subtitles: {err:#}");
                buffer
            }
        };
        if let Err(err) = self.enqueue(&buffer) {
            log::warn!("Picture in Picture: {err:#}");
        }
    }

    /// Keep the window's buttons and progress with playback.
    pub fn update(&mut self, playback: Playback) {
        // SAFETY: `shared` lives until `drop`.
        let told = &unsafe { &*self.shared }.playback;
        let changed = keep_up(self.timebase, &mut self.running, told, playback);
        if changed {
            // SAFETY: the controller lives until `drop`.
            unsafe {
                let _: () = msg_send![self.controller, invalidatePlaybackState];
            }
        }
    }

    /// Open the window once AVKit can (it needs a frame in the layer first).
    /// Whether it was asked to.
    pub fn start_when_possible(&mut self) -> bool {
        if !self.asked {
            // SAFETY: the controller lives until `drop`.
            unsafe {
                let possible: BOOL = msg_send![self.controller, isPictureInPicturePossible];
                if possible == YES {
                    let sublayers: *mut Object = msg_send![self.layer, sublayers];
                    let count: usize = if sublayers.is_null() {
                        0
                    } else {
                        msg_send![sublayers, count]
                    };
                    if count > 0 && self.video.is_null() {
                        self.video = msg_send![sublayers, objectAtIndex: 0usize];
                        let _: *mut Object = msg_send![self.video, retain];
                    }
                    let _: () = msg_send![self.controller, startPictureInPicture];
                    self.asked = true;
                }
            }
        }
        self.asked
    }

    /// While the window is open, keep the video filling it. AVKit moves the
    /// layer's video into it at a size and scale of its own, bigger than the
    /// window, which then shows only part of the picture, and puts an opaque
    /// view over part of it.
    pub fn follow_window(&mut self) {
        if self.video.is_null() {
            return;
        }
        // SAFETY: the video layer is retained; the layers and views are
        // AppKit's, read and laid out on the main thread.
        unsafe {
            let holder: *mut Object = msg_send![self.video, superlayer];
            if holder.is_null() || holder == self.layer {
                return;
            }
            let Some((window, content)) = hosting_window(holder) else {
                return;
            };
            let bounds: CGRect = msg_send![content, bounds];
            let scale: f64 = msg_send![window, backingScaleFactor];
            let size = (bounds.width, bounds.height, scale.max(1.0));
            if self.window != Some(size) {
                log::debug!(
                    "Picture in Picture window: {} x {} at {}x",
                    size.0,
                    size.1,
                    size.2
                );
                self.window = Some(size);
            }
            let fill: CGRect = msg_send![holder, convertRect: bounds fromLayer: content];
            let frame: CGRect = msg_send![self.video, frame];
            let transform: CATransform3D = msg_send![self.video, sublayerTransform];
            let covers = covering(holder, self.video);
            if frame.close_to(&fill) && transform == CATransform3D::IDENTITY && covers.is_empty() {
                return;
            }
            let _: () = msg_send![class!(CATransaction), begin];
            let _: () = msg_send![class!(CATransaction), setDisableActions: YES];
            let _: () = msg_send![self.video, setFrame: fill];
            // AVKit scales the video within for its own size.
            let _: () = msg_send![self.video, setSublayerTransform: CATransform3D::IDENTITY];
            let inside = CGRect {
                width: fill.width,
                height: fill.height,
                ..CGRect::default()
            };
            let sublayers: *mut Object = msg_send![self.video, sublayers];
            let count: usize = if sublayers.is_null() {
                0
            } else {
                msg_send![sublayers, count]
            };
            for i in 0..count {
                let sublayer: *mut Object = msg_send![sublayers, objectAtIndex: i];
                let _: () = msg_send![sublayer, setFrame: inside];
            }
            for cover in covers {
                let _: () = msg_send![cover, setHidden: YES];
            }
            let _: () = msg_send![class!(CATransaction), commit];
        }
    }

    /// The size frames of `picture` size (as they show) go at while
    /// subtitles are drawn into them: the window's, in pixels, so they are
    /// as sharp as it shows them.
    pub fn subtitle_size(&self, picture: (u32, u32)) -> (u32, u32) {
        let (Some((width, height, scale)), (w, h)) = (self.window, picture) else {
            return picture;
        };
        if w == 0 || h == 0 {
            return picture;
        }
        let (w, h) = (f64::from(w), f64::from(h));
        let fit = (width * scale / w).min(height * scale / h);
        let side = |length: f64| (length * fit).round().max(1.0) as u32;
        (side(w), side(h))
    }

    /// Draw these subtitles into the frames from now on, or none.
    pub fn set_subtitles(&mut self, subtitles: Option<Subtitles>) {
        self.subtitles = subtitles;
        self.burned = None;
        self.revision += 1;
    }

    /// `buffer` scaled to the subtitles' size with them drawn in, while
    /// there are any.
    fn burn_in(&mut self, buffer: &CVPixelBuffer) -> Result<Option<CVPixelBuffer>> {
        let Some(subtitles) = &self.subtitles else {
            return Ok(None);
        };
        let format = buffer.get_pixel_format();
        let layout = Layout::of(buffer).with_context(|| format!("frames in format {format:#x}"))?;
        let (width, height) = (subtitles.size.0 as usize, subtitles.size.1 as usize);
        let transfer = transfer_of(buffer);
        if self
            .burned
            .as_ref()
            .is_none_or(|(drawn, _)| *drawn != transfer)
        {
            let mut drawn = Overlay::new(width, height, transfer);
            for (image, rect, sampling) in &subtitles.bitmaps {
                overlay::draw_image(&mut drawn, image, *rect, *sampling);
            }
            overlay::draw_text(&mut drawn, &subtitles.text, &subtitles.text_system);
            self.burned = Some((transfer, drawn));
        }
        let scaled = self.scaled.get(format, width, height)?;
        let scaler = match &mut self.scaler {
            Some(scaler) => scaler,
            None => self.scaler.insert(Scaler::new()?),
        };
        scaler.scale(buffer, &scaled)?;
        let (_, drawn) = self.burned.as_ref().expect("drawn above");
        // SAFETY: the buffer was just made, and is this layout.
        unsafe { layout.draw(drawn, &scaled)? };
        Ok(Some(scaled))
    }

    /// Show or hide the layer over the tile; hidden while the video is in
    /// its own window.
    pub fn set_layer_visible(&self, visible: bool) {
        // SAFETY: the layer lives until `drop`.
        unsafe {
            let _: () = msg_send![class!(CATransaction), begin];
            let _: () = msg_send![class!(CATransaction), setDisableActions: YES];
            let _: () = msg_send![self.layer, setHidden: if visible { NO } else { YES }];
            let _: () = msg_send![class!(CATransaction), commit];
        }
    }

    /// A BGRA buffer with `image`'s pixels, tagged as sRGB, as gpui shows it.
    fn copy_image(&mut self, image: &RenderImage) -> Result<CVPixelBuffer> {
        let size = image.size(0);
        let (width, height) = (size.width.0.max(0) as usize, size.height.0.max(0) as usize);
        let bytes = image.as_bytes(0).context("the frame has no pixels")?;
        ensure!(bytes.len() >= width * height * 4, "the frame is short");
        let buffer = self.images.get(kCVPixelFormatType_32BGRA, width, height)?;
        // SAFETY: the buffer is locked while its memory is written, row by
        // row within its stride.
        unsafe {
            let status = buffer.lock_base_address(0);
            ensure!(status == 0, "couldn't lock a pixel buffer (error {status})");
            let base = buffer.get_base_address().cast::<u8>();
            let stride = buffer.get_bytes_per_row();
            let row = width * 4;
            for y in 0..height {
                ptr::copy_nonoverlapping(bytes[y * row..].as_ptr(), base.add(y * stride), row);
            }
            buffer.unlock_base_address(0);
            for (key, value) in [
                (
                    CFString::from(CVImageBufferKeys::ColorPrimaries),
                    CFString::from(CVImageBufferColorPrimaries::ITU_R_709_2),
                ),
                (
                    CFString::from(CVImageBufferKeys::TransferFunction),
                    CFString::wrap_under_get_rule(kCVImageBufferTransferFunction_sRGB),
                ),
            ] {
                CVBufferSetAttachment(
                    buffer.as_concrete_TypeRef(),
                    key.as_concrete_TypeRef(),
                    value.as_CFTypeRef(),
                    kCVAttachmentMode_ShouldPropagate,
                );
            }
        }
        Ok(buffer)
    }

    /// `buffer` turned to `orientation`, or as it is if it can't be.
    fn turn(&mut self, buffer: CVPixelBuffer, orientation: Orientation) -> CVPixelBuffer {
        if orientation == Orientation::UPRIGHT {
            return buffer;
        }
        if self.turner.is_none() {
            match Turner::new() {
                Ok(turner) => self.turner = Some(turner),
                Err(err) => {
                    log::info!("Picture in Picture shows videos as decoded: {err:#}");
                    return buffer;
                }
            }
        }
        let turner = self.turner.as_mut().expect("made above");
        turner
            .turn(&buffer, orientation)
            .inspect_err(|err| log::warn!("Picture in Picture: {err:#}"))
            .unwrap_or(buffer)
    }

    /// Put `buffer` in the layer, to show at once.
    fn enqueue(&mut self, buffer: &CVPixelBuffer) -> Result<()> {
        let image = buffer.as_concrete_TypeRef();
        // SAFETY: CoreMedia objects made here are released here or in
        // `drop`; the layer lives until then.
        unsafe {
            if self.format.is_null()
                || CMVideoFormatDescriptionMatchesImageBuffer(self.format, image) == 0
            {
                if !self.format.is_null() {
                    CFRelease(self.format);
                    self.format = ptr::null();
                }
                let mut format = ptr::null();
                let status = CMVideoFormatDescriptionCreateForImageBuffer(
                    kCFAllocatorDefault,
                    image,
                    &mut format,
                );
                ensure!(status == 0, "couldn't describe a frame (error {status})");
                self.format = format;
            }
            let presentation = if self.timebase.is_null() {
                kCMTimeZero
            } else {
                CMTimebaseGetTime(self.timebase)
            };
            let timing = CMSampleTimingInfo {
                duration: kCMTimeInvalid,
                presentation,
                decode: kCMTimeInvalid,
            };
            let mut sample = ptr::null();
            let status = CMSampleBufferCreateReadyWithImageBuffer(
                kCFAllocatorDefault,
                image,
                self.format,
                &timing,
                &mut sample,
            );
            ensure!(
                status == 0,
                "couldn't make a sample buffer (error {status})"
            );
            let attachments = CMSampleBufferGetSampleAttachmentsArray(sample, 1);
            if !attachments.is_null() && CFArrayGetCount(attachments) > 0 {
                let attachments = CFArrayGetValueAtIndex(attachments, 0) as CFMutableDictionaryRef;
                CFDictionarySetValue(
                    attachments,
                    kCMSampleAttachmentKey_DisplayImmediately.cast(),
                    CFBoolean::true_value().as_CFTypeRef(),
                );
            }
            let status: isize = msg_send![self.layer, status];
            if status == RENDERING_FAILED {
                let error: *mut Object = msg_send![self.layer, error];
                log::warn!("Picture in Picture's layer failed: {}", describe(error));
                let _: () = msg_send![self.layer, flush];
            }
            let _: () = msg_send![self.layer, enqueueSampleBuffer: sample];
            CFRelease(sample);
        }
        Ok(())
    }
}

impl Drop for Pip {
    fn drop(&mut self) {
        // SAFETY: everything here was made in `new` and is released once;
        // the delegate stops reaching `shared` before it goes.
        unsafe {
            let active: BOOL = msg_send![self.controller, isPictureInPictureActive];
            if active == YES {
                let _: () = msg_send![self.controller, stopPictureInPicture];
            }
            let _: () = msg_send![self.controller, setDelegate: ptr::null_mut::<Object>()];
            (*self.delegate).set_ivar(SHARED, ptr::null::<c_void>());
            drop(Arc::from_raw(self.shared));
            let _: () = msg_send![self.layer, flushAndRemoveImage];
            let _: () = msg_send![self.layer, removeFromSuperlayer];
            let _: () = msg_send![self.controller, release];
            let _: () = msg_send![self.delegate, release];
            let _: () = msg_send![self.layer, release];
            if !self.video.is_null() {
                let _: () = msg_send![self.video, release];
            }
            if !self.timebase.is_null() {
                CFRelease(self.timebase);
            }
            if !self.format.is_null() {
                CFRelease(self.format);
            }
        }
    }
}

/// The window's content view.
fn ns_view(window: &Window) -> Option<*mut Object> {
    // gpui's own `Window::window_handle` returns something else.
    let handle = HasWindowHandle::window_handle(window).ok()?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return None;
    };
    Some(handle.ns_view.as_ptr().cast())
}

/// What AVKit puts over `video` in `holder` that hides part of it: views
/// (or layers) drawn after it, filled, and not hidden yet.
///
/// # Safety
/// Both must be layers, `video` in `holder`, on the main thread.
unsafe fn covering(holder: *mut Object, video: *mut Object) -> Vec<*mut Object> {
    let mut covers = Vec::new();
    // SAFETY: as the caller says.
    unsafe {
        let sublayers: *mut Object = msg_send![holder, sublayers];
        let count: usize = if sublayers.is_null() {
            0
        } else {
            msg_send![sublayers, count]
        };
        let mut over = false;
        for i in 0..count {
            let layer: *mut Object = msg_send![sublayers, objectAtIndex: i];
            if layer == video {
                over = true;
                continue;
            }
            let fill: *const c_void = msg_send![layer, backgroundColor];
            if !over || fill.is_null() {
                continue;
            }
            // A view's layer is hidden through the view, or AppKit shows it
            // again.
            let delegate: *mut Object = msg_send![layer, delegate];
            let view: BOOL = if delegate.is_null() {
                NO
            } else {
                msg_send![delegate, isKindOfClass: class!(NSView)]
            };
            let cover = if view == YES { delegate } else { layer };
            let hidden: BOOL = msg_send![cover, isHidden];
            if hidden != YES {
                covers.push(cover);
            }
        }
    }
    covers
}

/// The window `layer` is in, and its content view's layer: from the first
/// layer up that a view draws.
///
/// # Safety
/// `layer` must be a layer, on the main thread.
unsafe fn hosting_window(layer: *mut Object) -> Option<(*mut Object, *mut Object)> {
    let mut layer = layer;
    // SAFETY: as the caller says; views answer `window`.
    unsafe {
        while !layer.is_null() {
            let delegate: *mut Object = msg_send![layer, delegate];
            if !delegate.is_null() {
                let view: BOOL = msg_send![delegate, isKindOfClass: class!(NSView)];
                if view == YES {
                    let window: *mut Object = msg_send![delegate, window];
                    if window.is_null() {
                        return None;
                    }
                    let content: *mut Object = msg_send![window, contentView];
                    let content: *mut Object = if content.is_null() {
                        ptr::null_mut()
                    } else {
                        msg_send![content, layer]
                    };
                    return (!content.is_null()).then_some((window, content));
                }
            }
            layer = msg_send![layer, superlayer];
        }
    }
    None
}

/// A buffer's color tag under `key`, if it has one.
fn tag(buffer: &CVPixelBuffer, key: CVImageBufferKeys) -> Option<CFString> {
    let key = CFString::from(key);
    // SAFETY: the buffer is valid; an attachment is got, not copied, and
    // these tags are strings.
    unsafe {
        let value = CVBufferGetAttachment(
            buffer.as_concrete_TypeRef(),
            key.as_concrete_TypeRef(),
            ptr::null_mut(),
        );
        (!value.is_null()).then(|| CFString::wrap_under_get_rule(value.cast()))
    }
}

/// The HDR signal a buffer is tagged with, if any.
fn transfer_of(buffer: &CVPixelBuffer) -> Option<Transfer> {
    let transfer = tag(buffer, CVImageBufferKeys::TransferFunction)?;
    // SAFETY: reading CoreVideo's constants.
    let (pq, hlg) = unsafe {
        (
            CFString::wrap_under_get_rule(kCVImageBufferTransferFunction_SMPTE_ST_2084_PQ),
            CFString::wrap_under_get_rule(kCVImageBufferTransferFunction_ITU_R_2100_HLG),
        )
    };
    if transfer == pq {
        Some(Transfer::Pq)
    } else if transfer == hlg {
        Some(Transfer::Hlg)
    } else {
        None
    }
}

/// How a frame's samples lie, to draw subtitles into them.
#[derive(Clone, Copy)]
enum Layout {
    Bgra,
    /// 4:2:0 in two planes, a byte a sample.
    Planes8(Ycc),
    /// 4:2:0 in two planes, ten bits high in two bytes a sample.
    Planes16(Ycc),
}

impl Layout {
    fn of(buffer: &CVPixelBuffer) -> Option<Self> {
        let matrix = tag(buffer, CVImageBufferKeys::YCbCrMatrix);
        // SAFETY: reading CoreVideo's constants.
        let (bt709, bt2020) = unsafe {
            (
                CFString::wrap_under_get_rule(kCVImageBufferYCbCrMatrix_ITU_R_709_2),
                CFString::wrap_under_get_rule(kCVImageBufferYCbCrMatrix_ITU_R_2020),
            )
        };
        let weights = |default| match &matrix {
            Some(matrix) if *matrix == bt709 => Ycc::BT709,
            Some(matrix) if *matrix == bt2020 => Ycc::BT2020,
            Some(_) => Ycc::BT601,
            None => default,
        };
        let ycc = |bits, full_range, default| {
            let (kr, kb) = weights(default);
            Ycc {
                kr,
                kb,
                bits,
                full_range,
                shift: if bits > 8 { 16 - bits } else { 0 },
            }
        };
        #[allow(non_upper_case_globals)]
        match buffer.get_pixel_format() {
            kCVPixelFormatType_32BGRA => Some(Self::Bgra),
            kCVPixelFormatType_420YpCbCr8BiPlanarFullRange => {
                Some(Self::Planes8(ycc(8, true, Ycc::BT601)))
            }
            kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange => {
                Some(Self::Planes8(ycc(8, false, Ycc::BT709)))
            }
            kCVPixelFormatType_420YpCbCr10BiPlanarFullRange => {
                Some(Self::Planes16(ycc(10, true, Ycc::BT2020)))
            }
            kCVPixelFormatType_420YpCbCr10BiPlanarVideoRange => {
                Some(Self::Planes16(ycc(10, false, Ycc::BT2020)))
            }
            _ => None,
        }
    }

    /// Lay `overlay` over `buffer`'s samples.
    ///
    /// # Safety
    /// `buffer` must be in this layout, and nothing else using it.
    unsafe fn draw(self, overlay: &Overlay, buffer: &CVPixelBuffer) -> Result<()> {
        let status = buffer.lock_base_address(0);
        ensure!(status == 0, "couldn't lock a pixel buffer (error {status})");
        // SAFETY: locked, the planes are this memory, apart from each other.
        let drawn = unsafe {
            match self {
                Self::Bgra => {
                    let base = buffer.get_base_address().cast::<u8>();
                    let stride = buffer.get_bytes_per_row();
                    (!base.is_null()).then(|| {
                        let pixels = slice::from_raw_parts_mut(base, stride * buffer.get_height());
                        overlay.over_bgra(pixels, stride);
                    })
                }
                Self::Planes8(ycc) => draw_planes::<u8>(overlay, buffer, ycc),
                Self::Planes16(ycc) => draw_planes::<u16>(overlay, buffer, ycc),
            }
        };
        buffer.unlock_base_address(0);
        drawn.context("a pixel buffer has no memory")
    }
}

/// Lay `overlay` over a locked biplanar buffer's samples; `None` if it has
/// no memory.
///
/// # Safety
/// `buffer` must be locked, and have two planes of `S` samples.
unsafe fn draw_planes<S: Sample>(
    overlay: &Overlay,
    buffer: &CVPixelBuffer,
    ycc: Ycc,
) -> Option<()> {
    // SAFETY: as the caller says.
    let plane = |index: usize| unsafe {
        let base = buffer.get_base_address_of_plane(index).cast::<S>();
        let stride = buffer.get_bytes_per_row_of_plane(index) / size_of::<S>();
        let rows = buffer.get_height_of_plane(index);
        (!base.is_null()).then(|| (slice::from_raw_parts_mut(base, stride * rows), stride))
    };
    let (luma, luma_stride) = plane(0)?;
    let (chroma, chroma_stride) = plane(1)?;
    overlay.over_ycbcr(luma, luma_stride, chroma, chroma_stride, ycc);
    Some(())
}

/// Scales pixel buffers on the GPU, into buffers of their own format, with
/// VideoToolbox's `VTPixelTransferSession`.
struct Scaler {
    session: *mut c_void,
}

impl Scaler {
    fn new() -> Result<Self> {
        let mut session = ptr::null_mut();
        // SAFETY: the session is ours; the value is retained by it.
        unsafe {
            let status = VTPixelTransferSessionCreate(kCFAllocatorDefault, &mut session);
            ensure!(
                status == 0 && !session.is_null(),
                "couldn't make a pixel transfer session (error {status})"
            );
            VTSessionSetProperty(
                session,
                kVTPixelTransferPropertyKey_RealTime,
                CFBoolean::true_value().as_CFTypeRef(),
            );
        }
        Ok(Self { session })
    }

    /// Scale `source` into `destination`, of the same format. It gets the
    /// source's color tags first, so the colors are left as they are.
    fn scale(&mut self, source: &CVPixelBuffer, destination: &CVPixelBuffer) -> Result<()> {
        // SAFETY: both buffers are valid; the session lives until dropped.
        unsafe {
            CVBufferPropagateAttachments(
                source.as_concrete_TypeRef(),
                destination.as_concrete_TypeRef(),
            );
            let status = VTPixelTransferSessionTransferImage(
                self.session,
                source.as_concrete_TypeRef(),
                destination.as_concrete_TypeRef(),
            );
            ensure!(status == 0, "scaling a frame failed (error {status})");
        }
        Ok(())
    }
}

impl Drop for Scaler {
    fn drop(&mut self) {
        // SAFETY: the session is ours.
        unsafe {
            VTPixelTransferSessionInvalidate(self.session);
            CFRelease(self.session as CFTypeRef);
        }
    }
}

/// Put `layer` over `frame` (in the window's points, from its top left) in
/// `superlayer`, at once.
///
/// # Safety
/// Both must be layers, on the main thread.
unsafe fn place(layer: *mut Object, superlayer: *mut Object, frame: Bounds<Pixels>) {
    unsafe {
        let bounds: CGRect = msg_send![superlayer, bounds];
        let flipped: BOOL = msg_send![superlayer, isGeometryFlipped];
        let (x, top) = (
            f64::from(f32::from(frame.origin.x)),
            f64::from(f32::from(frame.origin.y)),
        );
        let (width, height) = (
            f64::from(f32::from(frame.size.width)),
            f64::from(f32::from(frame.size.height)),
        );
        let y = if flipped == YES {
            top
        } else {
            bounds.height - top - height
        };
        let _: () = msg_send![class!(CATransaction), begin];
        let _: () = msg_send![class!(CATransaction), setDisableActions: YES];
        let _: () = msg_send![layer, setFrame: CGRect { x, y, width, height }];
        let _: () = msg_send![class!(CATransaction), commit];
    }
}

/// Keep `timebase`, the window's progress, with `playback`: `told` is
/// what the window was last told, which its own buttons change first, and
/// `running` whether the timebase was last set to run. Whether the window
/// must be told playback changed.
fn keep_up(
    timebase: CMTimebaseRef,
    running: &mut Option<bool>,
    told: &Mutex<Playback>,
    playback: Playback,
) -> bool {
    let previous = std::mem::replace(&mut *told.lock(), playback);
    let mut changed = previous.playing != playback.playing
        || previous.duration.is_some() != playback.duration.is_some()
        || previous
            .duration
            .zip(playback.duration)
            .is_some_and(|(a, b)| (a - b).abs() > 0.5);
    if !timebase.is_null() {
        // SAFETY: the caller's timebase, alive while it's used.
        unsafe {
            // By what it was last set to, not what the window was told:
            // its own buttons change that first.
            if *running != Some(playback.playing) {
                CMTimebaseSetRate(timebase, if playback.playing { 1.0 } else { 0.0 });
                *running = Some(playback.playing);
            }
            let now = seconds(CMTimebaseGetTime(timebase));
            // A seek, or drift from the clock: catch up.
            if !now.is_finite() || (now - playback.position).abs() > 0.25 {
                CMTimebaseSetTime(timebase, time(playback.position));
                changed = true;
            }
        }
    }
    changed
}

/// A timebase on the host clock, stopped at zero; null if there is none.
fn make_timebase() -> CMTimebaseRef {
    type Create = unsafe extern "C" fn(CFAllocatorRef, CMClockRef, *mut CMTimebaseRef) -> OSStatus;
    // The second is the first's older name, deprecated since macOS 12.
    let Some(create) = symbol(c"CMTimebaseCreateWithSourceClock")
        .or_else(|| symbol(c"CMTimebaseCreateWithMasterClock"))
    else {
        return ptr::null();
    };
    // SAFETY: both functions take these arguments; the timebase is ours.
    unsafe {
        let create: Create = std::mem::transmute(create);
        let mut timebase = ptr::null();
        if create(
            kCFAllocatorDefault,
            CMClockGetHostTimeClock(),
            &mut timebase,
        ) != 0
        {
            return ptr::null();
        }
        CMTimebaseSetTime(timebase, kCMTimeZero);
        CMTimebaseSetRate(timebase, 0.0);
        timebase
    }
}

/// IOSurface-backed pixel buffers of one format and size, made again when
/// either changes.
#[derive(Default)]
struct Buffers {
    pool: Option<(CVPixelBufferPool, (u32, usize, usize))>,
}

impl Buffers {
    fn get(&mut self, format: u32, width: usize, height: usize) -> Result<CVPixelBuffer> {
        let key = (format, width, height);
        if self.pool.as_ref().is_none_or(|(_, k)| *k != key) {
            let number = |n: i64| CFNumber::from(n).as_CFType();
            let no_properties = CFDictionary::<CFString, CFType>::from_CFType_pairs(&[]);
            let attributes = CFDictionary::from_CFType_pairs(&[
                (
                    CFString::from(CVPixelBufferKeys::PixelFormatType),
                    number(format.into()),
                ),
                (
                    CFString::from(CVPixelBufferKeys::Width),
                    number(width as i64),
                ),
                (
                    CFString::from(CVPixelBufferKeys::Height),
                    number(height as i64),
                ),
                (
                    CFString::from(CVPixelBufferKeys::IOSurfaceProperties),
                    no_properties.as_CFType(),
                ),
            ]);
            let pool = CVPixelBufferPool::new(None, Some(&attributes))
                .map_err(|status| anyhow!("couldn't make a pixel buffer pool (error {status})"))?;
            self.pool = Some((pool, key));
        }
        let (pool, _) = self.pool.as_ref().expect("made above");
        pool.create_pixel_buffer()
            .map_err(|status| anyhow!("couldn't get a pixel buffer (error {status})"))
    }
}

/// `VTPixelRotationSessionRotateImage`.
type Rotate = unsafe extern "C" fn(*mut c_void, CVPixelBufferRef, CVPixelBufferRef) -> OSStatus;

/// Turns and mirrors pixel buffers on the GPU, with VideoToolbox's
/// `VTPixelRotationSession` (macOS 13 and later).
struct Turner {
    session: *mut c_void,
    rotate: Rotate,
    invalidate: unsafe extern "C" fn(*mut c_void),
    /// What the session is set to.
    set_to: Option<Orientation>,
    buffers: Buffers,
}

impl Turner {
    fn new() -> Result<Self> {
        type Create = unsafe extern "C" fn(CFAllocatorRef, *mut *mut c_void) -> OSStatus;
        let (Some(create), Some(rotate), Some(invalidate)) = (
            symbol(c"VTPixelRotationSessionCreate"),
            symbol(c"VTPixelRotationSessionRotateImage"),
            symbol(c"VTPixelRotationSessionInvalidate"),
        ) else {
            bail!("turning frames needs macOS 13 or later");
        };
        // SAFETY: VideoToolbox's functions, with their signatures.
        unsafe {
            let create: Create = std::mem::transmute(create);
            let mut session = ptr::null_mut();
            let status = create(kCFAllocatorDefault, &mut session);
            ensure!(
                status == 0 && !session.is_null(),
                "couldn't make a rotation session (error {status})"
            );
            Ok(Self {
                session,
                rotate: std::mem::transmute::<*mut c_void, Rotate>(rotate),
                invalidate: std::mem::transmute::<*mut c_void, unsafe extern "C" fn(*mut c_void)>(
                    invalidate,
                ),
                set_to: None,
                buffers: Buffers::default(),
            })
        }
    }

    fn turn(&mut self, buffer: &CVPixelBuffer, orientation: Orientation) -> Result<CVPixelBuffer> {
        if self.set_to != Some(orientation) {
            self.set(orientation)?;
            self.set_to = Some(orientation);
        }
        let (width, height) = (buffer.get_width(), buffer.get_height());
        let (width, height) = if orientation.sideways() {
            (height, width)
        } else {
            (width, height)
        };
        let turned = self.buffers.get(buffer.get_pixel_format(), width, height)?;
        // SAFETY: both buffers are valid; the session lives until dropped.
        unsafe {
            let status = (self.rotate)(
                self.session,
                buffer.as_concrete_TypeRef(),
                turned.as_concrete_TypeRef(),
            );
            ensure!(status == 0, "turning a frame failed (error {status})");
            CVBufferPropagateAttachments(
                buffer.as_concrete_TypeRef(),
                turned.as_concrete_TypeRef(),
            );
        }
        Ok(turned)
    }

    /// Set the session to turn pictures to `orientation`: mirrored, then
    /// turned clockwise. VideoToolbox turns first and flips after, and
    /// mirroring then turning one way is turning the other way then
    /// mirroring.
    fn set(&self, orientation: Orientation) -> Result<()> {
        let property = |name: &CStr| -> Result<CFStringRef> {
            let found = symbol(name).with_context(|| format!("no {name:?}"))?;
            // SAFETY: the symbol is a CFStringRef variable.
            Ok(unsafe { *found.cast::<CFStringRef>() })
        };
        let turns = orientation.clockwise_turns();
        let turns = if orientation.is_mirrored() {
            (4 - turns) % 4
        } else {
            turns
        };
        let rotation = property(match turns {
            0 => c"kVTRotation_0",
            1 => c"kVTRotation_CW90",
            2 => c"kVTRotation_180",
            _ => c"kVTRotation_CCW90",
        })?;
        let settings = [
            (
                property(c"kVTPixelRotationPropertyKey_Rotation")?,
                rotation as CFTypeRef,
            ),
            (
                property(c"kVTPixelRotationPropertyKey_FlipHorizontalOrientation")?,
                CFBoolean::from(orientation.is_mirrored()).as_CFTypeRef(),
            ),
        ];
        for (key, value) in settings {
            // SAFETY: the session is valid; it retains the value.
            let status = unsafe { VTSessionSetProperty(self.session, key, value) };
            ensure!(
                status == 0,
                "couldn't set up turning frames (error {status})"
            );
        }
        Ok(())
    }
}

impl Drop for Turner {
    fn drop(&mut self) {
        // SAFETY: the session is ours.
        unsafe {
            (self.invalidate)(self.session);
            CFRelease(self.session as CFTypeRef);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    unsafe extern "C" {
        fn CMTimebaseGetRate(timebase: CMTimebaseRef) -> f64;
    }

    #[test]
    fn the_progress_runs_as_playback_does_whichever_buttons_play_it() {
        let timebase = make_timebase();
        assert!(!timebase.is_null(), "a timebase on the host clock");
        // SAFETY: the timebase lives until the end.
        let rate = || unsafe { CMTimebaseGetRate(timebase) };
        let told = Mutex::new(Playback::default());
        let mut running = None;
        let mut keep = |playing, position| {
            let duration = Some(10.0);
            let playback = Playback {
                playing,
                position,
                duration,
            };
            keep_up(timebase, &mut running, &told, playback);
        };
        keep(true, 0.0);
        assert_eq!(rate(), 1.0, "running with playback");
        // Paused from the window, which says so first, then the tile.
        told.lock().playing = false;
        keep(false, 0.1);
        assert_eq!(rate(), 0.0, "stopped with playback");
        // Played and paused from the tile, then played from the window.
        keep(true, 0.1);
        keep(false, 0.2);
        told.lock().playing = true;
        keep(true, 0.2);
        assert_eq!(rate(), 1.0, "running again");
        // SAFETY: made above, and done with.
        unsafe { CFRelease(timebase) };
    }
}
