use serde_json::json;

use super::*;

fn printed(print: impl FnOnce(&mut dyn Write) -> io::Result<()>) -> String {
    let mut out = Vec::new();
    print(&mut out).unwrap();
    String::from_utf8(out).unwrap()
}

#[test]
fn a_table_lines_up_its_columns() {
    let rows = [
        vec!["*".into(), "1".into(), "Living room".into()],
        vec![String::new(), "12".into(), "-".into()],
    ];
    assert_eq!(
        printed(|out| table(out, &["", "#", "LABEL"], &rows)),
        "   #   LABEL\n*  1   Living room\n   12  -\n"
    );
}

#[test]
fn keys_line_up_by_the_characters_shown_not_their_bytes() {
    let rows = [
        vec!["⇧⌘O".into(), "app.open".into()],
        vec!["Space".into(), "block.playback.play_pause".into()],
    ];
    assert_eq!(
        printed(|out| table(out, &["KEYS", "COMMAND"], &rows)),
        "KEYS   COMMAND\n⇧⌘O    app.open\nSpace  block.playback.play_pause\n"
    );
}

#[test]
fn results_print_as_text_or_json() {
    let print = |value: Value, json: bool| printed(|out| result(out, &value, json));
    assert_eq!(print(json!("1a2b3c4d"), false), "1a2b3c4d\n");
    assert_eq!(print(json!(0.5), false), "0.5\n");
    assert_eq!(print(Value::Null, false), "", "nothing for nothing");
    assert_eq!(print(Value::Null, true), "null\n");
    assert_eq!(print(json!("dark"), true), "\"dark\"\n");
    assert_eq!(
        print(json!({"window": "1a"}), false),
        "{\n  \"window\": \"1a\"\n}\n"
    );
}

#[test]
fn events_print_a_line_each() {
    let payload = json!({"window_id": "1a", "block_id": "2b", "position": 1.5});
    assert_eq!(
        printed(|out| event(out, "block.playback.paused", &payload, false)),
        "block.playback.paused window_id=1a block_id=2b position=1.5\n"
    );
    assert_eq!(
        printed(|out| event(out, "block.playback.paused", &payload, true)),
        "{\"type\":\"block.playback.paused\",\"payload\":{\"window_id\":\"1a\",\"block_id\":\"2b\",\"position\":1.5}}\n"
    );
}
