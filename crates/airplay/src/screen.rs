//! What a TV's screen shows, as `GET /info` says, which a receiver answers
//! before pairing: the largest picture it plays, the HDR it shows, and the
//! Dolby Vision profiles it decodes. TVs with AirPlay built in list it; one
//! that doesn't is taken as showing SDR.

use std::net::SocketAddr;

use plist::{Dictionary, Value};

use crate::connection::{Connection, Request};
use crate::device::Device;

/// High dynamic range, as a screen lists what it shows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Hdr {
    Hdr10,
    Hlg,
    DolbyVision,
}

impl Hdr {
    /// From an `HDRMode`: `HDR10`, `HLG` or `DoVi`. `SDR`, and `HDR`, which
    /// says no more than that, name none.
    fn named(mode: &str) -> Option<Self> {
        match mode {
            "HDR10" => Some(Self::Hdr10),
            "HLG" => Some(Self::Hlg),
            "DoVi" => Some(Self::DolbyVision),
            _ => None,
        }
    }
}

/// A TV's screen.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Screen {
    /// The largest picture it plays, in pixels: its own, which may be more
    /// than it shows now.
    pub max_size: Option<(u32, u32)>,
    /// The HDR it shows, in the order it lists them.
    pub hdr: Vec<Hdr>,
    /// The Dolby Vision profiles it decodes, as the codecs it lists for
    /// Dolby Vision say, `dvhe.05.09` profile 5 and `hev1.08.09` 8: none
    /// where it lists none.
    pub dolby_vision: Vec<u8>,
}

impl Screen {
    /// Where nearly every TV's AirPlay listens: to ask a screen at an
    /// address without looking for its receiver first.
    pub const PORT: u16 = 7000;

    /// Ask `device`, on a connection of its own; `None` if it doesn't say.
    pub fn ask(device: &Device) -> Option<Self> {
        Self::at(&device.addresses)
    }

    /// Ask whatever answers AirPlay at the first of `addresses` that takes
    /// a connection; `None` if none does, or it doesn't say.
    pub fn at(addresses: &[SocketAddr]) -> Option<Self> {
        let info = Connection::open(addresses).and_then(|mut connection| {
            connection
                .exchange(Request::http("GET", "/info"))
                .and_then(|answer| answer.dictionary())
        });
        match info {
            Ok(info) => Self::from_info(&info),
            Err(err) => {
                log::debug!("airplay: nothing at {addresses:?} says what it shows: {err}");
                None
            }
        }
    }

    /// Its first display, as `/info` describes it, if it does.
    pub fn from_info(info: &Dictionary) -> Option<Self> {
        let display = info.get("displays")?.as_array()?.first()?.as_dictionary()?;
        let pixels =
            |width: &str, height: &str| Some((number(display, width)?, number(display, height)?));
        let max_size = pixels("widthPixelsMax", "heightPixelsMax")
            .or_else(|| pixels("widthPixels", "heightPixels"));
        let listed = display
            .get("HDRSupportedModes")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_dictionary);
        let mut hdr = Vec::new();
        let mut dolby_vision = Vec::new();
        for mode in listed.chain(display.get("HDRInfo").and_then(Value::as_dictionary)) {
            let named = mode
                .get("HDRMode")
                .and_then(Value::as_string)
                .and_then(Hdr::named);
            if let Some(named) = named
                && !hdr.contains(&named)
            {
                hdr.push(named);
            }
            if named == Some(Hdr::DolbyVision) {
                let codecs = mode
                    .get("codecStrings")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_string);
                for profile in codecs.filter_map(dolby_vision_profile) {
                    if !dolby_vision.contains(&profile) {
                        dolby_vision.push(profile);
                    }
                }
            }
        }
        Some(Self {
            max_size,
            hdr,
            dolby_vision,
        })
    }
}

/// The Dolby Vision profile a codec string names: the second of its three
/// parts, as `dvhe.05.09` and `hev1.08.09` (profile 8, on HEVC) do.
fn dolby_vision_profile(codec: &str) -> Option<u8> {
    let mut parts = codec.split('.');
    let (Some(_), Some(profile), Some(_), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return None;
    };
    profile.parse().ok()
}

/// A count of pixels, which is more than none.
fn number(dictionary: &Dictionary, key: &str) -> Option<u32> {
    let value = dictionary.get(key)?.as_unsigned_integer()?;
    u32::try_from(value).ok().filter(|value| *value > 0)
}

#[cfg(test)]
#[path = "tests/screen.rs"]
mod tests;
