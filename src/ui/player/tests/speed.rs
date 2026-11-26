use super::*;

#[test]
fn faster_and_slower_go_through_the_speeds_and_stop_at_their_ends() {
    let mut up = vec![MIN_SPEED];
    while *up.last().unwrap() < MAX_SPEED {
        up.push(next_speed(*up.last().unwrap(), true));
    }
    assert_eq!(up, SPEEDS);
    assert_eq!(next_speed(MAX_SPEED, true), MAX_SPEED);
    let mut down = vec![MAX_SPEED];
    while *down.last().unwrap() > MIN_SPEED {
        down.push(next_speed(*down.last().unwrap(), false));
    }
    down.reverse();
    assert_eq!(down, SPEEDS);
    assert_eq!(next_speed(MIN_SPEED, false), MIN_SPEED);
}

#[test]
fn from_between_two_speeds_they_go_to_the_next_either_way() {
    assert_eq!(next_speed(1.1, true), 1.25);
    assert_eq!(next_speed(1.1, false), 1.0);
    assert_eq!(next_speed(2.5, true), 3.0);
    assert_eq!(next_speed(2.5, false), 2.0);
    // What a speed kept as `f32` reads back as is that speed.
    assert_eq!(next_speed(f64::from(1.25f32), true), 1.5);
}

#[test]
fn a_speed_says_how_many_times_as_fast() {
    assert_eq!(speed_text(1.0), "1×");
    assert_eq!(speed_text(0.25), "0.25×");
    assert_eq!(speed_text(1.5), "1.5×");
    assert_eq!(speed_text(f64::from(1.1f32)), "1.1×");
}
