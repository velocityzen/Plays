use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;

use super::*;
use crate::support::{Scratch, binary};

/// What's at a path in an install.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Node {
    Dir(u32),
    File(Vec<u8>, u32),
    Link(String),
}

/// `nodes` written into `root`, a folder, each folder before what's in it.
pub(crate) fn write_tree(root: &Path, nodes: &BTreeMap<String, Node>) {
    fs::create_dir_all(root).unwrap();
    for (path, node) in nodes {
        let on_disk = root.join(path);
        fs::create_dir_all(on_disk.parent().unwrap()).unwrap();
        match node {
            Node::Dir(_) => fs::create_dir_all(&on_disk).unwrap(),
            Node::File(bytes, mode) => {
                fs::write(&on_disk, bytes).unwrap();
                fs::set_permissions(&on_disk, fs::Permissions::from_mode(*mode)).unwrap();
            }
            Node::Link(target) => std::os::unix::fs::symlink(target, &on_disk).unwrap(),
        }
    }
    for (path, node) in nodes {
        if let Node::Dir(mode) = node {
            fs::set_permissions(root.join(path), fs::Permissions::from_mode(*mode)).unwrap();
        }
    }
}

/// What's in the folder at `root`.
pub(crate) fn read_tree(root: &Path) -> BTreeMap<String, Node> {
    let mut nodes = BTreeMap::new();
    let mut folders = vec![(root.to_owned(), String::new())];
    while let Some((folder, prefix)) = folders.pop() {
        for entry in fs::read_dir(&folder).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name().into_string().unwrap();
            let path = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            let metadata = fs::symlink_metadata(entry.path()).unwrap();
            let mode = metadata.permissions().mode() & 0o7777;
            let node = if metadata.is_symlink() {
                Node::Link(
                    fs::read_link(entry.path())
                        .unwrap()
                        .to_str()
                        .unwrap()
                        .to_owned(),
                )
            } else if metadata.is_dir() {
                folders.push((entry.path(), path.clone()));
                Node::Dir(mode)
            } else {
                Node::File(fs::read(entry.path()).unwrap(), mode)
            };
            nodes.insert(path, node);
        }
    }
    nodes
}

pub(crate) fn file(bytes: impl Into<Vec<u8>>) -> Node {
    Node::File(bytes.into(), 0o644)
}

/// An app bundle's worth of nodes.
fn app(executable: Vec<u8>) -> BTreeMap<String, Node> {
    BTreeMap::from([
        ("Contents".into(), Node::Dir(0o755)),
        ("Contents/Info.plist".into(), file("<plist>0.1.0</plist>")),
        ("Contents/MacOS".into(), Node::Dir(0o755)),
        ("Contents/MacOS/plays".into(), Node::File(executable, 0o755)),
        ("Contents/Resources".into(), Node::Dir(0o755)),
        (
            "Contents/Resources/AppIcon.icns".into(),
            file(binary(9, 4_000)),
        ),
        (
            "Contents/Resources/Current".into(),
            Node::Link("../MacOS".into()),
        ),
    ])
}

#[test]
fn a_whole_update_puts_a_folder_together_as_it_was() {
    let scratch = Scratch::new();
    let nodes = app(binary(1, 50_000));
    write_tree(&scratch.join("new"), &nodes);
    let made = make(
        &scratch.join("new"),
        None,
        "0.2.0",
        3,
        &scratch.join("whole.update"),
    )
    .unwrap();
    assert_eq!(
        made,
        Made {
            kept: 0,
            patched: 0,
            whole: 3
        }
    );
    assert_eq!(
        versions(&scratch.join("whole.update")).unwrap(),
        Versions {
            from: None,
            to: "0.2.0".into()
        }
    );
    apply(
        &scratch.join("whole.update"),
        None,
        "0.2.0",
        &scratch.join("out"),
    )
    .unwrap();
    assert_eq!(read_tree(&scratch.join("out")), nodes);
}

