use super::*;

/// A cast of the clip to a device that takes an MPEG-TS stream.
fn streamed(server: &Server) -> Cast {
    let clip = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/clip.mp4");
    let profile = Profile {
        files: Vec::new(),
        hls: None,
        stream: true,
        ..Profile::default()
    };
    let here = SocketAddr::from((Ipv4Addr::LOCALHOST, 0));
    server
        .add(CastRequest::new(Source::File(clip), profile), here)
        .unwrap()
}

/// Where a device reaches this computer: over its IP version, and for a
/// link-local address, through its zone.
#[test]
fn the_way_back_from_a_device() {
    let any_v6 = SocketAddr::from((Ipv6Addr::UNSPECIFIED, 0));
    let any_v4 = SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0));
    let at = |text: &str| text.parse::<SocketAddr>().unwrap();
    assert_eq!(
        local_address(at("[::1]:8009"), any_v6).unwrap(),
        IpAddr::from(Ipv6Addr::LOCALHOST)
    );
    assert_eq!(
        local_address(at("127.0.0.1:8009"), any_v6).unwrap(),
        IpAddr::from(Ipv4Addr::LOCALHOST)
    );
    assert!(matches!(
        local_address(at("[::1]:8009"), any_v4),
        Err(Error::Unsupported(_))
    ));
    #[cfg(target_os = "macos")]
    {
        // macOS's link-local address on its loopback, `fe80::1%lo0`, on
        // no port in particular.
        let link_local = at("[fe80::1%1]:0");
        assert_eq!(
            local_address(link_local, any_v6).unwrap(),
            "fe80::1".parse::<IpAddr>().unwrap()
        );
        assert!(
            local_address(at("[fe80::1]:8009"), any_v6).is_err(),
            "no way there without its zone"
        );
    }
}

#[test]
fn a_stream_past_the_most_stops_the_oldest_still_made() {
    let server = Server::start_with((Ipv4Addr::LOCALHOST, 0).into(), 1.0).unwrap();
    let cast = streamed(&server);
    let state = &cast.state;
    let stopped = |stop: &Arc<AtomicBool>| stop.load(Ordering::Relaxed);
    let made: Vec<_> = (0..MOST_STREAMS).map(|_| state.new_stream()).collect();
    assert!(!made.iter().any(stopped));

    let newer = state.new_stream();
    assert!(stopped(&made[0]), "the oldest stopped");
    assert!(!made[1..].iter().any(stopped));
    assert!(!stopped(&newer));

    // One that's ended leaves room: the oldest still made goes on.
    drop(newer);
    let newest = state.new_stream();
    assert!(!made[1..].iter().any(stopped) && !stopped(&newest));

    // Closing the cast stops them all, and any asked for after.
    state.close();
    assert!(made[1..].iter().all(stopped) && stopped(&newest));
    assert!(stopped(&state.new_stream()));
}

#[test]
fn reads_time_seeks() {
    assert_eq!(npt_start("npt=12.5-"), Some(12.5));
    assert_eq!(npt_start("npt=0:01:02.5-30"), Some(62.5));
    assert_eq!(npt_start("npt=-"), Some(0.0));
    assert_eq!(npt_start("bytes=0-"), None);
    assert_eq!(npt_start("npt=abc-"), None);
}

#[test]
fn names_segments() {
    assert_eq!(segment_number("12.ts", Segments::Ts), Some(12));
    assert_eq!(segment_number("3.m4s", Segments::Fmp4), Some(3));
    assert_eq!(segment_number("3.ts", Segments::Fmp4), None);
    assert_eq!(segment_number("x.ts", Segments::Ts), None);
}
