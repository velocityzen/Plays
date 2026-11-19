//! The UDP side of sending sound: RTP audio packets, sync packets that tie
//! RTP time to the clock, timing replies to the receiver's clock requests,
//! and resending what it missed.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const SAMPLE_RATE: u32 = 44_100;
/// Frames in each audio packet.
pub const FRAMES_PER_PACKET: usize = 352;
pub const CHANNELS: usize = 2;

const AUDIO: u8 = 0x60;
const SYNC: u8 = 0x54;
const TIMING_REQUEST: u8 = 0x52;
const TIMING_REPLY: u8 = 0x53;
const RETRANSMIT_REQUEST: u8 = 0x55;
const RETRANSMIT_REPLY: u8 = 0x56;
/// The marker bit, on the first packet of a stream.
const MARKER: u8 = 0x80;

/// Seconds since 1900, as NTP counts them, in 32.32 fixed point.
pub fn ntp(time: SystemTime) -> u64 {
    let since = time.duration_since(UNIX_EPOCH).unwrap_or_default() + NTP_TO_UNIX;
    (since.as_secs() << 32) | ((u64::from(since.subsec_nanos()) << 32) / 1_000_000_000)
}

pub fn ntp_now() -> u64 {
    ntp(SystemTime::now())
}

/// From 1900 to 1970.
const NTP_TO_UNIX: Duration = Duration::from_secs(2_208_988_800);

/// NTP time `frames` after `start`.
pub fn ntp_after(start: u64, frames: u64) -> u64 {
    start + (frames << 32) / u64::from(SAMPLE_RATE)
}

pub fn audio_header(first: bool, seq: u16, timestamp: u32, ssrc: u32) -> [u8; 12] {
    let mut header = [0; 12];
    header[0] = 0x80;
    header[1] = AUDIO | if first { MARKER } else { 0 };
    header[2..4].copy_from_slice(&seq.to_be_bytes());
    header[4..8].copy_from_slice(&timestamp.to_be_bytes());
    header[8..12].copy_from_slice(&ssrc.to_be_bytes());
    header
}

/// Interleaved samples as RTP carries them: 16-bit, big-endian.
pub fn payload(samples: &[i16]) -> Vec<u8> {
    samples
        .iter()
        .flat_map(|sample| sample.to_be_bytes())
        .collect()
}

/// Tells the receiver that the frame at RTP time `now` is sent at NTP time
/// `ntp`, and to play it `latency` frames later.
pub fn sync(first: bool, now: u32, latency: u32, ntp: u64) -> [u8; 20] {
    let mut packet = [0; 20];
    packet[0] = if first { 0x90 } else { 0x80 };
    packet[1] = SYNC | MARKER;
    packet[2..4].copy_from_slice(&7u16.to_be_bytes());
    packet[4..8].copy_from_slice(&now.wrapping_sub(latency).to_be_bytes());
    packet[8..16].copy_from_slice(&ntp.to_be_bytes());
    packet[16..20].copy_from_slice(&now.to_be_bytes());
    packet
}

/// The answer to a timing request: its send time back, and when it came
/// and goes, on our clock.
pub fn timing_reply(request: &[u8], now: u64) -> Option<[u8; 32]> {
    if request.len() < 32 || request[1] & !MARKER != TIMING_REQUEST {
        return None;
    }
    let mut reply = [0; 32];
    reply[0] = 0x80;
    reply[1] = TIMING_REPLY | MARKER;
    reply[2..4].copy_from_slice(&7u16.to_be_bytes());
    reply[8..16].copy_from_slice(&request[24..32]);
    reply[16..24].copy_from_slice(&now.to_be_bytes());
    reply[24..32].copy_from_slice(&now.to_be_bytes());
    Some(reply)
}

/// The packets a resend request asks for: the first sequence number and
/// how many.
pub fn resend_request(packet: &[u8]) -> Option<(u16, u16)> {
    if packet.len() < 8 || packet[1] & !MARKER != RETRANSMIT_REQUEST {
        return None;
    }
    Some((
        u16::from_be_bytes([packet[4], packet[5]]),
        u16::from_be_bytes([packet[6], packet[7]]),
    ))
}

