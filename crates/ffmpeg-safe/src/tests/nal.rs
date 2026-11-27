use std::ptr;

use ffmpeg_next::{codec, ffi};

use super::{extradata, nal_units};

#[test]
fn splits_annex_b_at_its_start_codes() {
    let units = |data: &[u8]| nal_units(data).map(<[u8]>::to_vec).collect::<Vec<_>>();
    // A 3-byte start code, a 4-byte one, and one with nothing after it.
    let stream = [0, 0, 1, 0x67, 5, 0, 0, 0, 1, 0x68, 0, 0, 1];
    assert_eq!(units(&stream), [vec![0x67, 5], vec![0x68], vec![]]);
    // What comes before the first start code isn't a unit.
    assert_eq!(units(&[9, 9, 0, 0, 1, 7]), [vec![7]]);
    // Zeros within a unit are kept but for those before a start code.
    assert_eq!(
        units(&[0, 0, 1, 0, 7, 0, 0, 0, 0, 1, 8]),
        [vec![0, 7], vec![8]]
    );
    assert!(units(&[1, 2, 3, 0, 0]).is_empty());
    assert!(units(&[]).is_empty());
}

#[test]
fn reads_the_extradata_its_parameters_have() {
    let mut parameters = codec::Parameters::new();
    assert!(extradata(&parameters).is_empty());
    let bytes = [1, 0x64, 0, 0x1f];
    let padding = ffi::AV_INPUT_BUFFER_PADDING_SIZE as usize;
    // SAFETY: the parameters own the buffer from here, FFmpeg's, as big as
    // the bytes and the padding FFmpeg reads past them.
    unsafe {
        let copy = ffi::av_mallocz(bytes.len() + padding).cast::<u8>();
        assert!(!copy.is_null());
        ptr::copy_nonoverlapping(bytes.as_ptr(), copy, bytes.len());
        let raw = parameters.as_mut_ptr();
        (*raw).extradata = copy;
        (*raw).extradata_size = bytes.len() as i32;
    }
    assert_eq!(extradata(&parameters), bytes);
    // SAFETY: a size the buffer is as big as, or more.
    unsafe { (*parameters.as_mut_ptr()).extradata_size = 1 };
    assert_eq!(extradata(&parameters), [1], "as long as it says");
    // SAFETY: as above.
    unsafe { (*parameters.as_mut_ptr()).extradata_size = -1 };
    assert!(extradata(&parameters).is_empty(), "a size below nothing");
    // SAFETY: as above.
    unsafe { (*parameters.as_mut_ptr()).extradata_size = 0 };
    assert!(extradata(&parameters).is_empty());
}
