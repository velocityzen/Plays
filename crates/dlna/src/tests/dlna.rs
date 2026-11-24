//! Against the fake renderer: finding it and losing it, what it plays,
//! loading with subtitles, driving it, what it does told by events and by
//! asking, and every way a session ends.

use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::thread;
use std::time::{Duration, Instant};

use futures_channel::mpsc::UnboundedReceiver;

use crate::fake::{FakeRenderer, Kind as Renderer, RENDERING_CONTROL, SOMETHING_ELSE};
use crate::http::Url;
use crate::xml::Element;
use crate::*;

/// The first item `wanted` picks, waited for up to `wait`.
fn wait_for<T>(
    items: &mut UnboundedReceiver<T>,
    wait: Duration,
    wanted: impl Fn(&T) -> bool,
) -> Option<T> {
    let deadline = Instant::now() + wait;
    while Instant::now() < deadline {
        match items.try_recv() {
            Ok(item) if wanted(&item) => return Some(item),
            Ok(_) => {}
            Err(_) => thread::sleep(Duration::from_millis(5)),
        }
    }
    None
}

fn ended(events: &mut UnboundedReceiver<Event>, wait: Duration) -> Option<Ended> {
    match wait_for(events, wait, |event| matches!(event, Event::Ended(_))) {
        Some(Event::Ended(why)) => Some(why),
        _ => None,
    }
}

fn status(
    events: &mut UnboundedReceiver<Event>,
    wanted: impl Fn(&Status) -> bool,
) -> Option<Status> {
    match wait_for(
        events,
        Duration::from_secs(2),
        |event| matches!(event, Event::Status(status) if wanted(status)),
    ) {
        Some(Event::Status(status)) => Some(status),
        _ => None,
    }
}

fn volume(events: &mut UnboundedReceiver<Event>, wanted: Volume) -> bool {
    wait_for(events, Duration::from_secs(2), |event| {
        *event == Event::Volume(wanted)
    })
    .is_some()
}

fn film() -> Media {
    Media {
        url: "http://192.0.2.1:8080/cast/media".into(),
        mime: "video/mpeg".into(),
        features: Some(
            "DLNA.ORG_OP=10;DLNA.ORG_CI=1;DLNA.ORG_FLAGS=01700000000000000000000000000000".into(),
        ),
        kind: Kind::Video,
        title: Some("A Film".into()),
        duration: Some(5400.0),
        size: None,
        subtitles: Some(Subtitles {
            url: "http://192.0.2.1:8080/cast/subs.srt".into(),
            mime: "text/srt".into(),
        }),
    }
}

/// A session with `renderer`, playing the film.
fn playing(renderer: Renderer) -> (FakeRenderer, Session, UnboundedReceiver<Event>) {
    let fake = FakeRenderer::start(renderer);
    let (session, events) = Session::connect(&fake.device()).unwrap();
    session.load(&film()).unwrap();
    session.play().unwrap();
    (fake, session, events)
}

#[test]
fn a_renderer_is_found_by_searching_and_lost_when_it_says_byebye() {
    let fake = FakeRenderer::start(Renderer::Tv);
    let (browser, mut found) = Browser::start_at(fake.search_at()).unwrap();
    let Some(Found::Added(device)) = wait_for(&mut found, Duration::from_secs(2), |_| true) else {
        panic!("found")
    };
    assert_eq!(device, fake.device());
    assert_eq!(device.name, "[TV] Fake");
    assert_eq!(browser.devices(), std::slice::from_ref(&device));
    assert!(fake.state().searches >= 1);

    fake.byebye(browser.listening_at().unwrap());
    assert_eq!(
        wait_for(&mut found, Duration::from_secs(1), |_| true),
        Some(Found::Removed(device))
    );
    assert!(browser.devices().is_empty());
}

#[test]
fn a_renderer_is_found_by_its_advert_and_lost_when_its_max_age_runs_out() {
    let fake = FakeRenderer::start(Renderer::Tv);
    {
        let mut state = fake.state();
        state.answers = false;
        state.max_age = 1;
    }
    let (browser, mut found) = Browser::start_at(fake.search_at()).unwrap();
    fake.announce(browser.listening_at().unwrap());
    let Some(Found::Added(device)) = wait_for(&mut found, Duration::from_secs(2), |_| true) else {
        panic!("found by its advert")
    };
    let announced = Instant::now();
    assert_eq!(
        wait_for(&mut found, Duration::from_secs(2), |_| true),
        Some(Found::Removed(device))
    );
    assert!(announced.elapsed() > Duration::from_millis(800));
    assert_eq!(fake.state().searches, 0);
}

