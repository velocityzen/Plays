//! H.264's and HEVC's NAL units, and a stream's extradata, which holds them
//! in Annex B or a record of them (avcC, hvcC).

use ffmpeg_next::codec;

/// `parameters`' extradata, empty where they have none.
pub fn extradata(parameters: &codec::Parameters) -> &[u8] {
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

/// The NAL units of an Annex B byte stream, without their start codes, or
/// the zeros before one (a 4-byte start code's).
pub fn nal_units(data: &[u8]) -> impl Iterator<Item = &[u8]> {
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
    let ends: Vec<usize> = starts
        .iter()
        .skip(1)
        .map(|&next| {
            let mut end = next - 3;
            while end > 0 && data[end - 1] == 0 {
                end -= 1;
            }
            end
        })
        .chain([data.len()])
        .collect();
    starts
        .into_iter()
        .zip(ends)
        .map(move |(start, end)| &data[start..end.max(start)])
}

#[cfg(test)]
#[path = "tests/nal.rs"]
mod tests;
