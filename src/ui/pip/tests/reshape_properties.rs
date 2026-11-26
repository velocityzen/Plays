use hegel::TestCase;
use hegel::generators as gs;

use super::*;
use crate::media::dovi::tests::reshaping;

/// A 4:2:0 frame of any size, its rows any stride apart: luma and chroma
/// planes, their strides, its width and how its samples are written.
struct Frame {
    luma: (Vec<u16>, usize),
    chroma: (Vec<u16>, usize),
    width: usize,
    ycc: Ycc,
}

/// What's past each row's samples.
const PAST: u16 = 0xabcd;

/// `sample` gives each, of a plane (0 for luma) at a place across.
fn frame(tc: &TestCase, sample: impl Fn(&TestCase, usize, usize) -> u16) -> Frame {
    let width = tc.draw(gs::integers::<usize>().min_value(1).max_value(9));
    let height = tc.draw(gs::integers::<usize>().min_value(1).max_value(9));
    let plane = |index: usize, wide: usize, rows: usize| {
        let stride = wide + tc.draw(gs::integers::<usize>().max_value(3));
        let samples = (0..rows)
            .flat_map(|_| {
                let mut row: Vec<u16> = (0..wide).map(|x| sample(tc, index, x)).collect();
                row.resize(stride, PAST);
                row
            })
            .collect();
        (samples, stride)
    };
    let luma = plane(0, width, height);
    let chroma = plane(1, width.div_ceil(2) * 2, height.div_ceil(2));
    let ycc = Ycc {
        kr: 0.2627,
        kb: 0.0593,
        bits: 10,
        full_range: tc.draw(gs::booleans()),
        shift: 6,
    };
    Frame {
        luma,
        chroma,
        width,
        ycc,
    }
}

impl Frame {
    fn reshape(&mut self) {
        let (luma, luma_stride) = &mut self.luma;
        let (chroma, chroma_stride) = &mut self.chroma;
        reshape(
            (&reshaping(), &Pq::default()),
            (luma, *luma_stride),
            (chroma, *chroma_stride),
            self.width,
            self.ycc,
        );
    }

    /// Each plane's samples, row by row, without what's past them.
    fn samples(&self) -> [Vec<u16>; 2] {
        let wide = [self.width, self.width.div_ceil(2) * 2];
        [&self.luma, &self.chroma]
            .into_iter()
            .zip(wide)
            .map(|((plane, stride), wide)| {
                plane
                    .chunks(*stride)
                    .flat_map(|row| row[..wide].to_vec())
                    .collect()
            })
            .collect::<Vec<_>>()
            .try_into()
            .unwrap()
    }

    /// Whether what's past each row is as it was.
    fn untouched(&self) -> bool {
        let wide = [self.width, self.width.div_ceil(2) * 2];
        [&self.luma, &self.chroma]
            .into_iter()
            .zip(wide)
            .all(|((plane, stride), wide)| {
                plane
                    .chunks(*stride)
                    .all(|row| row[wide..].iter().all(|&sample| sample == PAST))
            })
    }
}

#[hegel::test]
fn any_frame_is_reshaped_within_its_rows(tc: TestCase) {
    let mut frame = frame(&tc, |tc, _, _| tc.draw(gs::integers::<u16>()) & !0x3f);
    frame.reshape();
    assert!(frame.untouched());
}

#[hegel::test]
fn a_frame_of_one_color_stays_one_color(tc: TestCase) {
    let codes = [(); 3].map(|_| tc.draw(gs::integers::<u16>().max_value(1023)) << 6);
    let mut frame = frame(&tc, |_, plane, x| match plane {
        0 => codes[0],
        _ => codes[1 + x % 2],
    });
    frame.reshape();
    let [luma, chroma] = frame.samples();
    // Averages of fewer pixels at the edges may round apart.
    let alike = |a: u16, b: u16| (a >> 6).abs_diff(b >> 6) <= 1;
    assert!(luma.iter().all(|&sample| sample == luma[0]), "{luma:?}");
    let pairs = chroma.as_chunks::<2>().0;
    assert!(
        pairs
            .iter()
            .all(|pair| alike(pair[0], pairs[0][0]) && alike(pair[1], pairs[0][1])),
        "{pairs:?}"
    );
}