#[test]
fn what_it_plays_comes_from_its_protocol_info() {
    let fake = FakeRenderer::start(Renderer::Tv);
    let capabilities = fake.device().capabilities().unwrap();
    assert_eq!(
        capabilities,
        Capability::parse_list(include_str!("../../fixtures/samsung-protocol-info.txt"))
    );
    assert!(
        capabilities
            .iter()
            .any(|it| it.takes("video/mpeg") && it.profile.as_deref() == Some("MPEG_TS_HD_NA"))
    );
    assert_eq!(fake.state().received("GetProtocolInfo").len(), 1);
}

#[test]
fn loading_carries_the_url_and_didl_with_the_subtitles_both_ways() {
    let fake = FakeRenderer::start(Renderer::Tv);
    let (session, _events) = Session::connect(&fake.device()).unwrap();
    session.load(&film()).unwrap();
    let state = fake.state();
    let call = state.received("SetAVTransportURI")[0];
    assert_eq!(call.arg("InstanceID"), Some("0"));
    assert_eq!(call.arg("CurrentURI"), Some(film().url.as_str()));
    let didl = Element::parse(call.arg("CurrentURIMetaData").unwrap()).unwrap();
    let item = didl.child("item").unwrap();
    assert_eq!(item.child_text("title"), Some("A Film"));
    let res: Vec<&Element> = item.children("res").collect();
    assert_eq!(res[0].text(), film().url);
    assert_eq!(
        res[0].attribute("protocolInfo"),
        Some(film().protocol_info().as_str())
    );
    assert_eq!(res[0].attribute("duration"), Some("1:30:00.000"));
    assert_eq!(res[1].text(), "http://192.0.2.1:8080/cast/subs.srt");
    assert_eq!(
        res[1].attribute("protocolInfo"),
        Some("http-get:*:text/srt:*")
    );
    let caption = item.child("CaptionInfoEx").unwrap();
    assert_eq!(caption.text(), "http://192.0.2.1:8080/cast/subs.srt");
    assert_eq!(caption.attribute("type"), Some("srt"));
    assert_eq!(state.transport, "STOPPED");
}

#[test]
fn play_pause_seek_and_volume_reach_it() {
    let (fake, session, _events) = playing(Renderer::Tv);
    assert_eq!(fake.state().transport, "PLAYING");
    assert_eq!(fake.state().received("Play")[0].arg("Speed"), Some("1"));
    session.pause().unwrap();
    assert_eq!(fake.state().transport, "PAUSED_PLAYBACK");
    assert_eq!(session.transport_state().unwrap(), TransportState::Paused);
    session.seek(61.9).unwrap();
    {
        let state = fake.state();
        let seek = state.received("Seek")[0];
        assert_eq!(seek.arg("Unit"), Some("REL_TIME"));
        assert_eq!(seek.arg("Target"), Some("0:01:01"));
    }
    assert_eq!(
        session.position().unwrap(),
        Position {
            track_uri: Some(film().url),
            position: Some(61.0),
            duration: Some(5400.0),
        }
    );
    session.set_volume(0.35).unwrap();
    session.set_muted(true).unwrap();
    {
        let state = fake.state();
        assert_eq!(state.volume, 35);
        assert!(state.muted);
        let set = state.received("SetVolume")[0];
        assert_eq!(set.arg("Channel"), Some("Master"));
        assert_eq!(set.arg("DesiredVolume"), Some("35"));
    }
    assert_eq!(
        session.volume().unwrap(),
        Volume {
            level: 0.35,
            muted: true
        }
    );
}

#[test]
fn a_speaker_that_seeks_only_by_absolute_time_is_sought_so() {
    let (fake, session, _events) = playing(Renderer::Speaker);
    session.seek(10.0).unwrap();
    let state = fake.state();
    let units: Vec<Option<&str>> = state
        .received("Seek")
        .iter()
        .map(|seek| seek.arg("Unit"))
        .collect();
    assert_eq!(units, [Some("REL_TIME"), Some("ABS_TIME")]);
    assert_eq!(state.position, 10.0);
}