/// A packet sent again, wrapped as a reply.
pub fn resend(packet: &[u8]) -> Vec<u8> {
    let mut reply = vec![0x80, RETRANSMIT_REPLY | MARKER, packet[2], packet[3]];
    reply.extend_from_slice(packet);
    reply
}

/// The receiver's side, for the fake one.
#[cfg(any(test, feature = "fake"))]
pub mod receiver {
    use super::*;

    pub fn timing_request(seq: u16, now: u64) -> [u8; 32] {
        let mut request = [0; 32];
        request[0] = 0x80;
        request[1] = TIMING_REQUEST | MARKER;
        request[2..4].copy_from_slice(&seq.to_be_bytes());
        request[24..32].copy_from_slice(&now.to_be_bytes());
        request
    }

    pub fn resend_request(first: u16, count: u16) -> [u8; 8] {
        let mut request = [0x80, RETRANSMIT_REQUEST | MARKER, 0, 1, 0, 0, 0, 0];
        request[4..6].copy_from_slice(&first.to_be_bytes());
        request[6..8].copy_from_slice(&count.to_be_bytes());
        request
    }

    /// An audio packet's sequence number and RTP time.
    pub fn audio(packet: &[u8]) -> Option<(u16, u32)> {
        (packet.len() >= 12 && packet[1] & !MARKER == AUDIO).then(|| {
            (
                u16::from_be_bytes([packet[2], packet[3]]),
                u32::from_be_bytes(packet[4..8].try_into().unwrap()),
            )
        })
    }

    /// A sync packet's RTP time less latency, NTP time and RTP time.
    pub fn sync(packet: &[u8]) -> Option<(u32, u64, u32)> {
        (packet.len() >= 20 && packet[1] & !MARKER == SYNC).then(|| {
            (
                u32::from_be_bytes(packet[4..8].try_into().unwrap()),
                u64::from_be_bytes(packet[8..16].try_into().unwrap()),
                u32::from_be_bytes(packet[16..20].try_into().unwrap()),
            )
        })
    }

    /// A resent packet's original.
    pub fn resent(packet: &[u8]) -> Option<&[u8]> {
        (packet.len() > 4 && packet[1] & !MARKER == RETRANSMIT_REPLY).then(|| &packet[4..])
    }

    /// Interleaved samples from an RTP payload.
    pub fn samples(payload: &[u8]) -> Vec<i16> {
        payload
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| i16::from_be_bytes(*pair))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ntp_counts_from_1900_in_fixed_point() {
        let half = UNIX_EPOCH + Duration::from_millis(500);
        assert_eq!(ntp(half), (2_208_988_800u64 << 32) | (1 << 31));
        assert_eq!(ntp_after(0, u64::from(SAMPLE_RATE)), 1 << 32);
    }

    #[test]
    fn a_timing_reply_echoes_the_requests_send_time() {
        let request = receiver::timing_request(3, 0x1122_3344_5566_7788);
        let reply = timing_reply(&request, 42).unwrap();
        assert_eq!(&reply[..2], [0x80, 0xD3]);
        assert_eq!(reply[8..16], 0x1122_3344_5566_7788u64.to_be_bytes());
        assert_eq!(reply[16..24], 42u64.to_be_bytes());
        assert_eq!(timing_reply(&[0; 8], 42), None);
    }

    #[test]
    fn a_sync_packet_says_where_playing_is_behind_sending() {
        let packet = sync(true, 100_000, 66_150, 7);
        assert_eq!(&packet[..2], [0x90, 0xD4]);
        assert_eq!(
            receiver::sync(&packet),
            Some((100_000 - 66_150, 7, 100_000))
        );
        // RTP time wraps.
        assert_eq!(
            receiver::sync(&sync(false, 5, 10, 0)).unwrap().0,
            u32::MAX - 4
        );
    }

    #[test]
    fn a_resent_packet_is_the_original_wrapped() {
        let header = audio_header(false, 513, 9, 1);
        let mut packet = header.to_vec();
        packet.extend(payload(&[1, -2]));
        assert_eq!(
            resend_request(&receiver::resend_request(513, 2)),
            Some((513, 2))
        );
        let reply = resend(&packet);
        assert_eq!(receiver::resent(&reply), Some(packet.as_slice()));
        assert_eq!(receiver::audio(&packet), Some((513, 9)));
        assert_eq!(receiver::samples(&packet[12..]), [1, -2]);
    }
}
