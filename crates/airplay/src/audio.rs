//! Live sound to a speaker: 16-bit stereo PCM at 44.1 kHz, sent as it is
//! due, so the writer keeps time with the speaker. AirPlay 2 speakers take
//! it as realtime audio, each packet sealed with a key sent in SETUP;
//! AirPlay 1 ones as RAOP: plain, or, to a speaker that takes it only
//! encrypted, in AES under a key the ANNOUNCE gives wrapped for it.

use std::collections::VecDeque;
use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use futures_channel::mpsc::UnboundedReceiver;
use plist::{Dictionary, Value};

use crate::connection::Request;
use crate::crypto;
use crate::device::Device;
use crate::error::{Error, Result};
use crate::pairing::Credentials;
use crate::rsa::PublicKey;
use crate::rtp::{self, CHANNELS, FRAMES_PER_PACKET, SAMPLE_RATE};
use crate::session::{Control, Event, Sender, unsigned};

/// How far behind sending the speaker plays, in frames: a second and a
/// half, which rides out the network.
pub const LATENCY: u32 = 22_050 + SAMPLE_RATE;
/// The packets kept to send again.
const BACKLOG: usize = 1000;
/// AirPlay 2's code for 16-bit stereo PCM at 44.1 kHz.
const PCM_44100_16_2: u64 = 0x800;
/// The lowest volume in dB, and silence.
const MIN_DB: f32 = -30.0;
const MUTED_DB: f32 = -144.0;

type Backlog = Arc<Mutex<VecDeque<(u16, Vec<u8>)>>>;

/// How the packets go.
enum Encryption {
    /// Plain, to an AirPlay 1 speaker that takes it so.
    None,
    /// AirPlay 2: each packet sealed with the key sent in SETUP.
    Sealed([u8; 32]),
    /// AirPlay 1: each packet's whole blocks in AES-128 CBC, each packet
    /// from the IV; the ANNOUNCE gives both, the key wrapped for the
    /// speaker.
    Aes {
        key: [u8; crypto::BLOCK],
        iv: [u8; crypto::BLOCK],
    },
}

/// The volume as AirPlay says it: dB from -30 to 0, or -144 for silence.
pub fn volume_db(volume: f32, muted: bool) -> f32 {
    if muted || volume <= 0.0 {
        MUTED_DB
    } else {
        MIN_DB - MIN_DB * volume.min(1.0)
    }
}

/// The volume, 0 to 1, from dB.
pub fn volume_from_db(db: f32) -> f32 {
    if db < MIN_DB {
        0.0
    } else {
        (1.0 - db / MIN_DB).min(1.0)
    }
}

pub struct AudioStream {
    control: Control,
    /// Where the packets go.
    data: UdpSocket,
    /// Ours, for sync packets out and resend requests in.
    control_socket: Arc<UdpSocket>,
    receiver_control: SocketAddr,
    encryption: Encryption,
    ssrc: u32,
    seq: u16,
    /// RTP time of the next frame.
    timestamp: u32,
    /// Packets sealed, for their nonces.
    sealed: u64,
    /// Samples short of a packet.
    pending: Vec<i16>,
    /// When the stream started, as an instant and on NTP's clock, and the
    /// frames sent since.
    started: Option<(Instant, u64)>,
    sent: u64,
    since_sync: u64,
    /// The speaker's own delay, when it says.
    output_latency: u32,
    backlog: Backlog,
}

