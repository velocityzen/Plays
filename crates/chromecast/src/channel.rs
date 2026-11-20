//! The TLS connection to a device: Cast messages each way, read on one
//! thread while others write. Devices sign their own certificates, so any
//! is taken, though the handshake is still checked against it.

use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, verify_tls12_signature, verify_tls13_signature};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, ClientConnection, Connection, DigitallySignedStruct, SignatureScheme};

use crate::proto::{self, CastMessage};

/// How long each address has to take the connection.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
/// How long a write waits for the device to take it: one that takes
/// nothing for that long is gone.
#[cfg(not(test))]
const WRITE_TIMEOUT: Duration = Duration::from_secs(10);
#[cfg(test)]
const WRITE_TIMEOUT: Duration = Duration::from_millis(500);

pub(crate) fn provider() -> Arc<CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

/// Any certificate, as a device's is its own; the handshake's signature is
/// still checked against it.
#[derive(Debug)]
struct AnyCertificate(Arc<CryptoProvider>);

impl ServerCertVerifier for AnyCertificate {
    fn verify_server_cert(
        &self,
        _: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        _: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        certificate: &CertificateDer<'_>,
        signed: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(
            message,
            certificate,
            signed,
            &self.0.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        certificate: &CertificateDer<'_>,
        signed: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(
            message,
            certificate,
            signed,
            &self.0.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

fn client_config() -> io::Result<Arc<ClientConfig>> {
    let provider = provider();
    let config = ClientConfig::builder_with_provider(Arc::clone(&provider))
        .with_safe_default_protocol_versions()
        .map_err(io::Error::other)?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(AnyCertificate(provider)))
        .with_no_client_auth();
    Ok(Arc::new(config))
}

fn lock(tls: &Mutex<Connection>) -> MutexGuard<'_, Connection> {
    tls.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Write what TLS has to send.
fn flush(tls: &mut Connection, mut tcp: &TcpStream) -> io::Result<()> {
    while tls.wants_write() {
        tls.write_tls(&mut tcp)?;
    }
    Ok(())
}

/// The writing half.
pub(crate) struct Channel {
    tls: Arc<Mutex<Connection>>,
    tcp: TcpStream,
}

/// The reading half, for one thread.
pub(crate) struct Incoming {
    tls: Arc<Mutex<Connection>>,
    tcp: TcpStream,
    /// What's been read and not yet made into messages.
    plain: Vec<u8>,
    closed: bool,
}

impl Channel {
    /// Connect at the first of `addresses` that takes it.
    pub fn connect(addresses: &[SocketAddr]) -> io::Result<(Self, Incoming)> {
        let mut refused = None;
        for remote in addresses {
            match TcpStream::connect_timeout(remote, CONNECT_TIMEOUT) {
                Ok(tcp) => {
                    let name = ServerName::IpAddress(remote.ip().into());
                    let tls =
                        ClientConnection::new(client_config()?, name).map_err(io::Error::other)?;
                    return Self::start(tcp, tls.into());
                }
                Err(err) => {
                    log::debug!("chromecast: {remote}: {err}");
                    refused = Some(err);
                }
            }
        }
        Err(refused.unwrap_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no address")))
    }

    /// Shake hands on `tcp` as `tls` is to, client or server.
    pub fn start(mut tcp: TcpStream, mut tls: Connection) -> io::Result<(Self, Incoming)> {
        tcp.set_nodelay(true)?;
        tcp.set_read_timeout(Some(HANDSHAKE_TIMEOUT))?;
        tcp.set_write_timeout(Some(WRITE_TIMEOUT))?;
        while tls.is_handshaking() {
            tls.complete_io(&mut tcp)?;
        }
        tcp.set_read_timeout(None)?;
        let tls = Arc::new(Mutex::new(tls));
        let channel = Self {
            tls: Arc::clone(&tls),
            tcp: tcp.try_clone()?,
        };
        let incoming = Incoming {
            tls,
            tcp,
            plain: Vec::new(),
            closed: false,
        };
        Ok((channel, incoming))
    }

    /// Send a message; if the device won't take it in time, the
    /// connection is closed, which ends the reading half.
    pub fn send(&self, message: &CastMessage) -> io::Result<()> {
        let mut tls = lock(&self.tls);
        tls.writer().write_all(&message.frame())?;
        let flushed = flush(&mut tls, &self.tcp);
        if flushed.is_err() {
            let _ = self.tcp.shutdown(Shutdown::Both);
        }
        flushed
    }

    /// Say goodbye and close the connection, which ends the reading half.
    pub fn close(&self) {
        let mut tls = lock(&self.tls);
        tls.send_close_notify();
        let _ = flush(&mut tls, &self.tcp);
        let _ = self.tcp.shutdown(Shutdown::Both);
    }
}

impl Incoming {
    /// How long a read waits before `next` gives up with `WouldBlock` or
    /// `TimedOut`; it can be called again after.
    pub fn set_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        self.tcp.set_read_timeout(timeout)
    }

    /// The next message, or `None` once the other side has closed.
    pub fn next(&mut self) -> io::Result<Option<CastMessage>> {
        loop {
            if let Some(message) = proto::take(&mut self.plain)? {
                return Ok(Some(message));
            }
            if self.closed {
                return Ok(None);
            }
            let mut buffer = [0; 16 * 1024];
            // Read without the lock, so writing goes on meanwhile.
            let n = (&self.tcp).read(&mut buffer)?;
            if n == 0 {
                self.closed = true;
                continue;
            }
            let mut tls = lock(&self.tls);
            let mut rest = &buffer[..n];
            while !rest.is_empty() {
                // TLS takes no more once the other side has said goodbye, so
                // whatever came after that is dropped.
                if tls.read_tls(&mut rest)? == 0 {
                    self.closed = true;
                    break;
                }
                let state = tls.process_new_packets().map_err(io::Error::other)?;
                let ready = state.plaintext_bytes_to_read();
                if ready > 0 {
                    let start = self.plain.len();
                    self.plain.resize(start + ready, 0);
                    tls.reader().read_exact(&mut self.plain[start..])?;
                }
                if state.peer_has_closed() {
                    self.closed = true;
                    break;
                }
            }
            flush(&mut tls, &self.tcp)?;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, TcpListener};
    use std::sync::mpsc;
    use std::thread;

    use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
    use rustls::{ServerConfig, ServerConnection};
    use serde_json::json;

    use super::*;
    use crate::proto::{HEARTBEAT, PLATFORM};

    fn server_config() -> Arc<ServerConfig> {
        let made = rcgen::generate_simple_self_signed(vec!["localhost".to_owned()])
            .expect("a certificate");
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
}
