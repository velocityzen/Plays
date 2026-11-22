//! The TLS connection to a device: Cast messages each way, read on one
//! thread while others write. Devices sign their own certificates, so any
//! is taken, though the handshake is still checked against it.

use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use network::lock;
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
        let (tcp, remote) = network::connect_first(addresses, CONNECT_TIMEOUT)?;
        let name = ServerName::IpAddress(remote.ip().into());
        let tls = ClientConnection::new(client_config()?, name).map_err(io::Error::other)?;
        Self::start(tcp, tls.into())
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
#[path = "tests/channel.rs"]
mod tests;
