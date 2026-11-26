//! HEVC's base layer alone, for a hardware decoder. FFmpeg decodes only
//! the base layer on the GPU, yet hands the hardware every layer's
//! parameter sets. VideoToolbox won't take those of an MV-HEVC file's
//! second view (Dolby Vision profile 20, spatial video), which FFmpeg's MP4
//! demuxer adds to the stream's `hvcC` from its `lhvC`, and fails to
//! start. So a hardware decoder of a stream of more than one layer is given
//! the base layer's NAL units alone (`nuh_layer_id` 0), in its extradata
//! and in its packets. The RPU and profile 7's enhancement layer are the
//! base layer's, and stay.

use std::ptr;

use ffmpeg::packet::{Mut as _, Ref as _};
use ffmpeg::{Packet, codec, ffi};
use ffmpeg_next as ffmpeg;

/// Keeps a stream's packets to its base layer.
#[derive(Debug)]
pub(super) struct BaseLayer {
    framing: Framing,
}

/// How a stream's NAL units are told apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Framing {
    /// Each after its size, in this many bytes, as an `hvcC` says.
    Sized(usize),
    /// Each after a start code (Annex B).
    StartCodes,
}

impl BaseLayer {
    /// For an HEVC stream whose extradata has a layer besides the base one:
    /// what keeps its packets to the base layer, and its parameters with
    /// only the base layer's. `None` for anything else, or extradata that
    /// isn't whole.
    pub(super) fn of(parameters: &codec::Parameters) -> Option<(Self, codec::Parameters)> {
        if parameters.id() != codec::Id::HEVC {
            return None;
        }
        let extradata = extradata(parameters);
        let framing = Framing::of(extradata);
        let base = match framing {
            Framing::Sized(_) => base_record(extradata)?,
            Framing::StartCodes => base_units(extradata, framing)?,
        };
        let mut parameters = parameters.clone();
        set_extradata(&mut parameters, &base)?;
        Some((Self { framing }, parameters))
    }

    /// `packet` with the base layer's NAL units alone; `None` if it has
    /// only those, or can't be read.
    pub(super) fn packet(&self, packet: &Packet) -> Option<Packet> {
        let base = base_units(packet.data()?, self.framing)?;
        let mut kept = Packet::copy(&base);
        // SAFETY: both packets are valid; this copies timestamps, flags,
        // the stream's index and side data, not the data.
        let copied = unsafe { ffi::av_packet_copy_props(kept.as_mut_ptr(), packet.as_ptr()) };
        (copied >= 0).then_some(kept)
    }
}

impl Framing {
    /// As FFmpeg's HEVC decoder tells: extradata that doesn't start as a
    /// start code is an `hvcC`.
    fn of(extradata: &[u8]) -> Self {
        match extradata {
            [a, b, c, ..] if extradata.len() > 3 && (*a != 0 || *b != 0 || *c > 1) => Self::Sized(
                extradata
                    .get(21)
                    .map_or(4, |byte| usize::from(byte & 3) + 1),
            ),
            _ => Self::StartCodes,
        }
    }
}

/// The layer of a NAL unit, from its header: 0, the base layer, for one
/// too short to have one.
fn layer(unit: &[u8]) -> u8 {
    match unit {
        [first, second, ..] => (first & 1) << 5 | second >> 3,
        _ => 0,
    }
}

/// An `hvcC` with only the base layer's NAL units, and without the arrays
/// that leaves empty; `None` if it has no others, or isn't whole.
fn base_record(record: &[u8]) -> Option<Vec<u8>> {
    let (header, mut rest) = record.split_at_checked(23)?;
    let mut arrays = Vec::new();
    let (mut count, mut dropped) = (0u8, false);
    for _ in 0..header[22] {
        let (&[kind, high, low], after) = rest.split_first_chunk::<3>()?;
        rest = after;
        let mut units = Vec::new();
        for _ in 0..u16::from_be_bytes([high, low]) {
            let (&size, after) = rest.split_first_chunk::<2>()?;
            let (unit, after) = after.split_at_checked(usize::from(u16::from_be_bytes(size)))?;
            rest = after;
            if layer(unit) == 0 {
                units.push(unit);
            } else {
                dropped = true;
            }
        }
        if units.is_empty() {
            continue;
        }
        count += 1;
        arrays.push(kind);
        arrays.extend(u16::try_from(units.len()).ok()?.to_be_bytes());
        for unit in units {
            arrays.extend(u16::try_from(unit.len()).ok()?.to_be_bytes());
            arrays.extend(unit);
        }
    }
    if !dropped {
        return None;
    }
    let mut base = header.to_vec();
    base[22] = count;
    base.extend(arrays);
    // Whatever follows the arrays, as FFmpeg keeps it.
    base.extend(rest);
    Some(base)
}

