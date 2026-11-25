//! Bluetooth headphones and speakers paired with this computer, as outputs
//! while they aren't connected (macOS): listed under the id macOS gives
//! their output once they are (`coreaudio:`, their address, `:output`), so
//! a device picked is the same one either way. Picking one connects it, as
//! Control Center does, and its sound goes there once its output is up.
//! Nothing else connects one: not starting the app, nor it going away.
//!
//! Only in Plays.app, whose Info.plist says what it uses Bluetooth for:
//! macOS stops a process that uses it without saying, as tests and smoke
//! runs would be. Elsewhere none are listed; on Linux they're connected
//! from the desktop.

use crate::media::output::DeviceInfo;

/// What a paired device's output id starts and ends with, around its
/// address.
const ID_START: &str = "coreaudio:";
const ID_END: &str = ":output";

/// A device paired with this computer that plays sound.
#[derive(Clone, PartialEq, Eq, Debug)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub struct Paired {
    /// As macOS writes it: `00-00-5e-00-53-01`.
    pub address: String,
    pub name: String,
}

/// The id of the output macOS makes for the device at `address` once it's
/// connected.
pub fn output_id(address: &str) -> String {
    format!("{ID_START}{}{ID_END}", address.to_ascii_uppercase())
}

/// The address of the Bluetooth device output `id` is, if it's one's.
pub fn address(id: &str) -> Option<&str> {
    let address = id.strip_prefix(ID_START)?.strip_suffix(ID_END)?;
    let bytes: Vec<&str> = address.split('-').collect();
    let is_address = bytes.len() == 6
        && bytes
            .iter()
            .all(|byte| byte.len() == 2 && byte.chars().all(|c| c.is_ascii_hexdigit()));
    is_address.then_some(address)
}

/// The `paired` devices whose outputs aren't among `outputs`, as outputs to
/// pick.
pub fn not_connected(paired: &[Paired], outputs: &[DeviceInfo]) -> Vec<DeviceInfo> {
    paired
        .iter()
        .map(|device| DeviceInfo {
            id: output_id(&device.address),
            name: device.name.clone(),
            bluetooth: true,
        })
        .filter(|device| !outputs.iter().any(|output| output.id == device.id))
        .collect()
}

/// Whether a device of this class (its major and minor parts) plays sound:
/// headsets, headphones, speakers, car and hi-fi audio, a TV's speakers.
/// Not microphones, set-top boxes or cameras.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn plays_sound(major: u32, minor: u32) -> bool {
    /// Audio/Video, and its minor classes that play sound.
    const AUDIO: u32 = 0x04;
    const PLAYS: [u32; 8] = [0x01, 0x02, 0x05, 0x06, 0x07, 0x08, 0x0A, 0x0F];
    major == AUDIO && PLAYS.contains(&minor)
}

/// The Bluetooth devices paired with this computer that play sound. Slow,
/// so not for the UI thread.
pub fn paired() -> Vec<Paired> {
    #[cfg(target_os = "macos")]
    return mac::paired();
    #[cfg(not(target_os = "macos"))]
    Vec::new()
}

/// Connect the paired device at `address`. Waits until it is, or it
/// couldn't be, so not for the UI thread.
pub fn connect(address: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    return mac::connect(address);
    #[cfg(not(target_os = "macos"))]
    Err(format!(
        "{address}: Plays connects Bluetooth devices only on macOS"
    ))
}

// objc's message macros test a `cargo-clippy` feature this crate doesn't have.
#[cfg(target_os = "macos")]
#[allow(unexpected_cfgs)]
mod mac {
    use std::ffi::{CStr, CString, c_char};
    use std::ptr;

    use objc::runtime::{Class, Object};
    use objc::{class, msg_send, sel, sel_impl};

    use super::{Paired, plays_sound};

    type Id = *mut Object;

    // Referenced so the framework is linked, and its class there to find.
    #[link(name = "IOBluetooth", kind = "framework")]
    unsafe extern "C" {
        #[link_name = "OBJC_CLASS_$_IOBluetoothDevice"]
        static DEVICE: u8;
    }