impl AudioStream {
    /// Connect to `device` and set up a stream; sound goes once written.
    pub fn connect(
        device: &Device,
        credentials: Option<&Credentials>,
        sender: &Sender,
    ) -> Result<(Self, UnboundedReceiver<Event>)> {
        if !device.plays_audio() {
            return Err(Error::protocol("it doesn't play sound"));
        }
        // AirPlay 1's encryptions (`et`): 0 none, 1 RSA; the rest are
        // FairPlay's and MFi's, which this doesn't do.
        let encryption = if device.airplay2() {
            Encryption::Sealed(rand::random())
        } else if device.encryptions.is_empty() || device.encryptions.contains(&0) {
            Encryption::None
        } else if device.encryptions.contains(&1) {
            Encryption::Aes {
                key: rand::random(),
                iv: rand::random(),
            }
        } else {
            return Err(Error::protocol(
                "it takes AirPlay 1 sound only in FairPlay or MFi encryption, which isn't supported",
            ));
        };
        let (mut control, events) = Control::connect(device, credentials)?;
        let local = control.connection().local;
        let control_socket = Arc::new(UdpSocket::bind((local, 0))?);
        let control_port = control_socket.local_addr()?.port();
        let (data_port, receiver_control_port, output_latency) = match &encryption {
            Encryption::Sealed(key) => {
                control.setup(sender)?;
                setup_stream(&control, control_port, *key)?
            }
            _ => announce_and_setup(&mut control, control_port, &encryption)?,
        };
        let remote = control.remote.ip();
        let data = UdpSocket::bind((local, 0))?;
        data.connect((remote, data_port))?;

        let backlog = Backlog::default();
        let resends = Arc::clone(&control_socket);
        let kept = Arc::clone(&backlog);
        control.spawn("airplay resends", move |stop| {
            let _ = resends.set_read_timeout(Some(Duration::from_millis(100)));
            let mut buffer = [0; 64];
            while !stop.load(Ordering::Relaxed) {
                let Ok((n, from)) = resends.recv_from(&mut buffer) else {
                    continue;
                };
                let Some((first, count)) = rtp::resend_request(&buffer[..n]) else {
                    continue;
                };
                let kept = kept
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                for seq in (0..count).map(|i| first.wrapping_add(i)) {
                    if let Some((_, packet)) = kept.iter().find(|(kept, _)| *kept == seq) {
                        let _ = resends.send_to(&rtp::resend(packet), from);
                    }
                }
            }
        })?;

        let seq = rand::random();
        let mut stream = Self {
            control,
            data,
            control_socket,
            receiver_control: SocketAddr::new(remote, receiver_control_port),
            encryption,
            ssrc: rand::random(),
            seq,
            timestamp: LATENCY,
            sealed: 0,
            pending: Vec::new(),
            started: None,
            sent: 0,
            since_sync: 0,
            output_latency,
            backlog,
        };
        stream.record()?;
        Ok((stream, events))
    }

    /// Where the next frame starts, for RECORD and FLUSH.
    fn rtp_info(&self) -> String {
        format!("seq={};rtptime={}", self.seq, self.timestamp)
    }

    fn record(&mut self) -> Result<()> {
        let uri = self.control.uri();
        self.control.exchange(
            Request::rtsp("RECORD", uri)
                .header("Range", "npt=0-")
                .header("RTP-Info", self.rtp_info()),
        )?;
        Ok(())
    }

    /// How long after it's written a frame is heard.
    pub fn latency(&self) -> Duration {
        frames(u64::from(LATENCY + self.output_latency))
    }

    /// Frames written since the stream started, or since the last flush.
    pub fn frames(&self) -> u64 {
        self.sent + (self.pending.len() / CHANNELS) as u64
    }

    /// When the `frame`th frame since the stream started (or the last
    /// flush) is heard; `None` before anything is sent.
    pub fn heard_at(&self, frame: u64) -> Option<Instant> {
        let (started, _) = self.started?;
        Some(started + frames(frame) + self.latency())
    }

    /// Send interleaved stereo samples at 44.1 kHz. Waits until each packet
    /// is due, so the caller keeps time with the speaker.
    pub fn write(&mut self, samples: &[i16]) -> Result<()> {
        self.pending.extend_from_slice(samples);
        let packet = FRAMES_PER_PACKET * CHANNELS;
        while self.pending.len() >= packet {
            let samples: Vec<i16> = self.pending.drain(..packet).collect();
            self.send(&samples)?;
        }
        Ok(())
    }