#[test]
fn refusals_say_why_as_upnp_does() {
    let fake = FakeRenderer::start(Renderer::Tv);
    let (session, _events) = Session::connect(&fake.device()).unwrap();
    assert!(matches!(
        session.play(),
        Err(Error::Upnp { fault: Fault::TransitionNotAvailable, code: 701, ref action, .. })
            if action == "Play"
    ));
    fake.state().refuse.push(("SetAVTransportURI".into(), 714));
    assert!(matches!(
        session.load(&film()),
        Err(Error::Upnp {
            fault: Fault::IllegalMimeType,
            ..
        })
    ));
    fake.state().refuse.clear();
    session.set_volume(0.5).unwrap();
    fake.state().refuse.push(("SetVolume".into(), 702));
    assert!(matches!(
        session.set_volume(0.5),
        Err(Error::Upnp {
            fault: Fault::InvalidInstanceId,
            code: 702,
            ..
        })
    ));
}

#[test]
fn loading_while_it_plays_stops_it_first_when_it_must() {
    let (fake, session, _events) = playing(Renderer::Tv);
    fake.state().locked_while_playing = true;
    let mut next = film();
    next.url = "http://192.0.2.1:8080/cast/next".into();
    session.load(&next).unwrap();
    let state = fake.state();
    let actions: Vec<&str> = state
        .calls
        .iter()
        .map(|call| call.action.as_str())
        .collect();
    assert!(
        actions.ends_with(&["SetAVTransportURI", "Stop", "SetAVTransportURI"]),
        "{actions:?}"
    );
    assert_eq!(state.uri, next.url);
}

#[test]
fn events_tell_what_it_does() {
    let fake = FakeRenderer::start(Renderer::Tv);
    let (session, mut events) = Session::connect(&fake.device()).unwrap();
    assert_eq!(fake.state().subscribers.len(), 2);
    assert!(
        volume(
            &mut events,
            Volume {
                level: 0.2,
                muted: false
            }
        ),
        "the first event says the volume"
    );
    session.load(&film()).unwrap();
    session.play().unwrap();
    let playing = status(&mut events, |status| {
        status.state == TransportState::Playing
    });
    assert_eq!(playing.unwrap().duration, Some(5400.0));
    fake.turn_volume(70);
    assert!(volume(
        &mut events,
        Volume {
            level: 0.7,
            muted: false
        }
    ));
    fake.finish();
    assert!(
        status(&mut events, |status| status.state
            == TransportState::Stopped)
        .is_some()
    );
    // Asking doesn't ask what events tell.
    assert!(fake.wait_for(|state| state.received("GetPositionInfo").len() >= 6));
    assert!(fake.state().received("GetTransportInfo").is_empty());
    assert!(fake.state().received("GetVolume").is_empty());
    assert_eq!(session.status().state, TransportState::Stopped);
}

/// This computer's address on its network: as far as the events listener
/// can tell, someone else on it.
fn elsewhere_on_the_network() -> IpAddr {
    if_addrs::get_if_addrs()
        .unwrap()
        .into_iter()
        .find(|interface| !interface.is_loopback() && interface.ip().is_ipv4())
        .expect("this computer is on a network")
        .ip()
}

/// Send the event `body` to `callback`, as subscription `sid`, from `from`.
fn notify_from(from: IpAddr, callback: &str, sid: &str, body: &str) {
    let url = Url::parse(callback).unwrap();
    let to = url.addresses().unwrap()[0];
    let socket = socket2::Socket::new(
        socket2::Domain::for_address(to),
        socket2::Type::STREAM,
        None,
    )
    .unwrap();
    socket.bind(&SocketAddr::new(from, 0).into()).unwrap();
    socket
        .connect_timeout(&to.into(), Duration::from_secs(2))
        .unwrap();
    let mut stream = TcpStream::from(socket);
    let _ = write!(
        stream,
        "NOTIFY {} HTTP/1.1\r\nHost: {}\r\nContent-Type: text/xml\r\n\
         NT: upnp:event\r\nNTS: upnp:propchange\r\nSID: {sid}\r\nSEQ: 1\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        url.path,
        url.authority(),
        body.len()
    );
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let _ = stream.read_to_end(&mut Vec::new());
}

