use super::*;

fn turned(turns: u8) -> Adjustments {
    Adjustments {
        turns,
        ..Adjustments::default()
    }
}

#[test]
fn shows_a_picture_at_its_aspect_turned_and_cropped() {
    let none = Adjustments::default();
    assert_eq!(none.shown(1920, 1080), (1920.0, 1080.0));
    assert_eq!(turned(1).shown(1920, 1080), (1080.0, 1920.0));
    assert_eq!(turned(2).shown(1920, 1080), (1920.0, 1080.0));
    let four_three = Adjustments {
        aspect: Some(4.0 / 3.0),
        ..none
    };
    assert_eq!(four_three.shown(1920, 1080), (1440.0, 1080.0));
    let square = Adjustments {
        crop: Some(1.0),
        ..none
    };
    assert_eq!(square.shown(1920, 1080), (1080.0, 1080.0));
    // Cropped as it shows: on its side, a wide crop takes its middle.
    let (width, height) = Adjustments {
        turns: 3,
        crop: Some(16.0 / 9.0),
        ..none
    }
    .shown(1920, 1080);
    assert_eq!(width, 1080.0);
    assert!((height - 607.5).abs() < 1e-9, "{height}");
}

#[test]
fn crops_a_decoded_picture_evenly_from_either_side() {
    let none = Adjustments::default();
    assert_eq!(none.cropped(1920, 1080), [0; 4]);
    let square = Adjustments {
        crop: Some(1.0),
        ..none
    };
    assert_eq!(square.cropped(1920, 1080), [420, 0, 420, 0]);
    assert_eq!(square.cropped(1080, 1920), [0, 420, 0, 420]);
    // A wide crop of it on its side takes from what is its top and bottom
    // as it shows: its left and right as decoded.
    let sideways = Adjustments {
        turns: 1,
        crop: Some(16.0 / 9.0),
        ..none
    };
    assert_eq!(sideways.cropped(1920, 1080), [656, 0, 656, 0]);
    // Stretched to 4:3 first, it is narrower than it was.
    let stretched = Adjustments {
        aspect: Some(4.0 / 3.0),
        crop: Some(1.0),
        ..none
    };
    assert_eq!(stretched.cropped(1920, 1080), [240, 0, 240, 0]);
    // An odd amount goes even, the extra pixel kept; an even one whose
    // halves are odd, the extra two.
    assert_eq!(square.cropped(15, 10), [2, 0, 2, 0]);
    assert_eq!(square.cropped(16, 10), [2, 0, 2, 0]);
}

#[test]
fn crops_a_frame_by_moving_into_its_planes() {
    let mut frame = frame::Video::new(Pixel::YUV420P, 64, 36);
    for (plane, value) in [(0, 1u8), (1, 2), (2, 3)] {
        frame.data_mut(plane).fill(value);
    }
    let start = frame.data(0).as_ptr() as usize;
    crop(
        &mut frame,
        &Adjustments {
            crop: Some(1.0),
            ..Adjustments::default()
        },
    );
    assert_eq!((frame.width(), frame.height()), (36, 36));
    assert_eq!(frame.data(0).as_ptr() as usize - start, 14);
    // As it is, it stays.
    crop(&mut frame, &Adjustments::default());
    assert_eq!((frame.width(), frame.height()), (36, 36));
}

/// A plane of one-byte pixels, `width` a row, with `pad` bytes after
/// each row.
fn grid(rows: &[&[u8]], pad: usize) -> (Vec<u8>, Plane) {
    let width = rows[0].len();
    let stride = width + pad;
    let mut bytes = vec![0xee; stride * rows.len()];
    for (y, row) in rows.iter().enumerate() {
        bytes[y * stride..y * stride + width].copy_from_slice(row);
    }
    let plane = Plane {
        stride,
        size: (width, rows.len()),
        pixel: 1,
    };
    (bytes, plane)
}

