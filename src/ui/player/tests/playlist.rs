use std::path::PathBuf;

use super::*;

fn file(name: &str) -> Source {
    Source::File(PathBuf::from("/media").join(name))
}

fn of(names: &[&str]) -> Option<Playlist> {
    Playlist::new(names.iter().map(|name| file(name)).collect())
}

#[test]
fn takes_two_files_or_more() {
    assert!(of(&[]).is_none());
    assert!(of(&["a.mkv"]).is_none());
    let list = of(&["a.mkv", "b.mkv"]).unwrap();
    assert_eq!(list.counter(), "1 of 2");
    assert_eq!(list.beside(1), Some(&file("b.mkv")), "from the first");
}

#[test]
fn steps_through_its_files_in_order_and_no_further() {
    let mut list = of(&["a.mkv", "b.mkv", "c.mkv"]).unwrap();
    assert_eq!(list.beside(-1), None, "nothing before the first");
    assert_eq!(list.step(-1), None);
    assert_eq!(list.counter(), "1 of 3");

    assert_eq!(list.beside(1), Some(&file("b.mkv")));
    assert_eq!(list.counter(), "1 of 3", "looking doesn't move");
    assert_eq!(list.step(1), Some(file("b.mkv")));
    assert_eq!(list.counter(), "2 of 3");
    assert_eq!(list.step(1), Some(file("c.mkv")));
    assert_eq!(list.beside(1), None, "nothing after the last");
    assert_eq!(list.step(1), None);
    assert_eq!(list.counter(), "3 of 3");

    assert_eq!(list.beside(-1), Some(&file("b.mkv")));
    assert_eq!(list.step(-1), Some(file("b.mkv")));
    assert_eq!(list.counter(), "2 of 3");
}

#[test]
fn a_file_in_it_twice_is_stepped_to_where_it_is() {
    let mut list = of(&["a.mkv", "b.mkv", "a.mkv"]).unwrap();
    list.step(1);
    assert_eq!(list.step(1), Some(file("a.mkv")));
    assert_eq!(list.counter(), "3 of 3");
}

#[test]
fn goes_to_a_file_in_it_and_no_other() {
    let mut list = of(&["a.mkv", "b.mkv", "c.mkv"]).unwrap();
    assert!(list.go_to(&file("c.mkv")));
    assert_eq!(list.counter(), "3 of 3");
    assert!(list.go_to(&file("b.mkv")));
    assert_eq!(list.counter(), "2 of 3");
    assert!(!list.go_to(&file("d.mkv")));
    assert_eq!(list.counter(), "2 of 3", "left where it was");
}
