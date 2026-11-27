//! Which way up a picture goes: the quarter turns and mirroring a video
//! asks for in its display matrix (phones record sideways and say so), and
//! the ones the viewer adds.

use ffmpeg::codec;
use ffmpeg_next as ffmpeg;
use ffmpeg_safe::DisplayMatrix;

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

    /// Quarter turns clockwise, 0 to 3, after mirroring.
    pub fn clockwise_turns(self) -> u8 {
        self.turns
    }

    /// Mirrored left to right, before turning.
    pub fn is_mirrored(self) -> bool {
        self.mirrored
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
        ffmpeg_safe::coded::<DisplayMatrix>(parameters)
            .and_then(|matrix| matrix.turns())
            .map_or(Self::UPRIGHT, |(turns, mirrored)| {
                Self::new(turns.into(), mirrored)
            })
    }
}

#[cfg(test)]
#[path = "tests/orientation.rs"]
mod tests;
