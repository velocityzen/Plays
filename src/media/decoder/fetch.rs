//! A small file read over the network as FFmpeg reads a stream: through
//! its protocols, with the same options, stopping when told to.

use std::ffi::{CStr, CString, c_int, c_void};
use std::ptr;

use anyhow::Result;
use ffmpeg_next::{self as ffmpeg, Dictionary, ffi};

/// A file read whole.
pub struct Fetched {
    pub body: Vec<u8>,
    /// Where it came from, after any redirects.
    pub location: String,
}

/// Read `url` whole, opened with `options`; `None` if it holds more than
/// `limit` bytes. Gives up once `stop` says so.
pub fn fetch(
    url: &str,
    options: Dictionary,
    limit: usize,
    stop: impl Fn() -> bool,
) -> Result<Option<Fetched>> {
    let stop: &dyn Fn() -> bool = &stop;
    let interrupt = ffi::AVIOInterruptCB {
        callback: Some(stopped),
        opaque: (&raw const stop).cast_mut().cast(),
    };
    let address = CString::new(url)?;
    let mut connection = Connection(ptr::null_mut());
    // SAFETY: ffmpeg-next hands over the dictionary it owns.
    let mut options = unsafe { options.disown() };
    // SAFETY: the address and the interrupt callback outlive the call, and
    // `stop`, which the callback reads, outlives the connection: it is
    // closed before this returns.
    let opened = unsafe {
        ffi::avio_open2(
            &raw mut connection.0,
            address.as_ptr(),
            ffi::AVIO_FLAG_READ,
            &raw const interrupt,
            &raw mut options,
        )
    };
    // SAFETY: what FFmpeg left of the options is ours to free.
    drop(unsafe { Dictionary::own(options) });
    if opened < 0 {
        return Err(ffmpeg::Error::from(opened).into());
    }
    let mut body = Vec::new();
    let mut chunk = [0u8; 16 * 1024];
    loop {
        // SAFETY: the connection is open, and `chunk` holds as many bytes as
        // FFmpeg is told it may write.
        let read =
            unsafe { ffi::avio_read(connection.0, chunk.as_mut_ptr(), chunk.len() as c_int) };
        match usize::try_from(read) {
            Ok(0) => break,
            Ok(read) => body.extend_from_slice(&chunk[..read]),
            Err(_) => match ffmpeg::Error::from(read) {
                ffmpeg::Error::Eof => break,
                err => return Err(err.into()),
            },
        }
        if body.len() > limit {
            return Ok(None);
        }
    }
    let location = connection.location().unwrap_or_else(|| url.to_owned());
    Ok(Some(Fetched { body, location }))
}

/// FFmpeg's interrupt callback: whether to stop, as the closure `opaque`
/// points to says.
unsafe extern "C" fn stopped(opaque: *mut c_void) -> c_int {
    // SAFETY: `opaque` is the `&dyn Fn` `fetch` gave FFmpeg with this
    // callback, alive until the connection is closed.
    let stop = unsafe { &*opaque.cast::<&dyn Fn() -> bool>() };
    c_int::from(stop())
}

/// An FFmpeg connection, closed when dropped.
struct Connection(*mut ffi::AVIOContext);

impl Connection {
    /// Where what it read came from, after any redirects, when the
    /// protocol says.
    fn location(&self) -> Option<String> {
        let mut value: *mut u8 = ptr::null_mut();
        // SAFETY: the connection is open; FFmpeg puts a copy of the
        // option's value in `value`, for us to free.
        let found = unsafe {
            ffi::av_opt_get(
                self.0.cast(),
                c"location".as_ptr(),
                ffi::AV_OPT_SEARCH_CHILDREN,
                &raw mut value,
            )
        };
        if found < 0 || value.is_null() {
            return None;
        }
        // SAFETY: FFmpeg's copy is a nul-terminated string.
        let location = unsafe { CStr::from_ptr(value.cast()) }
            .to_string_lossy()
            .into_owned();
        // SAFETY: FFmpeg allocated it, and it's read.
        unsafe { ffi::av_free(value.cast()) };
        (!location.is_empty()).then_some(location)
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        // SAFETY: the connection, if it opened, is closed once, here; FFmpeg
        // takes a null one as already closed.
        unsafe { ffi::avio_closep(&raw mut self.0) };
    }
}