#[test]
fn a_whole_update_puts_a_single_file_together() {
    let scratch = Scratch::new();
    fs::write(scratch.join("plays"), binary(2, 10_000)).unwrap();
    fs::set_permissions(scratch.join("plays"), fs::Permissions::from_mode(0o755)).unwrap();
    make(
        &scratch.join("plays"),
        None,
        "0.2.0",
        3,
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
    assert_eq!(fs::read(scratch.join("out")).unwrap(), binary(2, 10_000));
    let mode = fs::metadata(scratch.join("out"))
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o755);
}

#[test]
fn a_delta_keeps_patches_and_adds_files_and_leaves_out_those_gone() {
    let scratch = Scratch::new();
    let old = app(binary(1, 200_000));
    let mut executable = binary(1, 200_000);
    executable[1_000..1_064].fill(0xAA);
    executable.extend(binary(5, 3_000));
    let mut new = app(executable);
    new.insert("Contents/Info.plist".into(), file("<plist>0.2.0</plist>"));
    new.insert("Contents/Resources/New.txt".into(), file("new"));
    new.remove("Contents/Resources/Current");
    write_tree(&scratch.join("old"), &old);
    write_tree(&scratch.join("new"), &new);

    let base = Some((scratch.join("old"), "0.1.0"));
    let base = base.as_ref().map(|(path, from)| (path.as_path(), *from));
    let made = make(
        &scratch.join("new"),
        base,
        "0.2.0",
        3,
        &scratch.join("delta.update"),
    )
    .unwrap();
    // Info.plist, patched or whole as either's smaller.
    assert_eq!((made.kept, made.patched + made.whole), (1, 3));
    assert!(made.patched >= 1);
    make(
        &scratch.join("new"),
        None,
        "0.2.0",
        3,
        &scratch.join("whole.update"),
    )
    .unwrap();
    let size = |name: &str| fs::metadata(scratch.join(name)).unwrap().len();
    assert!(
        size("delta.update") * 10 < size("whole.update"),
        "{} vs {}",
        size("delta.update"),
        size("whole.update")
    );

    let versions = versions(&scratch.join("delta.update")).unwrap();
    assert_eq!(versions.from.as_deref(), Some("0.1.0"));
    apply(
        &scratch.join("delta.update"),
        Some(&scratch.join("old")),
        "0.2.0",
        &scratch.join("out"),
    )
    .unwrap();
    assert_eq!(read_tree(&scratch.join("out")), new);
}

#[test]
fn a_delta_patches_a_single_file() {
    let scratch = Scratch::new();
    let old = binary(3, 100_000);
    let mut new = old.clone();
    new[50_000..50_010].fill(0);
    fs::write(scratch.join("old"), &old).unwrap();
    fs::write(scratch.join("new"), &new).unwrap();
    let made = make(
        &scratch.join("new"),
        Some((&scratch.join("old"), "0.1.0")),
        "0.2.0",
        3,
        &scratch.join("delta.update"),
    )
    .unwrap();
    assert_eq!(
        made,
        Made {
            kept: 0,
            patched: 1,
            whole: 0
        }
    );
    apply(
        &scratch.join("delta.update"),
        Some(&scratch.join("old")),
        "0.2.0",
        &scratch.join("out"),
    )
    .unwrap();
    assert_eq!(fs::read(scratch.join("out")).unwrap(), new);
}

/// A delta from `old` to `new`, both single files, at `delta.update`.
fn single_file_delta(scratch: &Scratch, old: &[u8], new: &[u8]) {
    fs::write(scratch.join("old"), old).unwrap();
    fs::write(scratch.join("new"), new).unwrap();
    make(
        &scratch.join("new"),
        Some((&scratch.join("old"), "0.1.0")),
        "0.2.0",
        3,
        &scratch.join("delta.update"),
    )
    .unwrap();
}

#[test]
fn a_delta_doesnt_apply_to_an_install_changed_since() {
    let scratch = Scratch::new();
    let old = binary(3, 100_000);
    let mut new = old.clone();
    new[10] ^= 1;
    single_file_delta(&scratch, &old, &new);
    let mut changed = old.clone();
    changed[99_000] ^= 1;
    fs::write(scratch.join("old"), &changed).unwrap();
    let applied = apply(
        &scratch.join("delta.update"),
        Some(&scratch.join("old")),
        "0.2.0",
        &scratch.join("out"),
    );
    assert!(
        matches!(applied, Err(Error::Update(why)) if why == "the installed file isn't the one it's from")
    );
    assert!(fs::symlink_metadata(scratch.join("out")).is_err());
}

