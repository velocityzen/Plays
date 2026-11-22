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

/// FFmpeg's own matrices, as its demuxers make them, come back as the
/// turns and flips they describe.
#[test]
fn reads_ffmpeg_display_matrices() {
    let matrix = |rotation: f64, hflip: bool| {
        let mut m = [0i32; 9];
        // SAFETY: both write nine i32s.
        unsafe {
            ffi::av_display_rotation_set(m.as_mut_ptr(), rotation);
            ffi::av_display_matrix_flip(m.as_mut_ptr(), hflip.into(), 0);
        }
        Orientation::from_display_matrix(&m).unwrap()
    };
    // `av_display_rotation_set` takes degrees clockwise, while
    // `av_display_rotation_get` (and ffprobe) give them counterclockwise:
    // a phone held upright records -90, and players turn it a quarter
    // clockwise.
    assert_eq!(matrix(0.0, false), Orientation::UPRIGHT);
    assert_eq!(matrix(90.0, false), Orientation::new(1, false));
    assert_eq!(matrix(-90.0, false), Orientation::new(3, false));
    assert_eq!(matrix(180.0, false), Orientation::new(2, false));
    assert_eq!(matrix(88.0, false), Orientation::new(1, false));
    let mut phone = [0i32; 9];
    // SAFETY: both take nine i32s.
    let rotation = unsafe {
        ffi::av_display_rotation_set(phone.as_mut_ptr(), 90.0);
        ffi::av_display_rotation_get(phone.as_ptr())
    };
    assert_eq!(rotation.round(), -90.0);
    // The flip comes after the turn.
    assert_eq!(matrix(0.0, true), Orientation::new(0, true));
    assert_eq!(
        matrix(90.0, true),
        Orientation::new(1, false).flipped_horizontally()
    );
    assert_eq!(Orientation::from_display_matrix(&[0; 9]), None);
}
