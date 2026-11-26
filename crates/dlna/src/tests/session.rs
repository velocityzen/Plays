use super::*;

#[test]
fn how_long_a_subscription_lasts() {
    assert_eq!(
        subscribed_for(Some("Second-1800")),
        Duration::from_secs(1800)
    );
    assert_eq!(subscribed_for(Some("second-60 ")), Duration::from_secs(60));
    assert_eq!(subscribed_for(Some("infinite")), Duration::from_secs(3600));
    assert_eq!(subscribed_for(Some("Second-x")), SUBSCRIBED);
    assert_eq!(subscribed_for(None), SUBSCRIBED);
    assert_eq!(
        subscribed_for(Some("Second-18446744073709551615")),
        LONGEST_SUBSCRIPTION,
        "an hour at most"
    );
    assert_eq!(
        subscribed_for(Some("Second-0")),
        SHORTEST_SUBSCRIPTION,
        "not renewed on every poll"
    );
}

#[test]
fn a_callback_says_no_zone() {
    assert_eq!(
        callback("[fe80::1c:2%7]:4004".parse().unwrap(), "t"),
        "http://[fe80::1c:2]:4004/t"
    );
    assert_eq!(
        callback("192.0.2.1:4004".parse().unwrap(), "t"),
        "http://192.0.2.1:4004/t"
    );
}

#[test]
fn tokens_differ() {
    assert_ne!(token(), token());
}

/// The events listener stops once the session closes, with nothing
/// connecting to it: when the Mac changes network, the address it
/// listens at goes, and nothing can.
#[test]
fn the_events_listener_stops_without_being_woken() {
    let fake = crate::fake::FakeRenderer::start(crate::fake::Kind::Tv);
    let (events, _events) = unbounded();
    let inner = Arc::new(Inner {
        device: fake.device(),
        state: Mutex::default(),
        events,
        callback: format!("http://127.0.0.1:1/{}", token()),
        ended: AtomicBool::new(false),
        poll_every: AtomicU64::new(POLL.as_millis() as u64),
        polling: Mutex::new(None),
    });
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let closing = Arc::new(AtomicBool::new(false));
    let (stopped, stops) = mpsc::channel();
    let closed = Arc::clone(&closing);
    thread::spawn(move || {
        listen(&inner, &listener, &[], &closed);
        let _ = stopped.send(());
    });
    thread::sleep(Duration::from_millis(50));
    closing.store(true, Ordering::Relaxed);
    assert!(
        stops.recv_timeout(Duration::from_secs(2)).is_ok(),
        "the listener still waits"
    );
}
