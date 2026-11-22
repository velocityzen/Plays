use super::*;

fn packet(stream: usize, time: i64, duration: i64) -> Packet {
    let mut packet = Packet::empty();
    packet.set_stream(stream);
    packet.set_pts(Some(time));
    packet.set_dts(Some(time));
    packet.set_duration(duration);
    packet
}

#[test]
fn timestamps_at_the_ends_of_what_they_can_be_are_no_trouble() {
    let mut queue = PacketQueue::new(0, 1.0 / 1000.0);
    queue.packets.push_back(packet(0, i64::MIN + 1, 0));
    queue.packets.push_back(packet(0, i64::MAX - 1, 100));
    assert!(queue.span() > 0.0, "the first to the last");
    queue.keep_from(0.0, 10);
    assert_eq!(queue.packets.len(), 1, "the one ending before is gone");
    queue.keep_from(f64::MAX, 10);
    assert!(queue.packets.is_empty(), "and the last, ending before then");
}
