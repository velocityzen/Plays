//! Which way up a picture goes: the quarter turns and mirroring a video
//! asks for in its display matrix (phones record sideways and say so), and
//! the ones the viewer adds.

use ffmpeg::{codec, ffi};
use ffmpeg_next as ffmpeg;

/// One of the eight ways to lay a picture back on its rectangle: mirrored
/// left to right or not, then turned clockwise by some quarter turns.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Orientation {
    /// Quarter turns clockwise, 0 to 3.
    turns: u8,
    /// Mirrored left to right, before turning.
    mirrored: bool,
}

impl Orientation {
    /// As decoded.
    pub const UPRIGHT: Self = Self {
        turns: 0,
        mirrored: false,
    };

    /// `turns` quarter turns clockwise (negative ones go the other way),
    /// after mirroring if `mirrored`.
    pub fn new(turns: i32, mirrored: bool) -> Self {
        Self {
            turns: turns.rem_euclid(4) as u8,
            mirrored,
        }
    }

    /// On its side: width and height trade places.
    pub fn sideways(self) -> bool {
        self.turns % 2 == 1
    }

    /// Then turned `quarter_turns` more clockwise (negative: the other way).
    pub fn rotated(self, quarter_turns: i32) -> Self {
        Self::new(i32::from(self.turns) + quarter_turns, self.mirrored)
    }

    /// Then flipped left to right, as it now shows.
    pub fn flipped_horizontally(self) -> Self {
        Self::new(-i32::from(self.turns), !self.mirrored)
    }

    /// Then flipped upside down, as it now shows.
    pub fn flipped_vertically(self) -> Self {
        self.flipped_horizontally().rotated(2)
    }

    /// `before` first, then this one.
    pub fn after(self, before: Self) -> Self {
        let turns = if self.mirrored {
            i32::from(self.turns) - i32::from(before.turns)
        } else {
            i32::from(self.turns) + i32::from(before.turns)
        };
        Self::new(turns, self.mirrored != before.mirrored)
    }

    /// Where it takes a point, as `[[xx, xy], [yx, yy]]` (`x' = xx x + xy y`),
    /// with x to the right and y down from the picture's center.
    pub fn matrix(self) -> [[f32; 2]; 2] {
        let sign = if self.mirrored { -1.0 } else { 1.0 };
        // A quarter turn clockwise takes (x, y) to (-y, x); mirroring
        // first negates x.
        match self.turns {
            0 => [[sign, 0.0], [0.0, 1.0]],
            1 => [[0.0, -1.0], [sign, 0.0]],
            2 => [[-sign, 0.0], [0.0, -1.0]],
            _ => [[0.0, 1.0], [-sign, 0.0]],
        }
    }

    /// What a video stream asks for, from the display matrix in its codec
    /// parameters (upright without one). Turns that aren't quarter turns go
    /// to the nearest one.
    pub fn of(parameters: &codec::Parameters) -> Self {
        // SAFETY: the parameters and their side data are valid while
        // `parameters` is; the matrix's size is checked before it's read.
        unsafe {
            let parameters = &*parameters.as_ptr();
            let side = ffi::av_packet_side_data_get(
                parameters.coded_side_data,
                parameters.nb_coded_side_data,
                ffi::AVPacketSideDataType::AV_PKT_DATA_DISPLAYMATRIX,
            );
            if side.is_null() || (*side).data.is_null() || (*side).size < 9 * 4 {
                return Self::UPRIGHT;
            }
            let matrix = std::ptr::read_unaligned((*side).data as *const [i32; 9]);
            Self::from_display_matrix(&matrix).unwrap_or_default()
        }
    }

    /// The nearest of the eight to FFmpeg's display matrix, which takes a
    /// pixel (p, q) to (a p + c q, b p + d q) with a, b, c, d its first two
    /// columns of the first two rows. `None` when it squashes the picture
    /// flat.
    pub fn from_display_matrix(matrix: &[i32; 9]) -> Option<Self> {
        let fixed = |i: usize| f64::from(matrix[i]) / 65536.0;
        let wanted = [[fixed(0), fixed(3)], [fixed(1), fixed(4)]];
        if (wanted[0][0] * wanted[1][1] - wanted[0][1] * wanted[1][0]).abs() < 1e-6 {
            return None;
        }
        (0..8).map(|i| Self::new(i % 4, i >= 4)).max_by(|a, b| {
            let likeness = |o: Self| {
                let m = o.matrix();
                (0..2)
                    .flat_map(|r| (0..2).map(move |c| (r, c)))
                    .map(|(r, c)| f64::from(m[r][c]) * wanted[r][c])
                    .sum::<f64>()
            };
            likeness(*a).total_cmp(&likeness(*b))
        })
    }
}

#[cfg(test)]
mod tests {
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
}
