use super::*;
use crate::media::hdr;
use crate::media::picture::gamma_table;
use crate::ui::framing::{Adjustments, CROPS, Ratio};

/// How the block frames a picture of `size`, upright, adjusted so.
fn framing(size: (u32, u32), aspect: Option<Ratio>, crop: Option<Ratio>) -> Framing {
    let adjustments = Adjustments {
        aspect,
        crop,
        ..Adjustments::default()
    };
    Framing::new(size, Orientation::UPRIGHT, &adjustments).unwrap()
}

#[test]
fn the_layers_are_looked_through_when_the_video_moves_and_now_and_then() {
    let mut lookout = Lookout::default();
    let start = Instant::now();
    let here = [0.0, 0.0, 480.0, 270.0, 480.0, 270.0, 2.0];
    assert!(lookout.due(here, start), "the first time");
    assert!(!lookout.due(here, start + Duration::from_millis(16)));
    assert!(!lookout.due(here, start + Duration::from_millis(400)));
    let bigger = [0.0, 0.0, 640.0, 360.0, 640.0, 360.0, 2.0];
    assert!(
        lookout.due(bigger, start + Duration::from_millis(420)),
        "resized"
    );
    assert!(!lookout.due(bigger, start + Duration::from_millis(440)));
    assert!(
        lookout.due(bigger, start + Duration::from_millis(920)),
        "a while on"
    );
}

/// Each of a BGRA buffer's pixels, read or written in turn.
fn pixels(buffer: &CVPixelBuffer, mut each: impl FnMut(&mut [u8; 4])) {
    // SAFETY: locked while its rows are touched, within its stride.
    unsafe {
        assert_eq!(buffer.lock_base_address(0), 0);
        let base = buffer.get_base_address().cast::<u8>();
        let stride = buffer.get_bytes_per_row();
        for y in 0..buffer.get_height() {
            let row = slice::from_raw_parts_mut(base.add(y * stride), buffer.get_width() * 4);
            row.as_chunks_mut::<4>().0.iter_mut().for_each(&mut each);
        }
        buffer.unlock_base_address(0);
    }
}

#[gpui::test]
fn frames_go_at_the_windows_size_as_subtitles_come_and_go(cx: &mut gpui::TestAppContext) {
    let mut decoded = Buffers::default();
    let frame = decoded.get(kCVPixelFormatType_32BGRA, 1920, 1080).unwrap();
    pixels(&frame, |pixel| *pixel = [0, 0, 0, 255]);
    let size = |buffer: Option<CVPixelBuffer>| {
        let buffer = buffer.expect("scaled");
        (buffer.get_width(), buffer.get_height())
    };
    let as_it_is = framing((1920, 1080), None, None);
    let mut fitting = Fitting::default();
    let fit = |fitting: &mut Fitting, frame: &CVPixelBuffer| {
        fitting
            .fit(frame, &as_it_is, 1.0, Orientation::UPRIGHT, None)
            .unwrap()
    };
    assert!(
        fit(&mut fitting, &frame).is_none(),
        "as they are before the window opens"
    );

    // The window: 480 by 270 points, at 2x.
    fitting.window = Some((480.0, 270.0, 2.0));
    assert_eq!(size(fit(&mut fitting, &frame)), (960, 540));
    // A bitmap subtitle of opaque white over all of it.
    let white = RenderImage::new(vec![image::Frame::new(
        image::RgbaImage::from_raw(1, 1, vec![255; 4]).unwrap(),
    )]);
    let subtitles = Subtitles {
        size: fitting.size((1920, 1080), &as_it_is),
        text: Vec::new(),
        bitmaps: vec![(
            Arc::new(white),
            [0.0, 0.0, 1.0, 1.0],
            gpui::ContentTransform::IDENTITY,
        )],
        text_system: cx.text_system().clone(),
    };
    fitting.set_subtitles(Some(subtitles));
    let burned = fit(&mut fitting, &frame);
    pixels(burned.as_ref().unwrap(), |pixel| {
        assert_eq!(pixel[..3], [255; 3], "drawn in");
    });
    assert_eq!(size(burned), (960, 540), "subtitles came");
    fitting.set_subtitles(None);
    assert_eq!(size(fit(&mut fitting, &frame)), (960, 540), "and went");

    let fits = decoded.get(kCVPixelFormatType_32BGRA, 960, 540).unwrap();
    assert!(fit(&mut fitting, &fits).is_none(), "one that fits as it is");
}

