use super::Toward::*;
use super::*;

/// From each place, in order, where `toward` goes.
fn from_each(count: usize, columns: usize, toward: Toward) -> Vec<Option<usize>> {
    (0..count)
        .map(|index| neighbor(index, count, columns, toward))
        .collect()
}

#[test]
fn across_and_down_a_full_grid_stopping_at_its_edges() {
    // 0 1
    // 2 3
    assert_eq!(from_each(4, 2, Left), [None, Some(0), None, Some(2)]);
    assert_eq!(from_each(4, 2, Right), [Some(1), None, Some(3), None]);
    assert_eq!(from_each(4, 2, Up), [None, None, Some(0), Some(1)]);
    assert_eq!(from_each(4, 2, Down), [Some(2), Some(3), None, None]);
}

#[test]
fn down_into_a_short_last_row_goes_to_its_last_block() {
    // 0 1 2
    // 3 4
    assert_eq!(
        from_each(5, 3, Down),
        [Some(3), Some(4), Some(4), None, None]
    );
    assert_eq!(
        from_each(5, 3, Right),
        [Some(1), Some(2), None, Some(4), None]
    );
    assert_eq!(from_each(5, 3, Up), [None, None, None, Some(0), Some(1)]);
}

#[test]
fn previous_and_next_go_round_the_order() {
    assert_eq!(from_each(3, 2, Next), [Some(1), Some(2), Some(0)]);
    assert_eq!(from_each(3, 2, Previous), [Some(2), Some(0), Some(1)]);
}

#[test]
fn one_block_or_one_row_goes_nowhere_it_has_none() {
    for toward in [Left, Right, Up, Down, Previous, Next] {
        assert_eq!(from_each(1, 1, toward), [None], "{toward:?}");
    }
    // More columns than blocks: one row.
    assert_eq!(from_each(2, 4, Down), [None, None]);
    assert_eq!(from_each(2, 4, Right), [Some(1), None]);
    assert_eq!(neighbor(0, 0, 2, Next), None);
}
