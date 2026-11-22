use super::FocusHistory;

/// `fronts` in front in turn, in that order.
fn history(fronts: &[char]) -> FocusHistory<char> {
    let mut history = FocusHistory::default();
    for &front in fronts {
        history.note(front, |_| false);
    }
    history
}

#[test]
fn recent_is_the_newest_but_the_one_in_front_and_back_walks_on() {
    let open = |_| true;
    let mut fronts = history(&['a', 'b', 'c', 'd']);
    assert_eq!(fronts.recent(Some('d'), open), Some('c'));
    assert_eq!(fronts.recent(None, open), Some('d'), "from none in front");

    let back = |fronts: &mut FocusHistory<char>, front| {
        let to = fronts.back(Some(front), true, open);
        if let Some(to) = to {
            fronts.note(to, |_| false);
        }
        to
    };
    assert_eq!(back(&mut fronts, 'd'), Some('c'));
    assert_eq!(back(&mut fronts, 'c'), Some('b'), "on, past it");
    assert_eq!(back(&mut fronts, 'b'), Some('a'));
    assert_eq!(back(&mut fronts, 'a'), None, "none before it");
    assert_eq!(fronts.recent(Some('a'), open), Some('b'), "back there last");

    // Not walking, it starts again from the one in front.
    assert_eq!(fronts.back(Some('a'), false, open), Some('b'));
}

#[test]
fn another_in_front_ends_a_walk_and_those_not_open_are_passed() {
    let open = |_| true;
    let mut fronts = history(&['a', 'b', 'c']);
    assert_eq!(fronts.back(Some('c'), true, open), Some('b'));
    fronts.note('b', |_| false);
    fronts.note('a', |_| false);
    fronts.note('b', |_| false);
    assert_eq!(
        fronts.back(Some('b'), true, open),
        Some('a'),
        "back where it went, it starts again"
    );

    let fronts = history(&['a', 'b', 'c']);
    assert_eq!(fronts.recent(Some('c'), |one| one != 'b'), Some('a'));
    let mut fronts = history(&['a', 'b', 'c']);
    fronts.note('d', |one| one == 'b');
    assert_eq!(fronts.back(Some('d'), true, |_| true), Some('c'));
    fronts.note('c', |_| false);
    assert_eq!(
        fronts.back(Some('c'), true, |_| true),
        Some('a'),
        "b forgotten"
    );
}