#[test]
fn frames_are_cropped_and_shaped_as_the_block_shows_them() {
    // 1920 × 1080, green in the middle square and red around it.
    let mut decoded = Buffers::default();
    let frame = decoded.get(kCVPixelFormatType_32BGRA, 1920, 1080).unwrap();
    let mut at = 0;
    pixels(&frame, |pixel| {
        let x = at % 1920;
        *pixel = if (420..1500).contains(&x) {
            [0, 255, 0, 255]
        } else {
            [0, 0, 255, 255]
        };
        at += 1;
    });
    let square = framing((1920, 1080), None, Some(CROPS[0]));
    let four_three = framing((1920, 1080), Some(ASPECT_4_3), None);
    let mut fitting = Fitting::default();
    let fitted = |fitting: &mut Fitting, framing: &Framing| {
        let buffer = fitting
            .fit(&frame, framing, 1.0, Orientation::UPRIGHT, None)
            .unwrap()
            .expect("made anew");
        let (width, height) = (buffer.get_width(), buffer.get_height());
        let mut colors = Vec::new();
        let mut at = 0;
        pixels(&buffer, |pixel| {
            let x = at % width;
            // The edges may blend in a little of what's next to them.
            if (4..width - 4).contains(&x) {
                colors.push((x * 3 / width, pixel[..3].to_vec()));
            }
            at += 1;
        });
        ((width, height), colors)
    };
    let (size, colors) = fitted(&mut fitting, &square);
    assert_eq!(
        size,
        (1080, 1080),
        "as tall as it was, before the window opens"
    );
    assert!(
        colors
            .iter()
            .all(|(_, color)| color[1] > 250 && color[2] < 5)
    );
    let (size, colors) = fitted(&mut fitting, &four_three);
    assert_eq!(size, (1440, 1080), "squeezed to 4:3");
    let third = |third: usize| colors.iter().filter(move |(at, _)| *at == third);
    assert!(third(0).any(|(_, color)| color[2] > 250), "red to the left");
    assert!(
        third(1).all(|(_, color)| color[1] > 250),
        "green in the middle"
    );
    // The window: 480 by 270 points, at 2x.
    fitting.window = Some((480.0, 270.0, 2.0));
    assert_eq!(fitted(&mut fitting, &square).0, (540, 540));
    assert_eq!(fitted(&mut fitting, &four_three).0, (720, 540));
    let key = CFString::from(CVImageBufferKeys::CleanAperture);
    assert!(
        frame.as_buffer().get_attachment(&key, None).is_none(),
        "the frame is left as it was"
    );
}

#[test]
fn frames_converted_smaller_stay_as_big_as_they_are_until_the_window_opens() {
    // A 1920 × 1080 video, converted at half that.
    let fitting = Fitting::default();
    let half = (960, 540);
    assert_eq!(fitting.size(half, &framing((1920, 1080), None, None)), half);
    // Cropped to 2.39:1, as tall as what's kept.
    let wide = framing((1920, 1080), None, Some(CROPS[4]));
    assert_eq!(fitting.size(half, &wide), (960, 402));
    assert_eq!(fitting.size((0, 540), &wide), (0, 540), "nothing to show");
}

/// 4:3, as the menus have it.
const ASPECT_4_3: Ratio = crate::ui::framing::ASPECT_RATIOS[1];

