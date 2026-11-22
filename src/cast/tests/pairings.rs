use airplay::PinPairing;
use airplay::fake::{FakeReceiver, Kind};

use super::*;

/// Credentials from pairing with a fake TV.
fn credentials() -> Credentials {
    let tv = FakeReceiver::start(Kind::Tv { pin: "1234" });
    PinPairing::start(&tv.device())
        .and_then(|pairing| pairing.finish("1234"))
        .expect("paired with the fake")
}

#[test]
fn pairings_are_saved_in_the_apps_folder_and_read_back() {
    let folder = Folder::for_test("airplay-pairings");
    let credentials = credentials();
    let pairings = Pairings::load(&folder);
    let sender = pairings.sender();
    assert_eq!(sender.name, "Plays");
    pairings.keep("TV2", "Bedroom", &credentials);
    pairings.keep("TV1", "living room", &credentials);

    let again = Pairings::load(&folder);
    assert_eq!(again.sender().id, sender.id, "known again by the same id");
    assert_eq!(again.credentials("TV1"), Some(credentials.clone()));
    assert_eq!(again.credentials("TV3"), None);
    assert_eq!(
        again.paired(),
        [
            ("TV2".to_owned(), "Bedroom".to_owned()),
            ("TV1".to_owned(), "living room".to_owned())
        ],
        "by name"
    );

    again.forget("TV2");
    let after = Pairings::load(&folder);
    assert_eq!(after.credentials("TV2"), None);
    assert_eq!(after.paired().len(), 1);
}

#[test]
fn a_broken_file_is_no_pairings_and_is_saved_over() {
    let folder = Folder::for_test("airplay-pairings-broken");
    let path = folder.file(FILE).unwrap();
    files::write_whole(&path, b"{ \"receivers\": ").unwrap();
    let pairings = Pairings::load(&folder);
    assert!(pairings.paired().is_empty());
    pairings.keep("TV1", "Den", &credentials());
    assert_eq!(Pairings::load(&folder).paired().len(), 1);
}

#[test]
fn without_a_folder_they_are_kept_while_the_app_runs() {
    let pairings = Pairings::load(&Folder::default());
    let id = pairings.sender().id;
    assert_eq!(pairings.sender().id, id);
    pairings.keep("TV1", "Den", &credentials());
    assert!(pairings.credentials("TV1").is_some());
}
