use super::*;

fn all() -> impl Iterator<Item = Orientation> {
    (0..8).map(|i| Orientation::new(i % 4, i >= 4))
}

fn times(a: [[f32; 2]; 2], b: [[f32; 2]; 2]) -> [[f32; 2]; 2] {
    let mut m = [[0.0; 2]; 2];
    for r in 0..2 {
        for c in 0..2 {
            m[r][c] = a[r][0] * b[0][c] + a[r][1] * b[1][c];
        }
    }
    m
}

const FLIP_H: [[f32; 2]; 2] = [[-1.0, 0.0], [0.0, 1.0]];
const FLIP_V: [[f32; 2]; 2] = [[1.0, 0.0], [0.0, -1.0]];

#[test]
fn composes_like_its_matrices() {
    for a in all() {
        for b in all() {
            assert_eq!(a.after(b).matrix(), times(a.matrix(), b.matrix()));
        }
        // Each has one that undoes it (mirrored ones undo themselves).
        let undo = if a.mirrored {
            a
        } else {
            Orientation::new(-i32::from(a.turns), false)
        };
        assert_eq!(undo.after(a), Orientation::UPRIGHT);
        assert_eq!(a.after(undo), Orientation::UPRIGHT);
        assert_eq!(a.flipped_horizontally().matrix(), times(FLIP_H, a.matrix()));
        assert_eq!(a.flipped_vertically().matrix(), times(FLIP_V, a.matrix()));
        assert_eq!(a.flipped_horizontally().flipped_horizontally(), a);
        assert_eq!(a.rotated(1).rotated(-1), a);
    }
    // Clockwise, with y down: the right edge goes to the bottom.
    assert_eq!(
        Orientation::new(1, false).matrix(),
        [[0.0, -1.0], [1.0, 0.0]]
    );
}

/// A stream's display matrix comes back as the turn and flip it asks for,
/// and none, or one that squashes the picture flat, as upright.
#[test]
fn reads_a_streams_display_matrix() {
    let read = |matrix: Option<[i32; 9]>| {
        let mut parameters = codec::Parameters::new();
        if let Some(matrix) = matrix {
            // SAFETY: FFmpeg allocates the side data as big as asked, owned
            // by the parameters, and nine i32s are copied into it.
            unsafe {
                let raw = parameters.as_mut_ptr();
                let side = ffmpeg::ffi::av_packet_side_data_new(
                    &raw mut (*raw).coded_side_data,
                    &raw mut (*raw).nb_coded_side_data,
                    ffmpeg::ffi::AVPacketSideDataType::AV_PKT_DATA_DISPLAYMATRIX,
                    size_of::<[i32; 9]>(),
                    0,
                );
                assert!(!side.is_null());
                std::ptr::copy_nonoverlapping(matrix.as_ptr(), (*side).data.cast(), 9);
            }
        }
        Orientation::of(&parameters)
    };
    let mut flipped = [0i32; 9];
    // SAFETY: both write nine i32s.
    unsafe {
        ffmpeg::ffi::av_display_rotation_set(flipped.as_mut_ptr(), 90.0);
        ffmpeg::ffi::av_display_matrix_flip(flipped.as_mut_ptr(), 1, 0);
    }
    assert_eq!(
        read(Some(flipped)),
        Orientation::new(1, false).flipped_horizontally()
    );
    assert_eq!(read(None), Orientation::UPRIGHT);
    assert_eq!(read(Some([0; 9])), Orientation::UPRIGHT);
}