/// A 4 × 4 biplanar buffer of `format`, one luma and one chroma sample
/// throughout, tagged with `transfer` if it's given.
fn planes<S: Sample>(
    format: u32,
    luma: S,
    chroma: S,
    transfer: Option<CFStringRef>,
) -> CVPixelBuffer {
    let mut decoded = Buffers::default();
    let buffer = decoded.get(format, 4, 4).unwrap();
    each_sample(&buffer, |plane, sample: &mut S| {
        *sample = if plane == 0 { luma } else { chroma };
    });
    if let Some(transfer) = transfer {
        // SAFETY: the buffer and CoreVideo's constants are valid.
        unsafe {
            CVBufferSetAttachment(
                buffer.as_concrete_TypeRef(),
                CFString::from(CVImageBufferKeys::TransferFunction).as_concrete_TypeRef(),
                transfer.cast(),
                kCVAttachmentMode_ShouldPropagate,
            );
        }
    }
    buffer
}

/// Each sample of a biplanar buffer's pixels, by plane, read or written.
fn each_sample<S: Sample>(buffer: &CVPixelBuffer, mut each: impl FnMut(usize, &mut S)) {
    // SAFETY: locked while its planes are touched, within their strides.
    unsafe {
        assert_eq!(buffer.lock_base_address(0), 0);
        for plane in 0..2 {
            let base = buffer.get_base_address_of_plane(plane).cast::<S>();
            let stride = buffer.get_bytes_per_row_of_plane(plane) / size_of::<S>();
            let wide = buffer.get_width_of_plane(plane) * (plane + 1);
            for y in 0..buffer.get_height_of_plane(plane) {
                let row = slice::from_raw_parts_mut(base.add(y * stride), wide);
                row.iter_mut().for_each(|sample| each(plane, sample));
            }
        }
        buffer.unlock_base_address(0);
    }
}

#[test]
fn a_surfaces_gamma_is_bent_into_its_frames_and_not_into_the_surface() {
    let upright = framing((4, 4), None, None);
    let mut fitting = Fitting::default();
    let mut bent = |frame: &CVPixelBuffer, gamma| {
        let bent = fitting
            .fit(frame, &upright, gamma, Orientation::UPRIGHT, None)
            .unwrap()
            .expect("a copy");
        let mut luma = Vec::new();
        each_sample(&bent, |plane, sample: &mut u16| {
            if plane == 0 {
                luma.push(sample.code() >> 6);
            }
        });
        luma
    };
    // SDR, 10 bits: a quarter of the way from black to white lifts to half.
    let quarter: u16 = 64 + 876 / 4;
    let sdr = planes::<u16>(
        kCVPixelFormatType_420YpCbCr10BiPlanarVideoRange,
        quarter << 6,
        512 << 6,
        None,
    );
    let half = 64.0 + 876.0 * 0.5;
    assert!(
        bent(&sdr, 2.0)
            .iter()
            .all(|&code| (f64::from(code) - half).abs() <= 2.0)
    );
    let mut kept = Vec::new();
    each_sample(&sdr, |plane, sample: &mut u16| {
        if plane == 0 {
            kept.push(*sample >> 6);
        }
    });
    assert!(
        kept.iter().all(|&code| code == quarter),
        "the surface keeps its own"
    );
    // PQ keeps SDR white where it is.
    let white = (64.0 + 876.0 * hdr::encode([1.0; 3], Transfer::Pq)[0]).round() as u16;
    // SAFETY: CoreVideo's constant.
    let pq = unsafe { kCVImageBufferTransferFunction_SMPTE_ST_2084_PQ };
    let pq_frame = planes::<u16>(
        kCVPixelFormatType_420YpCbCr10BiPlanarVideoRange,
        white << 6,
        512 << 6,
        Some(pq),
    );
    assert!(
        bent(&pq_frame, 2.0)
            .iter()
            .all(|&code| code.abs_diff(u32::from(white)) <= 2)
    );
    assert!(
        fitting
            .fit(&pq_frame, &upright, 1.0, Orientation::UPRIGHT, None)
            .unwrap()
            .is_none(),
        "none to bend"
    );
}

