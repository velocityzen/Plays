use std::time::Duration;

use gpui::{point, px, size};

use super::*;

fn screen(display: u32, x: f32) -> Screen {
    Screen {
        display,
        bounds: Bounds::new(point(px(x), px(0.)), size(px(1000.), px(800.))),
    }
}

fn on(display: u32, x: f32) -> Spot {
    Spot {
        display,
        at: Some(point(px(x), px(400.))),
    }
}

#[test]
fn covers_every_screen_but_those_full_screen_and_the_one_the_mouse_shows() {
    let screens = [screen(1, 0.), screen(2, 0.), screen(3, 0.)];
    let displays = |covered: Vec<Screen>| -> Vec<u32> {
        covered.iter().map(|screen| screen.display).collect()
    };
    assert_eq!(
        displays(to_cover(&screens, &[], None)),
        [0u32; 0],
        "none full screen"
    );
    assert_eq!(displays(to_cover(&screens, &[on(1, 500.)], None)), [2, 3]);
    assert_eq!(
        displays(to_cover(&screens, &[on(1, 500.), on(3, 500.)], None)),
        [2],
        "full screen on two"
    );
    let mouse = Spot {
        display: 2,
        at: None,
    };
    assert_eq!(
        displays(to_cover(&screens, &[on(1, 500.)], Some(mouse))),
        [3],
        "the mouse shows the one it is on"
    );

    // X11: monitors side by side on one display.
    let monitors = [screen(0, 0.), screen(0, 1000.)];
    let covered = to_cover(&monitors, &[on(0, 1500.)], None);
    assert_eq!(covered, [screen(0, 0.)], "the monitor without it");
    let covered = to_cover(&monitors, &[on(0, 1000.)], None);
    assert_eq!(
        covered,
        [screen(0, 0.)],
        "a window over both, centered on their edge, is on the one right of it"
    );
}

#[test]
fn the_mouse_shows_a_screen_from_moving_or_clicking_until_it_rests() {
    let mut peek = Peek::default();
    let start = Instant::now();
    let mouse = |x: f64, pressed: bool| Mouse {
        spot: Spot {
            display: 2,
            at: None,
        },
        at: (x, 0.),
        pressed,
    };
    let later = |seconds: f64| start + Duration::from_secs_f64(seconds);

    assert_eq!(peek.showing(None, start), None, "where, unknown");
    assert_eq!(peek.showing(Some(mouse(0., false)), start), None, "resting");
    assert!(
        peek.showing(Some(mouse(5., false)), later(0.1)).is_some(),
        "moved"
    );
    let rested = later(0.1) + BLACKOUT_PEEK;
    assert!(peek.showing(Some(mouse(5., false)), later(0.2)).is_some());
    assert_eq!(peek.showing(Some(mouse(5., false)), rested), None, "rested");
    assert!(
        peek.showing(Some(mouse(5., true)), rested).is_some(),
        "clicked"
    );
}