    fn send(&mut self, samples: &[i16]) -> Result<()> {
        let first = self.started.is_none();
        let (started, started_ntp) = *self
            .started
            .get_or_insert_with(|| (Instant::now(), rtp::ntp_now()));
        if let Some(wait) = (started + frames(self.sent)).checked_duration_since(Instant::now()) {
            thread::sleep(wait);
        }
        if first || self.since_sync >= u64::from(SAMPLE_RATE) {
            let sync = rtp::sync(
                first,
                self.timestamp,
                LATENCY,
                rtp::ntp_after(started_ntp, self.sent),
            );
            self.control_socket.send_to(&sync, self.receiver_control)?;
            self.since_sync = 0;
        }
        let header = rtp::audio_header(first, self.seq, self.timestamp, self.ssrc);
        let mut payload = rtp::payload(samples);
        let mut packet = header.to_vec();
        match &self.encryption {
            Encryption::Sealed(key) => {
                let nonce = self.sealed.to_le_bytes();
                self.sealed += 1;
                packet.extend(crypto::seal(key, nonce, &header[4..12], &payload));
                packet.extend_from_slice(&nonce);
            }
            Encryption::Aes { key, iv } => {
                crypto::cbc_encrypt(key, iv, &mut payload);
                packet.extend(payload);
            }
            Encryption::None => packet.extend(payload),
        }
        self.data.send(&packet)?;
        let mut backlog = self
            .backlog
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if backlog.len() == BACKLOG {
            backlog.pop_front();
        }
        backlog.push_back((self.seq, packet));
        drop(backlog);

        let n = (samples.len() / CHANNELS) as u64;
        self.seq = self.seq.wrapping_add(1);
        self.timestamp = self.timestamp.wrapping_add(n as u32);
        self.sent += n;
        self.since_sync += n;
        Ok(())
    }

    /// Drop what the speaker has yet to play, and what's short of a
    /// packet: for a pause or a seek. The next write starts afresh.
    pub fn flush(&mut self) -> Result<()> {
        self.pending.clear();
        let uri = self.control.uri();
        self.control.exchange(
            Request::rtsp("FLUSH", uri)
                .header("Range", "npt=0-")
                .header("RTP-Info", self.rtp_info()),
        )?;
        self.started = None;
        self.sent = 0;
        Ok(())
    }

    /// The speaker's volume, 0 to 1, or silent.
    pub fn set_volume(&mut self, volume: f32, muted: bool) -> Result<()> {
        let uri = self.control.uri();
        self.control.exchange(
            Request::rtsp("SET_PARAMETER", uri)
                .parameters(format!("volume: {:.6}\r\n", volume_db(volume, muted))),
        )?;
        Ok(())
    }

    /// The speaker's volume, 0 to 1, as it says.
    pub fn volume(&mut self) -> Result<f32> {
        let uri = self.control.uri();
        let answer = self
            .control
            .exchange(Request::rtsp("GET_PARAMETER", uri).parameters("volume\r\n".into()))?;
        let text = String::from_utf8_lossy(&answer.body);
        text.split_once(':')
            .and_then(|(_, db)| db.trim().parse::<f32>().ok())
            .map(volume_from_db)
            .ok_or_else(|| Error::protocol(format!("a volume of {text:?}")))
    }

    /// End the stream.
    pub fn stop(self) {
        self.control.stop();
    }
}

fn frames(n: u64) -> Duration {
    Duration::from_nanos(n * 1_000_000_000 / u64::from(SAMPLE_RATE))
}

/// AirPlay 2's second SETUP: the stream, with the key its packets are
/// sealed with. Gives the receiver's data and control ports.
fn setup_stream(control: &Control, control_port: u16, key: [u8; 32]) -> Result<(u16, u16, u32)> {
    let int = |n: u64| Value::Integer(n.into());
    let mut stream = Dictionary::new();
    stream.insert("audioFormat".into(), int(PCM_44100_16_2));
    stream.insert("audioMode".into(), "default".into());
    stream.insert("controlPort".into(), int(control_port.into()));
    stream.insert("ct".into(), int(1));
    stream.insert("isMedia".into(), true.into());
    stream.insert("latencyMax".into(), int(88_200));
    stream.insert("latencyMin".into(), int(11_025));
    stream.insert("shk".into(), Value::Data(key.to_vec()));
    stream.insert("spf".into(), int(FRAMES_PER_PACKET as u64));
    stream.insert("sr".into(), int(SAMPLE_RATE.into()));
    stream.insert("type".into(), int(0x60));
    stream.insert("supportsDynamicStreamID".into(), false.into());
    stream.insert(
        "streamConnectionID".into(),
        int(control.connection().session.into()),
    );
    let mut body = Dictionary::new();
    body.insert(
        "streams".into(),
        Value::Array(vec![Value::Dictionary(stream)]),
    );
    let uri = control.uri();
    let answer = control
        .exchange(Request::rtsp("SETUP", uri).plist(Value::Dictionary(body)))?
        .dictionary()?;
    let stream = answer
        .get("streams")
        .and_then(Value::as_array)
        .and_then(|streams| streams.first())
        .and_then(Value::as_dictionary)
        .ok_or_else(|| Error::protocol("a stream SETUP without the stream"))?;
    let port = |key: &str| {
        unsigned(stream.get(key))
            .and_then(|port| u16::try_from(port).ok())
            .ok_or_else(|| Error::protocol(format!("a stream SETUP without {key}")))
    };
    Ok((port("dataPort")?, port("controlPort")?, 0))
}