#[test]
fn converted_frames_bend_by_the_same_table_as_converting_them() {
    let mut decoded = Buffers::default();
    let frame = decoded.get(kCVPixelFormatType_32BGRA, 8, 8).unwrap();
    pixels(&frame, |pixel| *pixel = [64, 128, 200, 255]);
    let mut fitting = Fitting::default();
    let bent = fitting
        .fit(
            &frame,
            &framing((8, 8), None, None),
            1.7,
            Orientation::UPRIGHT,
            None,
        )
        .unwrap()
        .expect("a copy");
    let table = gamma_table(1.7);
    pixels(&bent, |pixel| {
        let expected = [table[64], table[128], table[200], 255];
        assert!(
            pixel.iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= 1),
            "{pixel:?} for {expected:?}"
        );
    });
}

/// Each of a BGRA buffer's pixels, by row.
fn rows(buffer: &CVPixelBuffer) -> Vec<Vec<[u8; 4]>> {
    let width = buffer.get_width();
    let mut rows = vec![Vec::new(); buffer.get_height()];
    let mut at = 0;
    pixels(buffer, |pixel| {
        rows[at / width].push(*pixel);
        at += 1;
    });
    rows
}

#[test]
fn frames_turn_on_the_cpu_as_on_the_gpu_where_it_cant() {
    // 4 × 2, each pixel its own.
    let mut decoded = Buffers::default();
    let frame = decoded.get(kCVPixelFormatType_32BGRA, 4, 2).unwrap();
    let mut at = 0u8;
    pixels(&frame, |pixel| {
        *pixel = [at * 30, 255 - at * 30, at * 7, 255];
        at += 1;
    });
    let decoded_rows = rows(&frame);
    let mut gpu = Turner::new().ok();
    for turns in 0..4 {
        for mirrored in [false, true] {
            let orientation = Orientation::new(turns, mirrored);
            let adjustments = Adjustments {
                orientation,
                ..Adjustments::default()
            };
            let framing = Framing::new((4, 2), Orientation::UPRIGHT, &adjustments).unwrap();
            let mut fitting = Fitting::default();
            let turned = fitting
                .fit(&frame, &framing, 1.0, orientation, None)
                .unwrap();
            // Upright, it goes as it is.
            let turned = rows(&turned.unwrap_or_else(|| frame.clone()));
            let (across, down) = across(orientation, (4, 2));
            assert_eq!(
                (turned[0].len(), turned.len()),
                (across, down),
                "{orientation:?}"
            );
            // Each pixel where the block shows it.
            for (y, row) in decoded_rows.iter().enumerate() {
                for (x, pixel) in row.iter().enumerate() {
                    let center = [(x as f32 + 0.5) / 4.0, (y as f32 + 0.5) / 2.0, 0.0, 0.0];
                    let [shown_x, shown_y, ..] = framing.place(center).0;
                    let (column, line) = (
                        (shown_x * across as f32) as usize,
                        (shown_y * down as f32) as usize,
                    );
                    assert_eq!(turned[line][column], *pixel, "{orientation:?} at {x}, {y}");
                }
            }
            // And as the GPU turns it, where it can.
            if let Some(gpu) = &mut gpu {
                let on_gpu = gpu.turn(&frame, orientation).unwrap();
                assert_eq!(rows(&on_gpu), turned, "{orientation:?}");
            }
        }
    }
}