/// `bytes` turned as `turns` and `mirrored` say, as rows.
fn turned_rows(bytes: &[u8], plane: &Plane, turns: u8, mirrored: bool) -> Vec<Vec<u8>> {
    let (width, height) = plane.size;
    let (width, height) = if turns % 2 == 1 {
        (height, width)
    } else {
        (width, height)
    };
    let stride = width + 3;
    let mut target = vec![0; stride * height];
    turn_plane(bytes, plane, turns, mirrored, &mut target, stride);
    target
        .chunks(stride)
        .map(|row| row[..width].to_vec())
        .collect()
}

#[test]
fn turns_and_mirrors_a_plane_as_a_player_shows_it() {
    let (bytes, plane) = grid(&[&[1, 2, 3], &[4, 5, 6]], 5);
    let turn = |turns, mirrored| turned_rows(&bytes, &plane, turns, mirrored);
    assert_eq!(turn(0, false), [[1, 2, 3], [4, 5, 6]]);
    // A quarter turn clockwise: the left column is the top row.
    assert_eq!(turn(1, false), [[4, 1], [5, 2], [6, 3]]);
    assert_eq!(turn(2, false), [[6, 5, 4], [3, 2, 1]]);
    assert_eq!(turn(3, false), [[3, 6], [2, 5], [1, 4]]);
    // Mirrored left to right, then turned.
    assert_eq!(turn(0, true), [[3, 2, 1], [6, 5, 4]]);
    assert_eq!(turn(1, true), [[6, 3], [5, 2], [4, 1]]);
    assert_eq!(turn(2, true), [[4, 5, 6], [1, 2, 3]]);
    assert_eq!(turn(3, true), [[1, 4], [2, 5], [3, 6]]);
}

#[test]
fn turns_pixels_of_several_bytes_whole() {
    // Two pixels of two bytes a row: a chroma plane's pairs.
    let bytes = [1, 2, 3, 4, 0, 5, 6, 7, 8, 0];
    let plane = Plane {
        stride: 5,
        size: (2, 2),
        pixel: 2,
    };
    let mut target = [0; 8];
    turn_plane(&bytes, &plane, 1, false, &mut target, 4);
    assert_eq!(target, [5, 6, 1, 2, 7, 8, 3, 4]);
}

/// An NV12 frame `width` × `height` whose luma counts up from 1, row
/// after row, and whose chroma pairs are (10 + n, 100 + n).
fn counted(width: u32, height: u32) -> frame::Video {
    let mut frame = frame::Video::new(Pixel::NV12, width, height);
    let (luma, chroma) = (frame.stride(0), frame.stride(1));
    for y in 0..height as usize {
        for x in 0..width as usize {
            frame.data_mut(0)[y * luma + x] = (1 + y * width as usize + x) as u8;
        }
    }
    for y in 0..height as usize / 2 {
        for x in 0..width as usize / 2 {
            let n = (y * width as usize / 2 + x) as u8;
            frame.data_mut(1)[y * chroma + x * 2..][..2].copy_from_slice(&[10 + n, 100 + n]);
        }
    }
    frame
}

/// The rows of `plane` of `frame`, `width` bytes of each.
fn rows_of(frame: &frame::Video, plane: usize, width: usize) -> Vec<Vec<u8>> {
    let stride = frame.stride(plane);
    let height = frame.plane_height(plane) as usize;
    (0..height)
        .map(|y| frame.data(plane)[y * stride..][..width].to_vec())
        .collect()
}

