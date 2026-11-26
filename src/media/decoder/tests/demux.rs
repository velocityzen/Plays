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

#[test]
fn reads_a_length_as_matroska_tags_it() {
    assert_eq!(length_in_seconds("00:00:01.000000000"), Some(1.0));
    assert_eq!(length_in_seconds("01:02:03.500000000"), Some(3723.5));
    assert_eq!(length_in_seconds(" 00:05:00.021 "), Some(300.021));
    for wrong in [
        "",
        "1.5",
        "00:01",
        "00:00:01:00",
        "00:00:-1",
        "00:00:inf",
        "a:00:01",
        "00:b:01",
    ] {
        assert_eq!(length_in_seconds(wrong), None, "{wrong}");
    }
}

/// The stream made for the engine's tests, as a demuxer opens it, and its
/// picture: (index, time base).
fn stream() -> (Input, (usize, f64)) {
    ffmpeg::init().unwrap();
    let input =
        ffmpeg::format::input(&crate::media::tests::stream_media().join("stream.ts")).unwrap();
    let video = input.streams().best(ffmpeg::media::Type::Video).unwrap();
    let picture = (video.index(), f64::from(video.time_base()));
    (input, picture)
}

/// Read `count` packets of `input`; the time of the last of `stream`'s.
fn read_on(input: &mut Input, stream: (usize, f64), count: usize) -> f64 {
    let mut last = None;
    for _ in 0..count {
        let mut packet = Packet::empty();
        packet.read(input).unwrap();
        if packet.stream() == stream.0 {
            last = packet_time(&packet, stream.1);
        }
    }
    last.unwrap()
}

#[test]
fn a_live_stream_goes_on_from_its_first_keyframe_opened_again() {
    let (mut input, picture) = stream();
    let read = read_on(&mut input, picture, 200);
    let start = read_to(&mut stream().0, picture, true).keyframe.unwrap();
    assert!(read > start + 1.0, "read on from {start} to {read}");
    let landing = go_live(&mut input, || Ok(stream().0), Some(picture), "test");
    assert_eq!(
        landing.keyframe,
        Some(start),
        "where the input opened again is"
    );
    let last = landing.packets.last().unwrap();
    assert!(last.stream() == picture.0 && last.is_key());
    assert!(
        read_on(&mut input, picture, 20) < read,
        "and it reads on from there"
    );
}

#[test]
fn a_live_stream_that_wont_open_again_as_it_was_reads_on_where_it_was() {
    let audio_only = || {
        ffmpeg::format::input(&crate::ui::testing::fixture("tone.m4a")).map_err(anyhow::Error::from)
    };
    for reopen in [&audio_only as &dyn Fn() -> anyhow::Result<Input>, &|| {
        Err(anyhow::anyhow!("gone"))
    }] {
        let (mut input, picture) = stream();
        let read = read_on(&mut input, picture, 200);
        let landing = go_live(&mut input, reopen, Some(picture), "test");
        let keyframe = landing.keyframe.unwrap();
        assert!(
            keyframe > read,
            "its next keyframe, {keyframe}, after {read}"
        );
    }
}