#[test]
fn frames_turned_on_the_cpu_are_cropped_as_they_show() {
    // 1920 × 1080, green in the middle square and red around it.
    let mut decoded = Buffers::default();
    let frame = decoded.get(kCVPixelFormatType_32BGRA, 1920, 1080).unwrap();
    let mut at = 0;
    pixels(&frame, |pixel| {
        let x = at % 1920;
        *pixel = if (420..1500).contains(&x) {
            [0, 255, 0, 255]
        } else {
            [0, 0, 255, 255]
        };
        at += 1;
    });
    // Turned, it's 1080 across and 1920 down, and cropped square it keeps
    // the middle of what was across.
    let turned = Orientation::new(1, false);
    let adjustments = Adjustments {
        orientation: turned,
        crop: Some(CROPS[0]),
        ..Adjustments::default()
    };
    let framing = Framing::new((1920, 1080), Orientation::UPRIGHT, &adjustments).unwrap();
    let mut fitting = Fitting::default();
    let fitted = fitting
        .fit(&frame, &framing, 1.0, turned, None)
        .unwrap()
        .expect("made anew");
    let (width, height) = (fitted.get_width(), fitted.get_height());
    assert_eq!((width, height), (1080, 1080));
    let mut at = 0;
    pixels(&fitted, |pixel| {
        let (x, y) = (at % width, at / width);
        // The edges may blend in a little of what's next to them.
        if (4..width - 4).contains(&x) && (4..height - 4).contains(&y) {
            assert!(pixel[1] > 250 && pixel[2] < 5, "{pixel:?} at {x}, {y}");
        }
        at += 1;
    });
}

/// A 4 × 4 frame in `format`, tagged PQ, its luma samples and chroma
/// pairs each their own, turned a quarter to the left on the CPU.
fn turn_both_planes<S: Sample>(format: u32) {
    // Ten bits are kept high in sixteen.
    let shift = if size_of::<S>() == 2 { 6 } else { 0 };
    // SAFETY: CoreVideo's constant.
    let pq = unsafe { kCVImageBufferTransferFunction_SMPTE_ST_2084_PQ };
    let frame = planes::<S>(format, S::from_code(0), S::from_code(0), Some(pq));
    let mut at = 0;
    each_sample(&frame, |plane, sample: &mut S| {
        let code = if plane == 0 {
            16 + at
        } else {
            100 + at % 2 + 10 * (at / 2)
        };
        *sample = S::from_code(code << shift);
        at = if plane == 0 && at == 15 { 0 } else { at + 1 };
    });
    let turned_left = Orientation::new(-1, false);
    let adjustments = Adjustments {
        orientation: turned_left,
        ..Adjustments::default()
    };
    let framing = Framing::new((4, 4), Orientation::UPRIGHT, &adjustments).unwrap();
    let mut fitting = Fitting::default();
    let turned = fitting
        .fit(&frame, &framing, 1.0, turned_left, None)
        .unwrap()
        .unwrap();
    let (mut luma, mut chroma) = (Vec::new(), Vec::new());
    each_sample(&turned, |plane, sample: &mut S| {
        let code = sample.code() >> shift;
        if plane == 0 {
            luma.push(code)
        } else {
            chroma.push(code)
        }
    });
    // Turned a quarter to the left, the top row is the right column, up.
    assert_eq!(luma[..4], [19, 23, 27, 31], "{format:#x}");
    assert_eq!(
        luma[12..],
        [16, 20, 24, 28],
        "{format:#x}, the bottom the left"
    );
    // Chroma pairs turn as pairs: the right column's on top.
    assert_eq!(
        chroma,
        [110, 111, 130, 131, 100, 101, 120, 121],
        "{format:#x}"
    );
    assert_eq!(transfer_of(&turned), Some(Transfer::Pq), "{format:#x}");
}

#[test]
fn both_planes_of_a_frame_turn_and_its_colors_go_with_them() {
    turn_both_planes::<u8>(kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange);
    turn_both_planes::<u16>(kCVPixelFormatType_420YpCbCr10BiPlanarVideoRange);
}

unsafe extern "C" {
    fn CMTimebaseGetRate(timebase: CMTimebaseRef) -> f64;
}

