use hegel::TestCase;
use hegel::generators::{self as gs, Generator};

use super::*;
use crate::testing::json;

/// The keys a request has.
fn keys() -> Vec<String> {
    [
        "id", "method", "command", "args", "target", "window", "block",
    ]
    .into_iter()
    .chain([
        "scope",
        "positions",
        "urls",
        "type",
        "payload",
        "result",
        "error",
    ])
    .map(str::to_owned)
    .collect()
}

/// Any request a client might make.
#[hegel::composite]
fn request(tc: &TestCase) -> Request {
    let id = tc.draw(hegel::one_of!(
        gs::just(Value::Null),
        gs::integers::<u64>().map(Value::from)
    ));
    let call = hegel::compose!(|tc| {
        Ask::Call {
            command: tc.draw(gs::text()),
            args: tc.draw(json(keys())),
            target: Target {
                window: tc.draw(gs::optional(gs::text())),
                block: tc.draw(gs::optional(gs::text())),
            },
        }
    });
    let ask = tc.draw(hegel::one_of!(
        gs::just(Ask::Hello),
        gs::just(Ask::Commands),
        gs::just(Ask::Unsubscribe),
        gs::text().map(|command| Ask::Describe { command }),
        call,
        gs::vecs(gs::text())
            .max_size(2)
            .map(|urls| Ask::HandOver { urls }),
    ));
    Request { id, ask }
}

#[hegel::test]
fn any_line_is_a_request_or_says_why(tc: TestCase) {
    if let Err((_, why)) = read(&tc.draw(gs::text())) {
        assert!(!why.is_empty());
    }
}

#[hegel::test]
fn any_json_is_a_request_or_says_why_with_its_id(tc: TestCase) {
    let value = tc.draw(json(keys()));
    if let Err((id, why)) = read(&value.to_string()) {
        assert_eq!(id, value.get("id").cloned().unwrap_or_default());
        assert!(!why.is_empty());
    }
}

#[hegel::test]
fn a_request_is_read_as_written(tc: TestCase) {
    let request = tc.draw(request());
    let line = serde_json::to_string(&request).unwrap();
    assert_eq!(read(&line).unwrap(), request);
}

#[hegel::test]
fn any_line_a_client_hears_is_an_answer_an_event_or_why_not(tc: TestCase) {
    if let Err(why) = Incoming::read(&tc.draw(gs::text())) {
        assert!(!why.is_empty());
    }
}