#[test]
fn a_delta_keeping_a_file_doesnt_apply_when_that_file_changed() {
    let scratch = Scratch::new();
    let old = BTreeMap::from([
        ("a".into(), file("same")),
        ("b".into(), file(binary(1, 1_000))),
    ]);
    let mut new = old.clone();
    new.insert("b".into(), file(binary(2, 1_000)));
    write_tree(&scratch.join("old"), &old);
    write_tree(&scratch.join("new"), &new);
    make(
        &scratch.join("new"),
        Some((&scratch.join("old"), "0.1.0")),
        "0.2.0",
        3,
        &scratch.join("delta.update"),
    )
    .unwrap();
    fs::write(scratch.join("old/a"), "SAME").unwrap();
    let applied = apply(
        &scratch.join("delta.update"),
        Some(&scratch.join("old")),
        "0.2.0",
        &scratch.join("out"),
    );
    assert!(
        matches!(applied, Err(Error::Update(why)) if why == "the installed a isn't the one it's from")
    );
    assert!(fs::symlink_metadata(scratch.join("out")).is_err());
}

#[test]
fn a_delta_doesnt_apply_with_no_install() {
    let scratch = Scratch::new();
    let old = binary(1, 5_000);
    let mut new = old.clone();
    new[0] ^= 1;
    single_file_delta(&scratch, &old, &new);
    let applied = apply(
        &scratch.join("delta.update"),
        None,
        "0.2.0",
        &scratch.join("out"),
    );
    assert!(matches!(applied, Err(Error::Update(_))));
    fs::remove_file(scratch.join("old")).unwrap();
    let applied = apply(
        &scratch.join("delta.update"),
        Some(&scratch.join("old")),
        "0.2.0",
        &scratch.join("out"),
    );
    assert!(
        matches!(applied, Err(Error::Update(why)) if why.starts_with("the installed file can't be read"))
    );
}

#[test]
fn an_update_for_another_version_doesnt_apply() {
    let scratch = Scratch::new();
    fs::write(scratch.join("new"), "plays").unwrap();
    make(
        &scratch.join("new"),
        None,
        "0.2.0",
        3,
        &scratch.join("whole.update"),
    )
    .unwrap();
    let applied = apply(
        &scratch.join("whole.update"),
        None,
        "0.3.0",
        &scratch.join("out"),
    );
    assert!(matches!(applied, Err(Error::Update(why)) if why == "it's 0.2.0's, not 0.3.0's"));
}

#[test]
fn what_isnt_an_update_or_is_cut_short_doesnt_apply() {
    let scratch = Scratch::new();
    fs::write(scratch.join("not.update"), "UPDATER0 and the rest").unwrap();
    let applied = apply(
        &scratch.join("not.update"),
        None,
        "0.2.0",
        &scratch.join("out"),
    );
    assert!(matches!(applied, Err(Error::Update(why)) if why == "it isn't an update file"));

    fs::write(scratch.join("new"), binary(4, 20_000)).unwrap();
    make(
        &scratch.join("new"),
        None,
        "0.2.0",
        3,
        &scratch.join("whole.update"),
    )
    .unwrap();
    let bytes = fs::read(scratch.join("whole.update")).unwrap();
    fs::write(scratch.join("cut.update"), &bytes[..bytes.len() - 10]).unwrap();
    let applied = apply(
        &scratch.join("cut.update"),
        None,
        "0.2.0",
        &scratch.join("out"),
    );
    assert!(matches!(applied, Err(Error::Update(why)) if why == "it's cut short"));
    assert!(fs::symlink_metadata(scratch.join("out")).is_err());
}

#[test]
fn an_update_isnt_put_where_something_is() {
    let scratch = Scratch::new();
    fs::write(scratch.join("new"), "plays").unwrap();
    make(
        &scratch.join("new"),
        None,
        "0.2.0",
        3,
        &scratch.join("whole.update"),
    )
    .unwrap();
    fs::write(scratch.join("out"), "there").unwrap();
    let applied = apply(
        &scratch.join("whole.update"),
        None,
        "0.2.0",
        &scratch.join("out"),
    );
    assert!(matches!(applied, Err(Error::Update(_))));
    assert_eq!(fs::read(scratch.join("out")).unwrap(), b"there");
}

