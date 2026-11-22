use network::mdns::Directory;

use super::*;
use crate::device::tests::{google_tv, speaker};

/// `_googlecast._tcp`, the one service browsed.
const SERVICE: usize = 0;

#[test]
fn devices_come_change_and_go() {
    let mut directory = Directory::<Cast>::default();
    let Some(Found::Added(tv)) = directory.resolved(SERVICE, "tv".into(), google_tv()) else {
        panic!("a new device")
    };
    assert_eq!(
        directory.resolved(SERVICE, "tv".into(), google_tv()),
        None,
        "the same again changes nothing"
    );
    let mut idle = google_tv();
    idle.txt.insert("rs".into(), String::new());
    let Some(Found::Changed(changed)) = directory.resolved(SERVICE, "tv".into(), idle) else {
        panic!("what it shows changed")
    };
    assert_eq!(changed.id, tv.id);
    assert_eq!(changed.showing, None);

    directory.resolved(SERVICE, "kitchen".into(), speaker());
    assert_eq!(directory.devices().len(), 2);
    let Some(Found::Removed(gone)) = directory.removed(SERVICE, "tv") else {
        panic!("gone with its record")
    };
    assert_eq!(gone.id, tv.id);
    assert_eq!(directory.removed(SERVICE, "tv"), None);
    assert_eq!(directory.devices().len(), 1);
}

#[test]
fn a_renamed_device_changes_and_keeps_its_id() {
    let mut directory = Directory::<Cast>::default();
    directory.resolved(SERVICE, "old name".into(), speaker());
    let mut renamed = speaker();
    renamed.txt.insert("fn".into(), "Pantry".into());
    let Some(Found::Changed(device)) = directory.resolved(SERVICE, "new name".into(), renamed)
    else {
        panic!("a new name for the same device")
    };
    assert_eq!(device.name, "Pantry");
    assert_eq!(
        directory.removed(SERVICE, "old name"),
        None,
        "the old name going changes nothing"
    );
    assert_eq!(directory.devices().len(), 1);
}