/// Events come from the renderer alone: one from elsewhere on the network,
/// as anyone there could send, isn't heard.
#[test]
fn an_event_from_elsewhere_is_not_heard() {
    let fake = FakeRenderer::start(Renderer::Tv);
    fake.state().silent = true;
    let (_session, mut events) = Session::connect(&fake.device()).unwrap();
    let quiet = Volume {
        level: 0.2,
        muted: false,
    };
    assert!(volume(&mut events, quiet), "asked");
    let (callback, sid, loud) = {
        let mut state = fake.state();
        let subscriber = state
            .subscribers
            .iter()
            .find(|subscriber| subscriber.service == RENDERING_CONTROL)
            .cloned()
            .unwrap();
        state.volume = 90;
        let loud = state.last_change(RENDERING_CONTROL);
        state.volume = 20;
        (subscriber.callback, subscriber.sid, loud)
    };
    let heard = Volume {
        level: 0.9,
        muted: false,
    };

    let elsewhere = elsewhere_on_the_network();
    notify_from(elsewhere, &callback, &sid, &loud);
    assert!(!volume(&mut events, heard), "heard from {elsewhere}");
    // The same from the renderer's address is heard.
    notify_from(IpAddr::V4(Ipv4Addr::LOCALHOST), &callback, &sid, &loud);
    assert!(volume(&mut events, heard));
}

#[test]
fn without_events_it_is_asked() {
    let (fake, session, mut events) = playing(Renderer::Speaker);
    assert!(fake.state().subscribers.is_empty());
    assert!(
        status(&mut events, |status| status.state
            == TransportState::Playing)
        .is_some()
    );
    fake.state().position = 30.0;
    assert!(status(&mut events, |status| status.position == Some(30.0)).is_some());
    fake.state().volume = 55;
    assert!(volume(
        &mut events,
        Volume {
            level: 0.55,
            muted: false
        }
    ));
    fake.state().transport = "PAUSED_PLAYBACK".into();
    assert!(status(&mut events, |status| status.state == TransportState::Paused).is_some());
    drop(session);
}

/// A TV that takes subscriptions but whose events never come, as when a
/// firewall stops them, is asked what it does instead.
#[test]
fn a_tv_whose_events_never_come_is_asked() {
    let fake = FakeRenderer::start(Renderer::Tv);
    fake.state().silent = true;
    let (session, mut events) = Session::connect(&fake.device()).unwrap();
    assert_eq!(fake.state().subscribers.len(), 2, "subscribed");
    session.load(&film()).unwrap();
    session.play().unwrap();
    assert!(
        status(&mut events, |status| status.state
            == TransportState::Playing)
        .is_some()
    );
    fake.state().transport = "PAUSED_PLAYBACK".into();
    assert!(status(&mut events, |status| status.state == TransportState::Paused).is_some());
    fake.state().volume = 55;
    assert!(volume(
        &mut events,
        Volume {
            level: 0.55,
            muted: false
        }
    ));
    fake.finish();
    assert!(
        status(&mut events, |status| status.state
            == TransportState::Stopped)
        .is_some()
    );
}

#[test]
fn a_position_is_heard_when_it_first_comes_and_polls_can_come_often() {
    let (fake, session, mut events) = playing(Renderer::Speaker);
    let asked = |fake: &FakeRenderer| fake.state().received("GetPositionInfo").len();
    let before = Instant::now();
    fake.state().position = 12.0;
    let heard = status(&mut events, |status| status.position == Some(12.0))
        .and_then(|status| status.position_at)
        .expect("when it was heard");
    assert!(heard >= before);
    // Asked again, it's still where it was: heard there first, then.
    let polls = asked(&fake);
    assert!(fake.wait_for(|state| state.received("GetPositionInfo").len() >= polls + 3));
    assert_eq!(session.status().position_at, Some(heard));

    // Where it said it was, halfway between the asking and the answer:
    // it's then asked what it's doing, which takes as long again, so the
    // status comes one and a half answers after that.
    let answer_after = Duration::from_millis(200);
    fake.state().answer_after = answer_after;
    fake.state().position = 20.0;
    let heard = status(&mut events, |status| status.position == Some(20.0))
        .and_then(|status| status.position_at)
        .expect("when it was heard");
    let since = heard.elapsed();
    assert!(
        since >= answer_after * 5 / 4,
        "heard {since:?} ago, not when the answer came"
    );
    fake.state().answer_after = Duration::ZERO;

    // Asked now, whatever the wait it was in, then as often as asked: here
    // five times as often, for a picture following its sound.
    let polls = asked(&fake);
    session.poll_every(Some(Duration::from_secs(60)));
    assert!(fake.wait_for(|state| state.received("GetPositionInfo").len() > polls));
    let (polls, start) = (asked(&fake), Instant::now());
    session.poll_every(Some(Duration::from_millis(10)));
    assert!(fake.wait_for(|state| state.received("GetPositionInfo").len() >= polls + 10));
    assert!(
        start.elapsed() < Duration::from_millis(400),
        "ten polls in {:?}",
        start.elapsed()
    );
    drop(session);
}

