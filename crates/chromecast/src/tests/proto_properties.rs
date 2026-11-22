use proptest::collection::vec;
use proptest::option;
use proptest::prelude::*;

use super::*;

proptest! {
    #[test]
    fn messages_come_out_whole_however_they_arrive(
        messages in vec(message(), 1..4),
        steps in vec(1usize..64, 1..6),
    ) {
        let wire: Vec<u8> = messages.iter().flat_map(CastMessage::frame).collect();
        let mut buffer = Vec::new();
        let mut taken = Vec::new();
        let mut at = 0;
        for step in steps.iter().cycle() {
            let end = (at + step).min(wire.len());
            buffer.extend_from_slice(&wire[at..end]);
            at = end;
            while let Some(message) = take(&mut buffer).unwrap() {
                taken.push(message);
            }
            if at == wire.len() {
                break;
            }
        }
        prop_assert_eq!(taken, messages);
        prop_assert!(buffer.is_empty());
    }

    #[test]
    fn a_length_past_the_most_is_refused_before_waiting_for_it(
        length in prop_oneof![MAX as u32 + 1..=2 * MAX as u32, MAX as u32 + 1..=u32::MAX],
        rest in vec(any::<u8>(), 0..64),
    ) {
        let mut buffer = length.to_be_bytes().to_vec();
        buffer.extend(rest);
        prop_assert!(take(&mut buffer).is_err());
    }

    #[test]
    fn a_message_that_isnt_one_is_taken_out_whole_and_what_follows_kept(
        garbage in vec(any::<u8>(), 0..256),
        after in vec(any::<u8>(), 0..8),
    ) {
        let mut buffer = (garbage.len() as u32).to_be_bytes().to_vec();
        buffer.extend(&garbage);
        buffer.extend(&after);
        let _ = take(&mut buffer);
        prop_assert_eq!(buffer, after);
    }
}

fn message() -> impl Strategy<Value = CastMessage> {
    (
        (any::<i32>(), ".{0,20}", ".{0,20}", ".{0,40}"),
        (
            any::<i32>(),
            option::of(".{0,200}"),
            option::of(vec(any::<u8>(), 0..200)),
        ),
    )
        .prop_map(
            |(
                (protocol_version, source_id, destination_id, namespace),
                (payload_type, payload_utf8, payload_binary),
            )| CastMessage {
                protocol_version,
                source_id,
                destination_id,
                namespace,
                payload_type,
                payload_utf8,
                payload_binary,
            },
        )
}
