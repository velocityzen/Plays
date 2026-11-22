use std::collections::HashSet;

use super::*;

fn named(ids: &[u32], labels: &[Option<&'static str>]) -> Vec<Named<'static>> {
    ids.iter()
        .zip(labels)
        .map(|(&id, &label)| Named { id: Id(id), label })
        .collect()
}

#[test]
fn ids_are_never_given_twice_and_end_differently() {
    let ids: Vec<Id> = (0..10_000).map(|_| Id::next()).collect();
    assert_eq!(ids.iter().collect::<HashSet<_>>().len(), ids.len());
    // Counted one after another, yet a few last digits tell them apart.
    let ends: HashSet<String> = (1..=16)
        .map(|n| Id(spread(n)).to_string()[5..].to_owned())
        .collect();
    assert_eq!(ends.len(), 16);
    assert_eq!(Id(0x3fa9_c2d1).to_string(), "3fa9c2d1");
    assert_eq!(Id(7).to_string(), "00000007");
}

#[test]
fn names_by_id_then_label_then_position_then_the_end_of_the_id() {
    let among = named(
        &[0x1111_aaaa, 0x2222_bbbb, 0x3333_cccc, 0x4444_dddd],
        &[Some("left"), None, Some("2"), Some("2222bbbb")],
    );
    let positions = [0, 1, 2, 3];
    let pick = |name| pick("tile", name, &among, &positions);
    assert_eq!(pick("2222bbbb"), Ok(1), "by id, before a label");
    assert_eq!(pick("left"), Ok(0), "by label");
    assert_eq!(pick("2"), Ok(2), "a label before a position");
    assert_eq!(pick("1"), Ok(0), "by position, from 1");
    assert_eq!(pick("3"), Ok(2));
    assert_eq!(pick("cccc"), Ok(2), "by the end of its id");
    assert_eq!(pick("BBB"), Ok(1), "in either case");
    assert_eq!(pick(" left "), Ok(0), "spaces aside");
    assert_eq!(pick("5"), Err(Error::not_found("tile", "5")));
    assert_eq!(pick("eeee"), Err(Error::not_found("tile", "eeee")));
    assert_eq!(pick(""), Err(Error::not_found("tile", "with no name")));

    // By position alone, as ⌘1 to ⌘9 name windows.
    assert_eq!(pick("#2"), Ok(1), "not the one labeled 2");
    assert_eq!(pick(" #4 "), Ok(3));
    assert_eq!(
        pick("#5"),
        Err(Error::not_found("tile", "#5")),
        "none fifth"
    );
    assert_eq!(
        pick("#dddd"),
        Err(Error::not_found("tile", "#dddd")),
        "not by the end of an id"
    );
    assert_eq!(pick("#0"), Err(Error::not_found("tile", "#0")));
    assert_eq!(pick("#"), Err(Error::not_found("tile", "#")));
    assert_eq!(
        super::pick("tile", "#1", &among, &[2, 3]),
        Ok(2),
        "counting only those given"
    );
}

#[test]
fn a_name_that_could_be_several_lists_them() {
    let among = named(
        &[0x1111_00a1, 0x2222_00a1, 0x3333_00b1],
        &[Some("news"), Some("news"), None],
    );
    let positions = [0, 1, 2];
    assert_eq!(
        pick("tile", "a1", &among, &positions),
        Err(Error::Ambiguous {
            what: "tile".into(),
            name: "a1".into(),
            matches: vec!["111100a1".into(), "222200a1".into()],
        })
    );
    assert_eq!(
        pick("window", "news", &among, &positions),
        Err(Error::Ambiguous {
            what: "window".into(),
            name: "news".into(),
            matches: vec!["111100a1".into(), "222200a1".into()],
        })
    );
    assert_eq!(pick("tile", "b1", &among, &positions), Ok(2));
}

#[test]
fn positions_count_only_those_given() {
    // Every tile, of which the window in front has the last two.
    let among = named(&[0x10, 0x20, 0x31], &[None, None, None]);
    assert_eq!(pick("tile", "1", &among, &[1, 2]), Ok(1));
    assert_eq!(pick("tile", "2", &among, &[1, 2]), Ok(2));
    assert_eq!(pick("tile", "00000010", &among, &[1, 2]), Ok(0), "by id");
    // Not a 31st there: the end of an id instead.
    assert_eq!(pick("tile", "31", &among, &[1, 2]), Ok(2));
    assert_eq!(
        pick("tile", "0", &among, &[1, 2]),
        Err(Error::Ambiguous {
            what: "tile".into(),
            name: "0".into(),
            matches: vec!["00000010".into(), "00000020".into()],
        }),
        "not a position, and two ids end so"
    );
}
