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
        "a paused block looks ten times a second"
    );
}

/// Cues laid out are held, so one made after another is let go of isn't
/// taken for it where it is made in the same place.
#[test]
fn a_cue_laid_out_is_told_from_the_next_one() {
    let cue = || {
        Arc::new(CueContent {
            texts: Vec::new(),
            bitmaps: Vec::new(),
        })
    };
    let first = cue();
    let laid = vec![first.clone()];
    assert!(same_cues(&laid, std::slice::from_ref(&first)));
    drop(first);
    // Made as the first is let go of, as the next cue is.
    let next = cue();
    assert!(!same_cues(&laid, std::slice::from_ref(&next)));
    assert!(!same_cues(&laid, &[]));
    assert!(!same_cues(&laid, &[laid[0].clone(), next]));
}