    /// What macOS wants an app that uses Bluetooth to say it's for.
    const USAGE_KEY: &str = "NSBluetoothAlwaysUsageDescription";

    /// Whether this process may use Bluetooth: its Info.plist says what
    /// for, as Plays.app's does. One that doesn't is stopped.
    pub fn may_use() -> bool {
        let Ok(key) = CString::new(USAGE_KEY) else {
            return false;
        };
        // SAFETY: class messages Foundation always answers; `key` outlives
        // the string made from it, which the autorelease pool keeps.
        unsafe {
            let pool: Id = msg_send![class!(NSAutoreleasePool), new];
            let bundle: Id = msg_send![class!(NSBundle), mainBundle];
            let key: Id = msg_send![class!(NSString), stringWithUTF8String: key.as_ptr()];
            let said = !bundle.is_null() && {
                let value: Id = msg_send![bundle, objectForInfoDictionaryKey: key];
                !value.is_null()
            };
            let _: () = msg_send![pool, drain];
            said
        }
    }

    fn device_class() -> &'static Class {
        // SAFETY: the class the framework exports.
        unsafe { &*ptr::addr_of!(DEVICE).cast::<Class>() }
    }

    /// `string`'s text, if there's a string.
    ///
    /// # Safety
    ///
    /// `string` is null or an `NSString`.
    unsafe fn text(string: Id) -> Option<String> {
        if string.is_null() {
            return None;
        }
        // SAFETY: an NSString answers `UTF8String`, with text that lives
        // as long as it does, here the autorelease pool's.
        let bytes: *const c_char = unsafe { msg_send![string, UTF8String] };
        (!bytes.is_null()).then(|| {
            // SAFETY: as above, and not null.
            unsafe { CStr::from_ptr(bytes) }
                .to_string_lossy()
                .into_owned()
        })
    }

    pub fn paired() -> Vec<Paired> {
        if !may_use() {
            return Vec::new();
        }
        let class = device_class();
        let mut found = Vec::new();
        // SAFETY: `pairedDevices` is an NSArray of IOBluetoothDevice, each
        // answering these; all are kept by the pool until it drains.
        unsafe {
            let pool: Id = msg_send![class!(NSAutoreleasePool), new];
            let devices: Id = msg_send![class, pairedDevices];
            let count: usize = if devices.is_null() {
                0
            } else {
                msg_send![devices, count]
            };
            for i in 0..count {
                let device: Id = msg_send![devices, objectAtIndex: i];
                let major: u32 = msg_send![device, deviceClassMajor];
                let minor: u32 = msg_send![device, deviceClassMinor];
                if !plays_sound(major, minor) {
                    continue;
                }
                let name = text(msg_send![device, name]);
                let address = text(msg_send![device, addressString]);
                if let (Some(name), Some(address)) = (name, address) {
                    found.push(Paired { address, name });
                }
            }
            let _: () = msg_send![pool, drain];
        }
        found
    }

    pub fn connect(address: &str) -> Result<(), String> {
        if !may_use() {
            return Err("only Plays.app may use Bluetooth".into());
        }
        let class = device_class();
        let address_text = CString::new(address).map_err(|err| err.to_string())?;
        // SAFETY: as in `paired`; `openConnection` waits for the device and
        // returns an IOReturn.
        let status: i32 = unsafe {
            let pool: Id = msg_send![class!(NSAutoreleasePool), new];
            let string: Id =
                msg_send![class!(NSString), stringWithUTF8String: address_text.as_ptr()];
            let device: Id = msg_send![class, deviceWithAddressString: string];
            let status = if device.is_null() {
                -1
            } else {
                msg_send![device, openConnection]
            };
            let _: () = msg_send![pool, drain];
            status
        };
        match status {
            0 => Ok(()),
            -1 => Err(format!("no device at {address}")),
            status => Err(format!("{address} didn't connect (IOReturn {status:#x})")),
        }
    }
}

#[cfg(test)]
#[path = "tests/bluetooth.rs"]
mod tests;
