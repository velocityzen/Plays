use hegel::TestCase;
use hegel::generators as gs;

use super::*;

#[hegel::test]
fn messages_come_out_whole_however_they_arrive(tc: TestCase) {
    let messages = tc.draw(gs::vecs(message()).min_size(1).max_size(3));
    let steps = tc.draw(
        gs::vecs(gs::integers::<usize>().min_value(1).max_value(63))
            .min_size(1)
            .max_size(5),
    );
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
    assert_eq!(taken, messages);
    assert!(buffer.is_empty());
}

#[hegel::test]
fn a_length_past_the_most_is_refused_before_waiting_for_it(tc: TestCase) {
    let past = MAX as u32 + 1;
    let length = tc.draw(hegel::one_of!(
        gs::integers::<u32>()
            .min_value(past)
            .max_value(2 * MAX as u32),
        gs::integers::<u32>().min_value(past),
    ));
    let mut buffer = length.to_be_bytes().to_vec();
    buffer.extend(tc.draw(gs::binary().max_size(63)));
    assert!(take(&mut buffer).is_err());
}

#[hegel::test]
fn a_message_that_isnt_one_is_taken_out_whole_and_what_follows_kept(tc: TestCase) {
    let garbage = tc.draw(gs::binary().max_size(255));
    let after = tc.draw(gs::binary().max_size(7));
    let mut buffer = (garbage.len() as u32).to_be_bytes().to_vec();
    buffer.extend(&garbage);
    buffer.extend(&after);
    let _ = take(&mut buffer);
    assert_eq!(buffer, after);
}

#[hegel::composite]
fn message(tc: &TestCase) -> CastMessage {
    CastMessage {
        protocol_version: tc.draw(gs::integers::<i32>()),
        source_id: tc.draw(gs::text().max_size(20)),
        destination_id: tc.draw(gs::text().max_size(20)),
        namespace: tc.draw(gs::text().max_size(40)),
        payload_type: tc.draw(gs::integers::<i32>()),
        payload_utf8: tc.draw(gs::optional(gs::text().max_size(200))),
        payload_binary: tc.draw(gs::optional(gs::binary().max_size(199))),
    }
}
