use super::*;

/// Devices that are a name, made from one record of the first type, or
/// one of the second if there's none of the first.
struct Named;

impl Protocol for Named {
    type Device = String;
    const NAME: &'static str = "named";
    const SERVICES: &'static [&'static str] = &["_first._tcp.local.", "_second._tcp.local."];

    fn id(_: usize, record: &Record) -> Option<String> {
        record.txt("id").map(str::to_owned)
    }

    fn device(records: &[Option<&Record>]) -> Option<String> {
        let record = records.iter().flatten().next()?;
        Some(record.instance.clone())
    }
}

const FIRST: usize = 0;
const SECOND: usize = 1;

fn record(id: &str, name: &str) -> Record {
    Record {
        instance: name.into(),
        addresses: vec!["fe80::1".parse().unwrap(), "192.168.1.20".parse().unwrap()],
        port: 7000,
        txt: [("id".to_owned(), id.to_owned())].into(),
    }
}

#[test]
fn addresses_go_ipv4_first_once_each() {
    let mut record = record("a", "A");
    record.addresses.push("192.168.1.10".parse().unwrap());
    record.addresses.push("192.168.1.20".parse().unwrap());
    let addresses: Vec<SocketAddr> = ["192.168.1.10:7000", "192.168.1.20:7000", "[fe80::1]:7000"]
        .iter()
        .map(|address| address.parse().unwrap())
        .collect();
    assert_eq!(record.socket_addresses(), addresses);
    assert_eq!(record.txt("id"), Some("a"));
    assert_eq!(record.txt("nothing"), None);
}

#[test]
fn the_newest_record_of_a_type_wins() {
    let mut directory = Directory::<Named>::default();
    assert_eq!(
        directory.resolved(FIRST, "a.1".into(), record("a", "Old")),
        Some(Found::Added("Old".into()))
    );
    assert_eq!(
        directory.resolved(FIRST, "a.2".into(), record("a", "New")),
        Some(Found::Changed("New".into()))
    );
    assert_eq!(
        directory.resolved(SECOND, "a.3".into(), record("a", "Second")),
        None,
        "the first type's still counts first"
    );
    assert_eq!(
        directory.removed(FIRST, "a.2"),
        Some(Found::Changed("Old".into()))
    );
    assert_eq!(
        directory.removed(FIRST, "a.1"),
        Some(Found::Changed("Second".into()))
    );
    assert_eq!(
        directory.removed(SECOND, "a.3"),
        Some(Found::Removed("Second".into()))
    );
    assert!(directory.devices().is_empty());
}

#[test]
fn a_record_that_names_another_device_leaves_the_one_before() {
    let mut directory = Directory::<Named>::default();
    directory.resolved(FIRST, "shared".into(), record("a", "A"));
    assert_eq!(
        directory.resolved(FIRST, "shared".into(), record("b", "B")),
        Some(Found::Added("B".into()))
    );
    assert_eq!(directory.devices(), ["B"], "a has no record left");
    assert_eq!(directory.removed(SECOND, "shared"), None, "of another type");
    assert_eq!(
        directory.resolved(
            FIRST,
            "unnamed".into(),
            Record {
                txt: HashMap::new(),
                ..record("", "Nobody")
            }
        ),
        None,
        "a record without an id is no device"
    );
}