/// An update file of `entries` alone: folders and links, no blobs.
fn update_of(scratch: &Scratch, entries: serde_json::Value) -> PathBuf {
    let header =
        serde_json::json!({"to": "0.2.0", "root": {"dir": {"mode": 493, "entries": entries}}});
    let header = serde_json::to_vec(&header).unwrap();
    let mut bytes = MAGIC.to_vec();
    bytes.extend((header.len() as u64).to_le_bytes());
    bytes.extend(header);
    let path = scratch.join("crafted.update");
    fs::write(&path, bytes).unwrap();
    path
}

#[test]
fn nothing_is_written_outside_the_install() {
    let scratch = Scratch::new();
    let refused = [
        serde_json::json!([{"path": "../out", "kind": "dir", "mode": 493}]),
        serde_json::json!([{"path": "/etc", "kind": "dir", "mode": 493}]),
        serde_json::json!([{"path": "a/b", "kind": "dir", "mode": 493}]),
        serde_json::json!([{"path": "a", "kind": "dir", "mode": 493}, {"path": "a", "kind": "dir", "mode": 493}]),
        serde_json::json!([{"path": "a", "kind": "symlink", "target": "../.."}]),
        serde_json::json!([{"path": "a", "kind": "symlink", "target": "/tmp"}]),
        serde_json::json!([{"path": "a", "kind": "symlink", "target": "."}, {"path": "a/b", "kind": "dir", "mode": 493}]),
    ];
    for entries in refused {
        let update = update_of(&scratch, entries.clone());
        let applied = apply(&update, None, "0.2.0", &scratch.join("out"));
        assert!(
            matches!(applied, Err(Error::Update(_))),
            "{entries}: {applied:?}"
        );
        assert!(
            fs::symlink_metadata(scratch.join("out")).is_err(),
            "{entries}"
        );
    }
    assert_eq!(fs::read_dir(&scratch.0).unwrap().count(), 1);
}

#[test]
fn set_user_and_set_group_bits_are_never_set() {
    let scratch = Scratch::new();
    let update = update_of(
        &scratch,
        serde_json::json!([{"path": "a", "kind": "dir", "mode": 0o6777}]),
    );
    apply(&update, None, "0.2.0", &scratch.join("out")).unwrap();
    let mode = fs::metadata(scratch.join("out/a"))
        .unwrap()
        .permissions()
        .mode()
        & 0o7777;
    assert_eq!(mode, 0o777);
}

#[test]
fn a_read_only_folder_is_filled_then_made_read_only() {
    let scratch = Scratch::new();
    let nodes = BTreeMap::from([("a".into(), Node::Dir(0o555)), ("a/b".into(), file("b"))]);
    write_tree(&scratch.join("new"), &nodes);
    make(
        &scratch.join("new"),
        None,
        "0.2.0",
        3,
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
    assert_eq!(read_tree(&scratch.join("out")), nodes);
    // And goes, read-only as it is.
    remove(&scratch.join("out"));
    remove(&scratch.join("new"));
    assert!(fs::symlink_metadata(scratch.join("out")).is_err());
}

#[test]
fn paths_an_install_can_have() {
    for path in ["a", "Contents/MacOS/plays", "a b/c.d", "Ünïcode"] {
        assert!(valid_path(path), "{path}");
    }
    for path in [
        "", "/a", "a/", "a//b", ".", "a/./b", "..", "a/../b", "a\\b", "C:", "a\0",
    ] {
        assert!(!valid_path(path), "{path:?}");
    }
}

#[test]
fn links_that_stay_inside() {
    assert!(link_stays_inside("a", "b"));
    assert!(link_stays_inside("a/b", "../c"));
    assert!(link_stays_inside("a/b/c", "../../d/./e"));
    assert!(!link_stays_inside("a", "../b"));
    assert!(!link_stays_inside("a/b", "../../c"));
    assert!(!link_stays_inside("a", "/etc"));
    assert!(!link_stays_inside("a", ""));
    assert!(!link_stays_inside("a", "c:\\x"));
}
