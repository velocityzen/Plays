use network::mdns::Directory;

use super::*;
use crate::device::tests::{apple_tv, speaker};

#[test]
fn a_receivers_two_records_make_one_device() {
    let mut directory = Directory::<AirPlay>::default();
    let (airplay, raop) = apple_tv();
    let Some(Found::Added(first)) = directory.resolved(RAOP, "raop.tv".into(), raop.clone()) else {
        panic!("the RAOP record alone is a receiver")
    };
    assert_eq!(first.name, "Living Room");
    assert!(first.codecs.contains(&0));
    let Some(Found::Changed(both)) =
        directory.resolved(AIRPLAY, "airplay.tv".into(), airplay.clone())
    else {
        panic!("the AirPlay record adds to it")
    };
    assert_eq!(both.id, first.id);
    assert_eq!(both.public_key, Some([0x11; 32]));
    assert_eq!(directory.devices().len(), 1);
    assert_eq!(
        directory.resolved(AIRPLAY, "airplay.tv".into(), airplay),
        None,
        "the same again changes nothing"
    );
}

#[test]
fn a_receiver_goes_once_both_its_records_have() {
    let mut directory = Directory::<AirPlay>::default();
    let (airplay, raop) = speaker();
    directory.resolved(AIRPLAY, "airplay.kitchen".into(), airplay);
    directory.resolved(RAOP, "raop.kitchen".into(), raop);
    let (tv, _) = apple_tv();
    directory.resolved(AIRPLAY, "airplay.tv".into(), tv);
    assert_eq!(directory.devices().len(), 2);

    assert!(matches!(
        directory.removed(AIRPLAY, "airplay.kitchen"),
        Some(Found::Changed(_))
    ));
    let Some(Found::Removed(gone)) = directory.removed(RAOP, "raop.kitchen") else {
        panic!("gone with its last record")
    };
    assert_eq!(gone.name, "Kitchen");
    assert_eq!(directory.removed(RAOP, "raop.kitchen"), None);
    assert_eq!(directory.devices().len(), 1);
}

#[test]
fn this_computers_own_receiver_is_left_out() {
    let here = this_computer();
    assert!(!here.is_empty(), "this computer is on a network");
    assert!(here.iter().all(|ip| !ip.is_loopback()), "{here:?}");
    let mut directory = Directory::<AirPlay>::default();
    let (mut airplay, mut raop) = apple_tv();
    for record in [&mut airplay, &mut raop] {
        record.addresses.push(here[0]);
    }
    assert_eq!(
        directory.resolved(AIRPLAY, "airplay.mac".into(), airplay),
        None
    );
    assert_eq!(directory.resolved(RAOP, "raop.mac".into(), raop), None);
    assert!(directory.devices().is_empty());
    // Another receiver is found as ever.
    let (airplay, _) = speaker();
    directory.resolved(AIRPLAY, "airplay.kitchen".into(), airplay);
    assert_eq!(directory.devices().len(), 1);
}

#[test]
fn a_renamed_receiver_changes_and_keeps_its_id() {
    let mut directory = Directory::<AirPlay>::default();
    let (mut airplay, _) = speaker();
    directory.resolved(AIRPLAY, "airplay.kitchen".into(), airplay.clone());
    airplay.instance = "Dining Room".into();
    let Some(Found::Changed(renamed)) =
        directory.resolved(AIRPLAY, "airplay.dining".into(), airplay)
    else {
        panic!("a new name for the same receiver")
    };
    assert_eq!(renamed.id, "06AABBCCDDEE");
    assert_eq!(renamed.name, "Dining Room");
    assert_eq!(
        directory.removed(AIRPLAY, "airplay.kitchen"),
        None,
        "the old name going changes nothing"
    );
    assert_eq!(directory.devices().len(), 1);
}