/// The key a speaker at `address` unwraps the sound's key with: the
/// AirPort's, which they all hold, or, for a fake one here, its own.
fn speaker_key(address: SocketAddr) -> PublicKey {
    #[cfg(any(test, feature = "fake"))]
    if crate::fake::listens_at(address) {
        return crate::rsa::PrivateKey::fake().public().clone();
    }
    #[cfg(not(any(test, feature = "fake")))]
    let _ = address;
    PublicKey::airport()
}

/// AirPlay 1: ANNOUNCE the stream as PCM, with the key it's encrypted with
/// if it is, then SETUP with our ports. Gives the receiver's data and
/// control ports, and its own latency.
fn announce_and_setup(
    control: &mut Control,
    control_port: u16,
    encryption: &Encryption,
) -> Result<(u16, u16, u32)> {
    let (local, remote, session) = {
        let connection = control.connection();
        (connection.local, connection.remote.ip(), connection.session)
    };
    let family = if local.is_ipv4() { "IP4" } else { "IP6" };
    let mut sdp = format!(
        "v=0\r\no=iTunes {session} 0 IN {family} {local}\r\ns=iTunes\r\nc=IN {family} {remote}\r\n\
         t=0 0\r\nm=audio 0 RTP/AVP 96\r\na=rtpmap:96 L16/44100/2\r\n\
         a=fmtp:96 {FRAMES_PER_PACKET} 0 16 40 10 14 2 255 0 0 44100\r\n"
    );
    if let Encryption::Aes { key, iv } = encryption {
        let wrapped = speaker_key(control.remote).encrypt(key);
        sdp += &format!(
            "a=rsaaeskey:{}\r\na=aesiv:{}\r\n",
            crypto::base64(&wrapped),
            crypto::base64(iv)
        );
    }
    let uri = control.uri();
    control.exchange(
        Request::rtsp("ANNOUNCE", uri.clone()).body("application/sdp", sdp.into_bytes()),
    )?;
    let answer = control.exchange(Request::rtsp("SETUP", uri).header(
        "Transport",
        format!(
            "RTP/AVP/UDP;unicast;interleaved=0-1;mode=record;control_port={control_port};timing_port={}",
            control.timing_port
        ),
    ))?;
    if let Some(session) = answer.header("Session") {
        control.rtsp_session = Some(session.split(';').next().unwrap_or(session).to_owned());
    }
    let transport = answer.header("Transport").unwrap_or_default();
    let port = |key: &str| {
        transport
            .split(';')
            .filter_map(|part| part.split_once('='))
            .find(|(name, _)| *name == key)
            .and_then(|(_, port)| port.parse().ok())
            .ok_or_else(|| Error::protocol(format!("a SETUP without {key}")))
    };
    let latency = answer
        .header("Audio-Latency")
        .and_then(|frames| frames.parse().ok())
        .unwrap_or(0);
    Ok((port("server_port")?, port("control_port")?, latency))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_goes_from_minus_30_db_to_0_and_silence_is_minus_144() {
        assert_eq!(volume_db(1.0, false), 0.0);
        assert_eq!(volume_db(0.5, false), -15.0);
        assert_eq!(volume_db(0.0, false), -144.0);
        assert_eq!(volume_db(0.8, true), -144.0);
        assert_eq!(volume_from_db(-15.0), 0.5);
        assert_eq!(volume_from_db(-144.0), 0.0);
    }

    #[test]
    fn the_sounds_key_is_wrapped_for_the_airport_or_for_a_fake_here() {
        let real = "192.168.1.20:5000".parse().unwrap();
        assert_eq!(speaker_key(real), PublicKey::airport());
        let fake = crate::fake::FakeReceiver::start(crate::fake::Kind::EncryptedSpeaker);
        let address = fake.device().addresses[0];
        assert_eq!(
            speaker_key(address),
            *crate::rsa::PrivateKey::fake().public()
        );
        drop(fake);
        assert_eq!(speaker_key(address), PublicKey::airport());
    }
}
