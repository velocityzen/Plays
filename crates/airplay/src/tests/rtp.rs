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