#[test]
fn subscriptions_are_renewed_and_ended() {
    let fake = FakeRenderer::start(Renderer::Tv);
    fake.state().subscription = 1;
    let (session, _events) = Session::connect(&fake.device()).unwrap();
    assert!(fake.wait_for(|state| {
        state.subscribers.len() == 2 && state.subscribers.iter().all(|it| it.renewals >= 1)
    }));
    let mut sids: Vec<String> = fake
        .state()
        .subscribers
        .iter()
        .map(|it| it.sid.clone())
        .collect();
    drop(session);
    let mut state = fake.state();
    assert!(state.subscribers.is_empty());
    state.unsubscribed.sort();
    sids.sort();
    assert_eq!(state.unsubscribed, sids);
}

/// A subscription being renewed as the session ends is ended too, once
/// the renewal's answered, not left to run on.
#[test]
fn a_subscription_being_renewed_as_the_session_ends_is_ended() {
    let fake = FakeRenderer::start(Renderer::Tv);
    {
        let mut state = fake.state();
        state.subscription = 1;
        state.renew_after = Duration::from_millis(500);
    }
    let (session, _events) = Session::connect(&fake.device()).unwrap();
    assert!(fake.wait_for(|state| state.subscribers.iter().any(|it| it.renewals >= 1)));
    let sids: Vec<String> = fake
        .state()
        .subscribers
        .iter()
        .map(|it| it.sid.clone())
        .collect();
    drop(session);
    let mut state = fake.state();
    assert!(state.subscribers.is_empty(), "{:?}", state.subscribers);
    state.unsubscribed.sort();
    assert_eq!(state.unsubscribed, sids);
}

#[test]
fn a_subscription_that_cant_be_renewed_is_made_again() {
    let fake = FakeRenderer::start(Renderer::Tv);
    fake.state().subscription = 1;
    let (_session, _events) = Session::connect(&fake.device()).unwrap();
    let first = fake.state().subscribers[0].sid.clone();
    // As when the renderer restarted and forgot it.
    fake.state().subscribers.clear();
    assert!(fake.wait_for(|state| {
        state.subscribers.len() == 2 && state.subscribers.iter().all(|it| it.sid != first)
    }));
}

#[test]
fn the_session_ends_when_the_renderer_goes_away() {
    let (mut fake, session, mut events) = playing(Renderer::Tv);
    fake.go_away();
    assert_eq!(
        ended(&mut events, Duration::from_secs(1)),
        Some(Ended::Gone)
    );
    assert!(matches!(session.play(), Err(Error::Ended)));
}

#[test]
fn the_session_ends_when_something_else_plays() {
    let (fake, _session, mut events) = playing(Renderer::Tv);
    // Its track lags, so only the event tells.
    fake.state().track = Some(film().url);
    fake.play_something_else();
    assert_eq!(
        ended(&mut events, Duration::from_secs(1)),
        Some(Ended::Replaced)
    );
}

#[test]
fn the_session_ends_when_something_else_plays_on_a_speaker_asked() {
    let (fake, _session, mut events) = playing(Renderer::Speaker);
    {
        let mut state = fake.state();
        state.uri = SOMETHING_ELSE.into();
    }
    assert_eq!(
        ended(&mut events, Duration::from_secs(1)),
        Some(Ended::Replaced)
    );
}

#[test]
fn what_played_before_isnt_taken_for_something_else() {
    let fake = FakeRenderer::start(Renderer::Tv);
    fake.play_something_else();
    let (session, mut events) = Session::connect(&fake.device()).unwrap();
    assert!(
        status(&mut events, |status| status.state
            == TransportState::Playing)
        .is_some()
    );
    assert!(fake.wait_for(|state| state.received("GetPositionInfo").len() >= 2));
    session.load(&film()).unwrap();
    session.play().unwrap();
    // Nor is a track that lags what it was given.
    fake.state().track = Some(SOMETHING_ELSE.into());
    assert_eq!(ended(&mut events, Duration::from_millis(300)), None);
    assert!(
        !fake.state().received("GetMediaInfo").is_empty(),
        "it was asked"
    );
}

#[test]
fn stopping_stops_it_without_news() {
    let (fake, session, mut events) = playing(Renderer::Tv);
    session.stop();
    let state = fake.state();
    assert_eq!(state.transport, "STOPPED");
    assert_eq!(state.unsubscribed.len(), 2);
    drop(state);
    assert_eq!(ended(&mut events, Duration::from_millis(200)), None);
}