#[test]
fn finishes_a_frame_turned_with_its_chroma() {
    let frame = finish(counted(4, 2), &turned(1));
    assert_eq!((frame.width(), frame.height()), (2, 4));
    assert_eq!(
        rows_of(&frame, 0, 2),
        [[5, 1], [6, 2], [7, 3], [8, 4]],
        "luma"
    );
    assert_eq!(
        rows_of(&frame, 1, 2),
        [[10, 100], [11, 101]],
        "chroma, a pair a pixel of half the size"
    );
    // Mirrored only, it is mirrored all the same.
    let mirrored = finish(
        counted(4, 2),
        &Adjustments {
            mirrored: true,
            ..Adjustments::default()
        },
    );
    assert_eq!(rows_of(&mirrored, 0, 4), [[4, 3, 2, 1], [8, 7, 6, 5]]);
    assert_eq!(rows_of(&mirrored, 1, 4), [[11, 101, 10, 100]]);
    // As it is, it is the frame itself.
    let plain = counted(4, 2);
    let pointer = plain.data(0).as_ptr();
    assert_eq!(
        finish(plain, &Adjustments::default()).data(0).as_ptr(),
        pointer
    );
}

#[test]
fn turns_ten_bit_frames_a_sample_of_two_bytes_at_a_time() {
    let mut frame = frame::Video::new(Pixel::P010LE, 2, 2);
    let stride = frame.stride(0);
    for (at, code) in [(0, 1u16), (2, 2), (stride, 3), (stride + 2, 4)] {
        frame.data_mut(0)[at..at + 2].copy_from_slice(&(code << 6).to_le_bytes());
    }
    // A chroma pair of two samples.
    frame.data_mut(1)[..4].copy_from_slice(&[1, 2, 3, 4]);
    let turned = finish(frame, &turned(1));
    let codes = |row: Vec<u8>| -> Vec<u16> {
        row.chunks(2)
            .map(|sample| u16::from_le_bytes([sample[0], sample[1]]) >> 6)
            .collect()
    };
    let luma: Vec<Vec<u16>> = rows_of(&turned, 0, 4).into_iter().map(codes).collect();
    assert_eq!(luma, [[3, 1], [4, 2]]);
    assert_eq!(&turned.data(1)[..4], [1, 2, 3, 4]);
}

#[test]
fn bends_luma_by_gamma_and_leaves_chroma() {
    let mut frame = frame::Video::new(Pixel::NV12, 4, 2);
    frame.data_mut(0)[..4].copy_from_slice(&[16, 126, 235, 240]);
    frame.data_mut(1)[..4].copy_from_slice(&[90, 160, 91, 161]);
    let lifted = finish(
        frame,
        &Adjustments {
            gamma: 2.0,
            ..Adjustments::default()
        },
    );
    let expected = table(16, 235, 255, 2.0);
    let luma: Vec<u16> = lifted.data(0)[..4].iter().map(|&y| y.into()).collect();
    assert_eq!(luma, [16, 126, 235, 240].map(|code: usize| expected[code]));
    assert_eq!(&lifted.data(1)[..4], [90, 160, 91, 161]);

    // Ten-bit samples in the high bits of 16.
    let mut deep = frame::Video::new(Pixel::P010LE, 2, 2);
    deep.data_mut(0)[..4].copy_from_slice(&[(500u16 << 6).to_le_bytes(), [0, 0]].concat());
    let lifted = finish(
        deep,
        &Adjustments {
            gamma: 0.5,
            ..Adjustments::default()
        },
    );
    let expected = table(64, 940, 1023, 0.5);
    let code = u16::from_le_bytes([lifted.data(0)[0], lifted.data(0)[1]]);
    assert_eq!(code, expected[500] << 6);
}

#[test]
fn gamma_keeps_black_and_white_and_bends_between_them() {
    let lifted = table(16, 235, 255, 2.0);
    assert_eq!((lifted[16], lifted[235]), (16, 235));
    assert_eq!(lifted[126], 171, "the middle, lifted");
    assert!(lifted.windows(2).all(|pair| pair[0] <= pair[1]));
    // Past black and white they bend the same way, within the codes.
    assert_eq!(lifted[0], 0);
    assert_eq!(lifted[255], 245);
    let sunk = table(16, 235, 255, 0.5);
    assert_eq!(sunk[126], 71, "the middle, sunk");
    assert_eq!(sunk[255], 255, "past white, within the codes");
    assert_eq!(table(64, 940, 1023, 1.0), (0..=1023).collect::<Vec<u16>>());
}