#[test]
fn the_progress_runs_as_playback_does_whichever_buttons_play_it() {
    let timebase = make_timebase();
    assert!(!timebase.is_null(), "a timebase on the host clock");
    // SAFETY: the timebase lives until the end.
    let rate = || unsafe { CMTimebaseGetRate(timebase) };
    let told = Mutex::new(Playback::default());
    let mut running = None;
    let mut keep = |playing, position| {
        let duration = Some(10.0);
        let playback = Playback {
            playing,
            position,
            duration,
        };
        keep_up(timebase, &mut running, &told, playback);
    };
    keep(true, 0.0);
    assert_eq!(rate(), 1.0, "running with playback");
    // Paused from the window, which says so first, then the block.
    told.lock().playing = false;
    keep(false, 0.1);
    assert_eq!(rate(), 0.0, "stopped with playback");
    // Played and paused from the block, then played from the window.
    keep(true, 0.1);
    keep(false, 0.2);
    told.lock().playing = true;
    keep(true, 0.2);
    assert_eq!(rate(), 1.0, "running again");
    // SAFETY: made above, and done with.
    unsafe { CFRelease(timebase) };
}

#[test]
fn dolby_vision_is_reshaped_into_hdr10_before_gamma_bends_it() {
    let reshaping = ::hdr::testing::reshaping();
    let ycc = Ycc {
        kr: 0.2627,
        kb: 0.0593,
        bits: 10,
        full_range: false,
        shift: 6,
    };
    // 4 × 4 of one IPT color, tagged PQ as Dolby Vision surfaces are.
    let (i, pt) = (0.6, 0.45);
    let code = |value: f32| ((value * 1024.0).round() as u16) << 6;
    // SAFETY: CoreVideo's constant.
    let pq = unsafe { kCVImageBufferTransferFunction_SMPTE_ST_2084_PQ };
    let frame = planes::<u16>(
        kCVPixelFormatType_420YpCbCr10BiPlanarVideoRange,
        code(i),
        code(pt),
        Some(pq),
    );
    let signal = |value: f32| f32::from(code(value)) / 65536.0;
    let light = reshaping.lms_to_rgb(
        reshaping
            .lms_signal([signal(i), signal(pt), signal(pt)])
            .map(hdr::pq_to_nits),
    );
    let reshaped = light.map(hdr::nits_to_pq);
    let upright = framing((4, 4), None, None);
    let mut fitting = Fitting::default();
    for gamma in [1.0, 1.7] {
        let rgb = reshaped.map(|signal| Curve::new(gamma, Some(Transfer::Pq)).at(signal));
        let fitted = fitting
            .fit(
                &frame,
                &upright,
                gamma,
                Orientation::UPRIGHT,
                Some(&reshaping),
            )
            .unwrap()
            .expect("reshaped, though it fits as it is");
        let luma: u16 = ycc.write(ycc.luma(rgb), ycc.luma_range());
        let (cb, cr) = ycc.chroma(rgb);
        let chroma: [u16; 2] = [cb, cr].map(|value| ycc.write(value, ycc.chroma_range()));
        let mut at = 0;
        each_sample(&fitted, |plane, sample: &mut u16| {
            let want = if plane == 0 { luma } else { chroma[at % 2] };
            assert!(
                (*sample >> 6).abs_diff(want >> 6) <= 1,
                "gamma {gamma}, plane {plane}: {} for {}",
                *sample >> 6,
                want >> 6
            );
            at += plane;
        });
        assert_eq!(transfer_of(&fitted), Some(Transfer::Pq), "HDR10");
    }
    each_sample(&frame, |plane, sample: &mut u16| {
        let own = if plane == 0 { code(i) } else { code(pt) };
        assert_eq!(*sample, own, "the surface keeps its own");
    });
}

#[test]
fn dolby_vision_in_bgra_is_no_frame_to_show() {
    let mut decoded = Buffers::default();
    let frame = decoded.get(kCVPixelFormatType_32BGRA, 4, 4).unwrap();
    let reshaping = ::hdr::testing::reshaping();
    let fitted = Fitting::default().fit(
        &frame,
        &framing((4, 4), None, None),
        1.0,
        Orientation::UPRIGHT,
        Some(&reshaping),
    );
    assert!(fitted.is_err());
}
