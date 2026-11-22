use super::*;

#[test]
fn each_kind_has_its_exit_status_and_says_what_went_wrong() {
    let cases = [
        (
            Error::NoSuchCommand {
                command: "dance".into(),
            },
            1,
            "no command dance",
        ),
        (
            Error::bad_args("playback.go_to_chapter", "needs number"),
            2,
            "playback.go_to_chapter: needs number",
        ),
        (Error::not_found("window", "9"), 3, "no window 9"),
        (
            Error::Ambiguous {
                what: "block".into(),
                name: "a1".into(),
                matches: vec!["0000ffa1".into(), "1234aaa1".into()],
            },
            3,
            "a1 could be the block 0000ffa1 or 1234aaa1",
        ),
        (Error::failed("the window closed"), 1, "the window closed"),
        (
            Error::command_line_off(),
            5,
            "control from the command line is off: turn it on in Settings → Command Line",
        ),
    ];
    for (error, status, message) in cases {
        assert_eq!(error.exit_status(), status, "{error:?}");
        assert_eq!(error.to_string(), message);
    }
    assert!(Error::not_found("block", "x").is_not_found());
    assert!(!Error::failed("x").is_not_found());
}

#[test]
fn says_which_kind_it_is_as_json() {
    let json = serde_json::to_value(Error::not_found("window", "9")).unwrap();
    assert_eq!(
        json,
        serde_json::json!({"error": "not_found", "what": "window", "name": "9"})
    );
    assert_eq!(
        serde_json::from_value::<Error>(json).unwrap(),
        Error::not_found("window", "9")
    );
    let off = serde_json::to_value(Error::command_line_off()).unwrap();
    assert_eq!(off["error"], "not_allowed");
    assert_eq!(
        serde_json::from_value::<Error>(off).unwrap(),
        Error::command_line_off()
    );
}
