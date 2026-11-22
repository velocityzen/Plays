use super::*;

const ROOM: Size<Pixels> = size(px(1920.), px(1080.));

#[test]
fn one_video_at_its_own_size() {
    let cells = [size(px(1280.), px(720.))];
    assert_eq!(grid_size(&cells, 1, 1, ROOM), Some(cells[0]));
}

#[test]
fn too_big_for_the_screen_shrinks_to_fit() {
    let cells = [size(px(3840.), px(2160.))];
    assert_eq!(
        grid_size(&cells, 1, 1, ROOM),
        Some(size(px(1920.), px(1080.)))
    );
}

#[test]
fn several_share_the_biggest_cell() {
    let cells = [size(px(640.), px(360.)), size(px(480.), px(360.))];
    // Two side by side, with the gap between them.
    assert_eq!(
        grid_size(&cells, 2, 2, ROOM),
        Some(size(px(640. * 2.) + BLOCK_GAP, px(360.)))
    );
    // Four in two rows, one size unknown yet.
    let four = grid_size(&cells, 4, 2, ROOM).unwrap();
    assert_eq!(four.height, px(720.) + BLOCK_GAP);
}

#[test]
fn asked_a_zoomed_window_is_sized_too_but_never_one_in_full_screen() {
    use Sizing::{Asked, Opening};
    assert!(may_size(Opening, false, false));
    assert!(!may_size(Opening, false, true), "a zoomed window is let be");
    assert!(may_size(Asked, false, false));
    assert!(may_size(Asked, false, true), "asked, a zoomed one is sized");
    assert!(!may_size(Asked, true, false));
    assert!(!may_size(Opening, true, false));
}

#[test]
fn nothing_known_leaves_the_window_be() {
    assert_eq!(grid_size(&[], 2, 2, ROOM), None);
}
