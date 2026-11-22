use std::net::{Ipv4Addr, TcpListener};
use std::sync::mpsc;
use std::thread;

use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::{ServerConfig, ServerConnection};
use serde_json::json;

use super::*;
use crate::proto::{HEARTBEAT, PLATFORM};

fn server_config() -> Arc<ServerConfig> {
    let made =
        rcgen::generate_simple_self_signed(vec!["localhost".to_owned()]).expect("a certificate");
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(made.signing_key.serialize_der()));
    let config = ServerConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()
        .expect("TLS 1.2 and 1.3")
        .with_no_client_auth()
        .with_single_cert(vec![made.cert.der().clone()], key)
        .expect("the certificate");
    Arc::new(config)
}

#[test]
fn what_comes_after_the_other_side_closes_is_dropped() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("a port");
    let address = listener.local_addr().expect("its address");
    let ping = CastMessage::json(PLATFORM, "sender-0", HEARTBEAT, &json!({"type": "PING"}));
    let sent = ping.clone();
    let (written, wrote) = mpsc::channel();
    thread::spawn(move || {
        let (mut tcp, _) = listener.accept().expect("the sender");
        let mut tls = ServerConnection::new(server_config()).expect("TLS");
        while tls.is_handshaking() {
            tls.complete_io(&mut tcp).expect("the handshake");
        }
        // A message, the goodbye, and then more than one read of TLS
        // takes, all in one write, so one read on the other side has it
        // all.
        tls.writer().write_all(&sent.frame()).expect("the message");
        tls.send_close_notify();
        let mut burst = Vec::new();
        while tls.wants_write() {
            tls.write_tls(&mut burst).expect("the records");
        }
        burst.extend([0x17; 6000]);
        tcp.write_all(&burst).expect("the burst");
        written.send(tcp).expect("the test");
    });
    let (_channel, mut incoming) = Channel::connect(&[address]).expect("a connection");
    let _tcp = wrote.recv().expect("the burst written");
    let (read, reads) = mpsc::channel();
    thread::spawn(move || {
        let _ = read.send(incoming.next().map_err(|err| err.to_string()));
        let _ = read.send(incoming.next().map_err(|err| err.to_string()));
    });
    let wait = Duration::from_secs(5);
    assert_eq!(reads.recv_timeout(wait), Ok(Ok(Some(ping))));
    assert_eq!(reads.recv_timeout(wait), Ok(Ok(None)));
}

#[test]
fn a_device_that_takes_nothing_more_is_given_up_on() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("a port");
    let address = listener.local_addr().expect("its address");
    let (shaken, hands) = mpsc::channel();
    thread::spawn(move || {
        let (mut tcp, _) = listener.accept().expect("the sender");
        let mut tls = ServerConnection::new(server_config()).expect("TLS");
        while tls.is_handshaking() {
            tls.complete_io(&mut tcp).expect("the handshake");
        }
        // Read nothing more, but keep the connection open.
        shaken.send(tcp).expect("the test");
    });
    let (channel, mut incoming) = Channel::connect(&[address]).expect("a connection");
    let _tcp = hands.recv().expect("the handshake");
    let padding = "x".repeat(60_000);
    let big = CastMessage::json(
        "sender-0",
        PLATFORM,
        HEARTBEAT,
        &json!({"type": "PING", "padding": padding}),
    );
    let (sent, sends) = mpsc::channel();
    thread::spawn(move || {
        // Far more than the sockets between hold.
        for _ in 0..4000 {
            if channel.send(&big).is_err() {
                let _ = sent.send(true);
                return;
            }
        }
        let _ = sent.send(false);
    });
    assert_eq!(
        sends.recv_timeout(Duration::from_secs(10)),
        Ok(true),
        "a write gave up"
    );
    let (read, reads) = mpsc::channel();
    thread::spawn(move || {
        let _ = read.send(matches!(incoming.next(), Ok(None) | Err(_)));
    });
    assert_eq!(
        reads.recv_timeout(Duration::from_secs(5)),
        Ok(true),
        "the reading half ended"
    );
}
