use proptest::prelude::*;

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
fn request() -> impl Strategy<Value = Request> {
    let id = prop_oneof![Just(Value::Null), any::<u64>().prop_map(Value::from)];
    let target = (any::<Option<String>>(), any::<Option<String>>())
        .prop_map(|(window, block)| Target { window, block });
    let ask = prop_oneof![
        Just(Ask::Hello),
        Just(Ask::Commands),
        Just(Ask::Unsubscribe),
        any::<String>().prop_map(|command| Ask::Describe { command }),
        (any::<String>(), json(keys()), target).prop_map(|(command, args, target)| Ask::Call {
            command,
            args,
            target,
        }),
        prop::collection::vec(any::<String>(), 0..3).prop_map(|urls| Ask::HandOver { urls }),
    ];
    (id, ask).prop_map(|(id, ask)| Request { id, ask })
}

proptest! {
    #[test]
    fn any_line_is_a_request_or_says_why(line in any::<String>()) {
        if let Err((_, why)) = read(&line) {
            prop_assert!(!why.is_empty());
        }
    }

    #[test]
    fn any_json_is_a_request_or_says_why_with_its_id(value in json(keys())) {
        if let Err((id, why)) = read(&value.to_string()) {
            prop_assert_eq!(id, value.get("id").cloned().unwrap_or_default());
            prop_assert!(!why.is_empty());
        }
    }

    #[test]
    fn a_request_is_read_as_written(request in request()) {
        let line = serde_json::to_string(&request).unwrap();
        prop_assert_eq!(read(&line).unwrap(), request);
    }

    #[test]
    fn any_line_a_client_hears_is_an_answer_an_event_or_why_not(line in any::<String>()) {
        if let Err(why) = Incoming::read(&line) {
            prop_assert!(!why.is_empty());
        }
    }
}
