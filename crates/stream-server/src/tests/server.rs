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
    let here = IpAddr::V4(Ipv4Addr::LOCALHOST);
    server
        .add(CastRequest::new(Source::File(clip), profile), here)
        .unwrap()
}

#[test]
fn a_stream_past_the_most_stops_the_oldest_still_made() {
    let here = IpAddr::V4(Ipv4Addr::LOCALHOST);
    let server = Server::start_with(SocketAddr::new(here, 0), 1.0).unwrap();
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