/// `data`'s NAL units of the base layer alone, framed as they were;
/// `None` if it has no others, or can't be read.
fn base_units(data: &[u8], framing: Framing) -> Option<Vec<u8>> {
    let units = units(data, framing)?;
    if units.iter().all(|unit| layer(unit) == 0) {
        return None;
    }
    let mut base = Vec::with_capacity(data.len());
    for unit in units.into_iter().filter(|unit| layer(unit) == 0) {
        match framing {
            Framing::Sized(bytes) => {
                let size = u32::try_from(unit.len()).ok()?.to_be_bytes();
                base.extend(size.get(4 - bytes..)?);
            }
            Framing::StartCodes => base.extend([0, 0, 0, 1]),
        }
        base.extend(unit);
    }
    Some(base)
}

/// The NAL units in `data`; `None` if a size runs past its end.
fn units(data: &[u8], framing: Framing) -> Option<Vec<&[u8]>> {
    match framing {
        Framing::Sized(bytes) => {
            let mut units = Vec::new();
            let mut rest = data;
            while !rest.is_empty() {
                let (size, after) = rest.split_at_checked(bytes)?;
                let size = size
                    .iter()
                    .fold(0usize, |size, &byte| size << 8 | usize::from(byte));
                let (unit, after) = after.split_at_checked(size)?;
                units.push(unit);
                rest = after;
            }
            Some(units)
        }
        Framing::StartCodes => Some(start_code_units(data)),
    }
}

/// The NAL units of an Annex B byte stream, without their start codes, or
/// the zeros before one.
fn start_code_units(data: &[u8]) -> Vec<&[u8]> {
    let mut starts = Vec::new();
    let mut index = 0;
    while index + 3 <= data.len() {
        if data[index..index + 3] == [0, 0, 1] {
            starts.push(index + 3);
            index += 3;
        } else {
            index += 1;
        }
    }
    let ends = starts.iter().skip(1).map(|&next| {
        let mut end = next - 3;
        while end > 0 && data[end - 1] == 0 {
            end -= 1;
        }
        end
    });
    starts
        .iter()
        .zip(ends.chain([data.len()]))
        .map(|(&start, end)| &data[start..end.max(start)])
        .collect()
}

fn extradata(parameters: &codec::Parameters) -> &[u8] {
    // SAFETY: the parameters own `extradata_size` bytes of extradata while
    // they are borrowed.
    unsafe {
        let raw = parameters.as_ptr();
        let (data, size) = ((*raw).extradata, (*raw).extradata_size);
        match usize::try_from(size) {
            Ok(size) if !data.is_null() && size > 0 => std::slice::from_raw_parts(data, size),
            _ => &[],
        }
    }
}

/// Give `parameters` `data` as their extradata; `None` without the memory.
fn set_extradata(parameters: &mut codec::Parameters, data: &[u8]) -> Option<()> {
    let size = i32::try_from(data.len()).ok()?;
    let padding = ffi::AV_INPUT_BUFFER_PADDING_SIZE as usize;
    // SAFETY: the new buffer is FFmpeg's, as big as `data` and the zeroed
    // padding FFmpeg reads past extradata; the parameters own it from here,
    // and free it as they freed the one it replaces.
    unsafe {
        let copy = ffi::av_mallocz(data.len() + padding).cast::<u8>();
        if copy.is_null() {
            return None;
        }
        ptr::copy_nonoverlapping(data.as_ptr(), copy, data.len());
        let raw = parameters.as_mut_ptr();
        ffi::av_freep(ptr::addr_of_mut!((*raw).extradata).cast());
        (*raw).extradata = copy;
        (*raw).extradata_size = size;
    }
    Some(())
}

#[cfg(test)]
#[path = "tests/base_layer.rs"]
pub(super) mod tests;

#[cfg(test)]
#[path = "tests/base_layer_properties.rs"]
mod properties;
