use super::*;

#[test]
fn reads_each_request_with_what_it_leaves_out_at_its_default() {
    let cases = [
        (r#"{"id": 1, "method": "hello"}"#, json!(1), Ask::Hello),
        (
            r#"{"id": "a", "method": "call", "command": "playback.seek", "args": {"seconds": 30}, "target": {"window": "2"}}"#,
            json!("a"),
            Ask::Call {
                command: "playback.seek".into(),
                args: json!({"seconds": 30}),
                target: Target {
                    window: Some("2".into()),
                    block: None,
                },
            },
        ),
        (
            r#"{"method": "call", "command": "app.quit"}"#,
            Value::Null,
            Ask::Call {
                command: "app.quit".into(),
                args: Value::Null,
                target: Target::default(),
            },
        ),
        (
            r#"{"id": 2, "method": "commands"}"#,
            json!(2),
            Ask::Commands,
        ),
        (
            r#"{"id": 3, "method": "describe", "command": "window.ls"}"#,
            json!(3),
            Ask::Describe {
                command: "window.ls".into(),
            },
        ),
        (
            r#"{"id": 4, "method": "subscribe"}"#,
            json!(4),
            Ask::Subscribe {
                scope: Heard::All,
                positions: false,
            },
        ),
        (
            r#"{"id": 5, "method": "subscribe", "scope": "app", "positions": true}"#,
            json!(5),
            Ask::Subscribe {
                scope: Heard::App,
                positions: true,
            },
        ),
        (
            r#"{"id": 6, "method": "subscribe", "scope": {"window": "2"}}"#,
            json!(6),
            Ask::Subscribe {
                scope: Heard::Window("2".into()),
                positions: false,
            },
        ),
        (
            r#"{"id": 7, "method": "unsubscribe"}"#,
            json!(7),
            Ask::Unsubscribe,
        ),
        (
            r#"{"id": 8, "method": "hand_over", "urls": ["/a.mkv"]}"#,
            json!(8),
            Ask::HandOver {
                urls: vec!["/a.mkv".into()],
            },
        ),
        (
            r#"{"id": 9, "method": "hand_over", "later": "ignored"}"#,
            json!(9),
            Ask::HandOver { urls: Vec::new() },
        ),
    ];
    for (line, id, ask) in cases {
        let request = read(line).unwrap_or_else(|why| panic!("{line}: {why:?}"));
        assert_eq!(request, Request { id, ask }, "{line}");
        // As a client writes it.
        let written = serde_json::to_string(&request).unwrap();
        assert_eq!(read(&written).unwrap(), request, "{written}");
    }
}

#[test]
fn says_why_a_request_cant_be_read_with_its_id_if_it_has_one() {
    let (id, why) = read("not json").unwrap_err();
    assert_eq!(id, Value::Null);
    assert!(!why.is_empty());
    let (id, why) = read(r#"{"id": 3, "method": "dance"}"#).unwrap_err();
    assert_eq!(id, json!(3));
    assert!(why.contains("dance"), "{why}");
    let (id, why) = read(r#"{"id": 4, "method": "call"}"#).unwrap_err();
    assert_eq!(id, json!(4));
    assert!(why.contains("command"), "{why}");
}

#[test]
fn answers_with_the_result_or_the_error_and_what_it_says() {
    assert_eq!(
        answer(&json!(1), Ok(json!({"window": "3fa9c2d1"}))),
        json!({"id": 1, "result": {"window": "3fa9c2d1"}})
    );
    assert_eq!(
        answer(&json!("x"), Ok(Value::Null)),
        json!({"id": "x", "result": null})
    );
    assert_eq!(
        answer(&json!(2), Err(Error::not_found("window", "9"))),
        json!({
            "id": 2,
            "error": "not_found",
            "what": "window",
            "name": "9",
            "message": "no window 9",
        })
    );
    assert_eq!(
        answer(&json!(3), Err(Error::bad_args("app.open", "needs urls"))),
        json!({
            "id": 3,
            "error": "bad_args",
            "command": "app.open",
            "message": "needs urls",
        })
    );
    assert_eq!(
        bad_request(&json!(3), "no method"),
        json!({"id": 3, "error": "bad_request", "message": "no method"})
    );
}

#[test]
fn a_client_reads_answers_errors_and_events_back() {
    let cases = [
        (
            answer(&json!(1), Ok(json!([1, 2]))),
            Incoming::Answer {
                id: json!(1),
                outcome: Ok(json!([1, 2])),
            },
        ),
        (
            answer(&json!(2), Err(Error::bad_args("a.b", "needs urls"))),
            Incoming::Answer {
                id: json!(2),
                outcome: Err(Refusal::Command(Error::bad_args("a.b", "needs urls"))),
            },
        ),
        (
            bad_request(&json!(3), "no method"),
            Incoming::Answer {
                id: json!(3),
                outcome: Err(Refusal::BadRequest("no method".into())),
            },
        ),
        (
            // An error a later Plays has, which this one doesn't know.
            json!({"id": 4, "error": "busy", "message": "try later"}),
            Incoming::Answer {
                id: json!(4),
                outcome: Err(Refusal::BadRequest("try later".into())),
            },
        ),
        (
            json!({"type": "playing", "payload": {"block_id": "1"}}),
            Incoming::Event {
                kind: "playing".into(),
                payload: json!({"block_id": "1"}),
            },
        ),
    ];
    for (line, incoming) in cases {
        assert_eq!(Incoming::read(&line.to_string()), Ok(incoming), "{line}");
    }
    assert!(Incoming::read("[]").is_err());
    assert!(Incoming::read("{").is_err());
}
