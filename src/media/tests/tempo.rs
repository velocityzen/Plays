use super::*;

const RATE: u32 = 48_000;

/// `seconds` of a sine at `hz`, the same in both channels, interleaved.
pub(super) fn tone(hz: f64, seconds: f64) -> Vec<f32> {
    let frames = (seconds * f64::from(RATE)) as usize;
    (0..frames)
        .flat_map(|i| {
            let x = (std::f64::consts::TAU * hz * i as f64 / f64::from(RATE)).sin() as f32;
            [x, x]
        })
        .collect()
}

/// What `stretch` makes of `sound`, from time 0, at `speed`, fed as it
/// wants; and the time it says is heard before each half window.
pub(super) fn play(stretch: &mut Stretch, sound: &[f32], speed: f64) -> (Vec<f32>, Vec<f64>) {
    let mut fed = 0;
    let mut heard = Vec::new();
    let mut times = Vec::new();
    loop {
        let pts = fed as f64 / f64::from(RATE);
        fed += stretch.feed(pts, &sound[fed * SOURCE_CHANNELS..]);
        if !stretch.step(speed) {
            assert!(stretch.wants() > 0, "it steps once fed what it wants");
            if fed * SOURCE_CHANNELS == sound.len() {
                return (heard, times);
            }
            continue;
        }
        times.push(stretch.read_pts().unwrap());
        let ready = stretch.ready().len() / SOURCE_CHANNELS;
        heard.extend_from_slice(stretch.ready());
        stretch.take(ready);
    }
}

/// How many times a second `sound`'s left channel crosses zero going up.
fn pitch(sound: &[f32]) -> f64 {
    let left: Vec<f32> = sound.iter().step_by(SOURCE_CHANNELS).copied().collect();
    let ups = left
        .windows(2)
        .filter(|w| w[0] < 0.0 && w[1] >= 0.0)
        .count();
    ups as f64 * f64::from(RATE) / left.len() as f64
}

#[test]
fn faster_or_slower_it_takes_less_or_more_time_at_the_same_pitch() {
    let sound = tone(440.0, 4.0);
    for speed in [0.25, 0.5, 0.8, 1.25, 1.5, 2.0, 3.0, 4.0] {
        let (heard, _) = play(&mut Stretch::new(RATE), &sound, speed);
        let seconds = heard.len() as f64 / SOURCE_CHANNELS as f64 / f64::from(RATE);
        // All but the last window or so, which waits for more.
        let expected = 4.0 / speed;
        assert!(
            (seconds - expected).abs() < 0.06 + 0.06 / speed,
            "at {speed}: {seconds} s, not {expected}"
        );
        let hz = pitch(&heard);
        assert!((hz - 440.0).abs() < 4.0, "at {speed}: {hz} Hz");
    }
}

#[test]
fn at_its_own_speed_it_s_heard_as_it_is_past_the_first_half_window() {
    let sound = tone(1000.0, 0.5);
    let mut stretch = Stretch::new(RATE);
    let (heard, _) = play(&mut stretch, &sound, 1.0);
    let hop = stretch.hop * SOURCE_CHANNELS;
    assert!(heard.len() > hop * 10);
    for (at, (x, y)) in heard.iter().zip(&sound).enumerate().skip(hop) {
        assert!((x - y).abs() < 1e-5, "at {at}: {x} for {y}");
    }
}

#[test]
fn what_s_heard_moves_at_its_speed() {
    let sound = tone(300.0, 3.0);
    for speed in [0.5, 1.0, 2.0] {
        let mut stretch = Stretch::new(RATE);
        let (_, times) = play(&mut stretch, &sound, speed);
        let step = stretch.hop as f64 / f64::from(RATE) * speed;
        assert_eq!(times[0], 0.0);
        for pair in times.windows(2) {
            assert!(
                (pair[1] - pair[0] - step).abs() < 1e-9,
                "at {speed}: {pair:?}"
            );
        }
    }
}

#[test]
fn its_speed_can_change_as_it_plays() {
    let sound = tone(440.0, 2.0);
    let mut stretch = Stretch::new(RATE);
    let half = sound.len() / 2;
    let (first, _) = play(&mut stretch, &sound[..half], 2.0);
    let before = stretch.read_pts().unwrap();
    let (second, times) = play(&mut stretch, &sound[half..], 0.5);
    assert!(times[0] >= before, "{} then {before}", times[0]);
    assert!((pitch(&first) - 440.0).abs() < 4.0);
    assert!((pitch(&second) - 440.0).abs() < 4.0);
}

#[test]
fn reset_it_holds_nothing_and_takes_its_time_afresh() {
    let sound = tone(440.0, 0.5);
    let mut stretch = Stretch::new(RATE);
    play(&mut stretch, &sound, 1.5);
    assert!(stretch.is_active());
    stretch.reset();
    assert!(!stretch.is_active());
    assert_eq!(stretch.read_pts(), None);
    assert!(stretch.ready().is_empty());
    let wanted = stretch.wants();
    assert_eq!(stretch.feed(10.0, &sound), wanted, "only what it wants");
    assert_eq!(stretch.read_pts(), Some(10.0));
}

#[test]
fn it_steps_only_once_what_s_ready_is_heard() {
    let sound = tone(440.0, 0.5);
    let mut stretch = Stretch::new(RATE);
    let mut fed = stretch.feed(0.0, &sound);
    assert!(stretch.step(1.5));
    fed += stretch.feed(0.0, &sound[fed * SOURCE_CHANNELS..]);
    assert!(stretch.wants() == 0 && fed > 0);
    assert!(!stretch.step(1.5), "half a window is waiting");
    stretch.take(1);
    assert!(!stretch.step(1.5));
    stretch.take(usize::MAX);
    assert!(stretch.ready().is_empty());
    assert!(stretch.step(1.5));
}

#[test]
fn back_at_its_own_speed_it_goes_on_from_where_the_speed_got_it() {
    let sound = tone(440.0, 1.0);
    let mut stretch = Stretch::new(RATE);
    let (_, fast) = play(&mut stretch, &sound[..sound.len() / 2], 4.0);
    let (_, then) = play(&mut stretch, &sound, 1.0);
    let (got, next) = (fast.last().unwrap(), then[0]);
    let hop = 0.02;
    // No further back than the search reaches.
    assert!(next >= got + hop - 0.01, "from {got} back to {next}");
    // It settles onto where the sound goes on, within the search; then
    // it plays on as it is.
    assert!((then[1] - then[0] - hop).abs() <= 0.01);
    for pair in then[1..].windows(2) {
        assert!(
            (pair[1] - pair[0] - hop).abs() < 1e-9,
            "at its own speed: {pair:?}"
        );
    }
}
