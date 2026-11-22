use super::*;

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
    let mut fitting = Fitting::default();
    assert!(
        fitting.fit(&frame).unwrap().is_none(),
        "as they are before the window opens"
    );

    // The window: 480 by 270 points, at 2x.
    fitting.window = Some((480.0, 270.0, 2.0));
    assert_eq!(size(fitting.fit(&frame).unwrap()), (960, 540));
    // A bitmap subtitle of opaque white over all of it.
    let white = RenderImage::new(vec![image::Frame::new(
        image::RgbaImage::from_raw(1, 1, vec![255; 4]).unwrap(),
    )]);
    let subtitles = Subtitles {
        size: fitting.size((1920, 1080)),
        text: Vec::new(),
        bitmaps: vec![(
            Arc::new(white),
            [0.0, 0.0, 1.0, 1.0],
            gpui::ContentTransform::IDENTITY,
        )],
        text_system: cx.text_system().clone(),
    };
    fitting.set_subtitles(Some(subtitles));
    let burned = fitting.fit(&frame).unwrap();
    pixels(burned.as_ref().unwrap(), |pixel| {
        assert_eq!(pixel[..3], [255; 3], "drawn in");
    });
    assert_eq!(size(burned), (960, 540), "subtitles came");
    fitting.set_subtitles(None);
    assert_eq!(size(fitting.fit(&frame).unwrap()), (960, 540), "and went");

    let fits = decoded.get(kCVPixelFormatType_32BGRA, 960, 540).unwrap();
    assert!(
        fitting.fit(&fits).unwrap().is_none(),
        "one that fits as it is"
    );
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
    // Paused from the window, which says so first, then the tile.
    told.lock().playing = false;
    keep(false, 0.1);
    assert_eq!(rate(), 0.0, "stopped with playback");
    // Played and paused from the tile, then played from the window.
    keep(true, 0.1);
    keep(false, 0.2);
    told.lock().playing = true;
    keep(true, 0.2);
    assert_eq!(rate(), 1.0, "running again");
    // SAFETY: made above, and done with.
    unsafe { CFRelease(timebase) };
}
