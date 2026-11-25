use super::*;

#[test]
fn a_time_is_hours_minutes_and_seconds_or_fewer() {
    assert_eq!(seconds("1:23:45"), Some(5025.0));
    assert_eq!(seconds("23:45"), Some(1425.0));
    assert_eq!(seconds("90"), Some(90.0));
    assert_eq!(seconds("1:02.5"), Some(62.5));
    assert_eq!(seconds("0"), Some(0.0));
    assert_eq!(
        seconds("120:00"),
        Some(7200.0),
        "minutes past an hour first"
    );
}

#[test]
fn a_time_with_more_than_its_parts_take_isnt_one() {
    for typed in [
        "", "1:60", "1:2:3:4", "a", "-5", "1:-5", "1.5:00", "inf", "NaN", "1::2",
    ] {
        assert_eq!(seconds(typed), None, "{typed:?}");
    }
}

fn arg(kind: Kind, required: bool) -> &'static Arg {
    Box::leak(Box::new(if required {
        Arg::required("level", kind, "How loud, from 0 to 100.")
    } else {
        Arg::optional("level", kind, "How loud, from 0 to 100.")
    }))
}

#[test]
fn what_is_typed_is_read_as_its_kind_says_and_checked() {
    let number = arg(
        Kind::Number {
            min: 0.0,
            max: Some(100.0),
        },
        true,
    );
    assert_eq!(read_typed(number, " 50 "), Ok(json!(50.0)));
    assert_eq!(
        read_typed(number, "150"),
        Err("Level is at most 100".to_owned())
    );
    assert_eq!(read_typed(number, "loud"), Err("Type a number".to_owned()));
    assert_eq!(
        read_typed(number, ""),
        Err("Type how loud, from 0 to 100".to_owned())
    );
    let whole = arg(Kind::Integer { min: 1 }, true);
    assert_eq!(read_typed(whole, "3"), Ok(json!(3)));
    assert_eq!(
        read_typed(whole, "0"),
        Err("Level is at least 1".to_owned())
    );
    assert_eq!(
        read_typed(whole, "2.5"),
        Err("Type a whole number".to_owned())
    );
    assert_eq!(
        read_typed(arg(Kind::Text, true), "Movies"),
        Ok(json!("Movies"))
    );
    assert_eq!(
        read_typed(arg(Kind::Texts, true), "/a b"),
        Ok(json!(["/a b"]))
    );
    let panels = arg(Kind::Choices(|| vec!["bars", "notes"]), true);
    assert_eq!(
        read_typed(panels, "notes, bars"),
        Ok(json!(["notes", "bars"]))
    );
    assert_eq!(
        read_typed(panels, "notes bars"),
        Ok(json!(["notes", "bars"]))
    );
    assert_eq!(
        read_typed(panels, "reel"),
        Err("Level is a list of any of bars, notes, none too".to_owned())
    );
    let toggle = arg(Kind::Toggle, true);
    assert_eq!(read_typed(toggle, "Off"), Ok(json!(false)));
    assert_eq!(read_typed(toggle, "true"), Ok(json!(true)));
    assert_eq!(
        read_typed(toggle, "maybe"),
        Err("Type true or false".to_owned())
    );
    assert_eq!(read_typed(arg(Kind::Any, true), "0.5"), Ok(json!(0.5)));
    assert_eq!(read_typed(arg(Kind::Any, true), "dark"), Ok(json!("dark")));
}
