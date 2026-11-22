use super::*;

#[test]
fn fits_the_picture_in_even_numbers() {
    assert_eq!(fit_size(1920, 1080, None), (1920, 1080));
    assert_eq!(fit_size(3840, 2160, Some((1920, 1080))), (1920, 1080));
    assert_eq!(fit_size(1920, 800, Some((1280, 720))), (1280, 534));
    assert_eq!(fit_size(161, 121, None), (162, 122));
}
