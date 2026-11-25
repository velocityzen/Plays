use std::collections::BTreeMap;

use hegel::TestCase;
use hegel::generators as gs;

use super::tests::{Node, read_tree, write_tree};
use super::*;
use crate::support::Scratch;

/// Where an install's files may be. No folder is named as a file is.
const FILES: [&str; 7] = ["a", "b", "c", "d/a", "d/b", "d/e/a", "e/c"];
/// Where its links may be, and what each may link to, inside it.
const LINKS: [(&str, &[&str]); 2] = [("l", &["a", "d", "d/e/a"]), ("d/l", &["../a", "e", "../c"])];

/// A file's bytes: `old`'s as they are, changed or added to, or new.
fn contents(tc: &TestCase, old: Option<&[u8]>) -> Vec<u8> {
    let new = || tc.draw(gs::binary().max_size(400));
    let Some(old) = old else { return new() };
    match tc.draw(gs::integers::<u8>().max_value(3)) {
        0 => old.to_vec(),
        1 if !old.is_empty() => {
            let mut bytes = old.to_vec();
            let at = tc.draw(gs::integers::<usize>().max_value(old.len() - 1));
            bytes[at] ^= tc.draw(gs::integers::<u8>().min_value(1));
            bytes
        }
        2 => [old, &new()].concat(),
        _ => new(),
    }
}

/// An install's files, links and folders: changed from `old`'s, if
/// there's one.
fn install(tc: &TestCase, old: Option<&BTreeMap<String, Node>>) -> BTreeMap<String, Node> {
    let mut nodes = BTreeMap::new();
    for path in FILES {
        if !tc.draw(gs::booleans()) {
            continue;
        }
        let was = old.and_then(|old| match old.get(path) {
            Some(Node::File(bytes, _)) => Some(bytes.as_slice()),
            _ => None,
        });
        let mode = tc.draw(gs::sampled_from(&[0o644, 0o755, 0o600][..]));
        nodes.insert(path.to_owned(), Node::File(contents(tc, was), mode));
    }
    for (path, targets) in LINKS {
        if tc.draw(gs::weighted_booleans(0.25)) {
            nodes.insert(
                path.to_owned(),
                Node::Link(tc.draw(gs::sampled_from(targets)).to_owned()),
            );
        }
    }
    let folders: Vec<String> = nodes
        .keys()
        .flat_map(|path| path.match_indices('/').map(|(at, _)| path[..at].to_owned()))
        .collect();
    for folder in folders {
        if let std::collections::btree_map::Entry::Vacant(entry) = nodes.entry(folder) {
            entry.insert(Node::Dir(
                tc.draw(gs::sampled_from(&[0o755, 0o700, 0o555][..])),
            ));
        }
    }
    nodes
}

#[hegel::test]
fn a_whole_update_puts_any_install_together_as_it_was(tc: TestCase) {
    let new = install(&tc, None);
    let scratch = Scratch::new();
    write_tree(&scratch.join("new"), &new);
    make(
        &scratch.join("new"),
        None,
        "0.2.0",
        1,
        &scratch.join("whole.update"),
    )
    .unwrap();
    apply(
        &scratch.join("whole.update"),
        None,
        "0.2.0",
        &scratch.join("out"),
    )
    .unwrap();
    assert_eq!(read_tree(&scratch.join("out")), new);
}

#[hegel::test]
fn a_delta_puts_the_new_install_together_from_the_old(tc: TestCase) {
    let old = install(&tc, None);
    let new = install(&tc, Some(&old));
    let scratch = Scratch::new();
    write_tree(&scratch.join("old"), &old);
    write_tree(&scratch.join("new"), &new);
    let base = scratch.join("old");
    make(
        &scratch.join("new"),
        Some((&base, "0.1.0")),
        "0.2.0",
        1,
        &scratch.join("delta.update"),
    )
    .unwrap();
    apply(
        &scratch.join("delta.update"),
        Some(&base),
        "0.2.0",
        &scratch.join("out"),
    )
    .unwrap();
    assert_eq!(read_tree(&scratch.join("out")), new);
}

#[hegel::test]
fn a_delta_applied_to_any_install_puts_the_new_one_together_or_nothing(tc: TestCase) {
    let old = install(&tc, None);
    let new = install(&tc, Some(&old));
    let installed = install(&tc, Some(&old));
    let scratch = Scratch::new();
    write_tree(&scratch.join("old"), &old);
    write_tree(&scratch.join("new"), &new);
    write_tree(&scratch.join("installed"), &installed);
    let base = scratch.join("old");
    make(
        &scratch.join("new"),
        Some((&base, "0.1.0")),
        "0.2.0",
        1,
        &scratch.join("delta.update"),
    )
    .unwrap();
    let applied = apply(
        &scratch.join("delta.update"),
        Some(&scratch.join("installed")),
        "0.2.0",
        &scratch.join("out"),
    );
    match applied {
        Ok(()) => assert_eq!(read_tree(&scratch.join("out")), new),
        Err(err) => {
            assert!(matches!(err, Error::Update(_)), "{err}");
            assert!(fs::symlink_metadata(scratch.join("out")).is_err());
        }
    }
}
