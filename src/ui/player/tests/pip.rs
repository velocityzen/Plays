use super::*;

#[test]
fn the_pump_waits_for_the_next_frame_and_idles_while_paused() {
    let ms = Duration::from_millis;
    assert_eq!(
        pump_wait(true, true, Some(ms(33))),
        ms(33),
        "the next frame"
    );
    assert_eq!(pump_wait(true, true, Some(ms(0))), PUMP_LEAST, "one late");
    assert_eq!(pump_wait(true, true, Some(ms(900))), PUMP_IDLE, "far off");
    assert_eq!(pump_wait(true, true, None), PUMP_WAITING, "being decoded");
    assert_eq!(pump_wait(false, true, Some(ms(33))), PUMP_IDLE, "paused");
    assert_eq!(pump_wait(true, false, None), PUMP_IDLE, "a cover");
    assert!(
        PUMP_IDLE >= ms(100),
        "a paused tile looks ten times a second"
    );
}
