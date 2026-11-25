use serde::Deserialize;

use super::*;

const TAKES: [Arg; 3] = [
    Arg::required("number", Kind::Integer { min: 1 }, "Which."),
    Arg::optional("name", Kind::Text, "What it's called."),
    Arg::optional(
        "shape",
        Kind::Choice(|| vec!["round", "square"]),
        "Its shape.",
    ),
];

fn checked(args: Value) -> Result<(), Error> {
    check("go", &TAKES, &args)
}

fn bad(message: &str) -> Result<(), Error> {
    Err(Error::bad_args("go", message))
}

#[test]
fn takes_the_arguments_it_has() {
    assert_eq!(checked(json!({"number": 2})), Ok(()));
    assert_eq!(
        checked(json!({"number": 1, "name": "x", "shape": "round"})),
        Ok(())
    );
    assert_eq!(
        checked(json!({"number": 1, "name": null, "shape": null})),
        Ok(()),
        "null is none, for one not needed"
    );
    assert_eq!(check("go", &[], &Value::Null), Ok(()), "none");
    assert_eq!(check("go", &[], &json!({})), Ok(()));
}

#[test]
fn turns_away_others() {
    assert_eq!(checked(json!({})), bad("needs number"));
    assert_eq!(checked(Value::Null), bad("needs number"));
    assert_eq!(
        checked(json!({"number": 1, "size": 2})),
        bad("takes no size: it takes number, name, shape")
    );
    assert_eq!(
        check("go", &[], &json!({"size": 2})),
        bad("takes no size: it takes no arguments")
    );
    assert_eq!(
        checked(json!([1])),
        bad("its arguments are an object of them")
    );
    assert_eq!(checked(json!({"number": 0})), bad("number is at least 1"));
    assert_eq!(
        checked(json!({"number": 1.5})),
        bad("number is a whole number")
    );
    assert_eq!(
        checked(json!({"number": "2"})),
        bad("number is a whole number")
    );
    assert_eq!(
        checked(json!({"number": null})),
        bad("number is a whole number")
    );
    assert_eq!(
        checked(json!({"number": 1, "name": 3})),
        bad("name is text")
    );
    assert_eq!(
        checked(json!({"number": 1, "shape": "oval"})),
        bad("shape is one of round, square")
    );
}

#[test]
fn takes_numbers_and_lists_of_text() {
    let takes = [
        Arg::optional(
            "at",
            Kind::Number {
                min: 0.0,
                max: None,
            },
            "When.",
        ),
        Arg::optional(
            "level",
            Kind::Number {
                min: 0.0,
                max: Some(100.0),
            },
            "How loud.",
        ),
        Arg::optional("urls", Kind::Texts, "What."),
    ];
    let checked = |args: Value| check("go", &takes, &args);
    assert_eq!(checked(json!({"at": 12.5, "level": 100})), Ok(()));
    assert_eq!(checked(json!({"at": 0, "level": 0.5})), Ok(()));
    assert_eq!(checked(json!({"at": -0.5})), bad("at is at least 0"));
    assert_eq!(
        checked(json!({"level": 100.5})),
        bad("level is at most 100")
    );
    assert_eq!(checked(json!({"at": "1:00"})), bad("at is a number"));
    assert_eq!(checked(json!({"urls": ["a", "b"]})), Ok(()));
    for urls in [json!([]), json!("a"), json!(["a", 1])] {
        assert_eq!(
            checked(json!({ "urls": urls })),
            bad("urls is a list of text, at least one"),
            "{urls}"
        );
    }
    assert_eq!(
        takes[1].describe(),
        json!({"name": "level", "type": "number", "min": 0.0, "max": 100.0, "required": false, "description": "How loud."})
    );
    assert_eq!(takes[0].describe()["max"], Value::Null, "none");
    assert_eq!(takes[2].describe()["type"], "texts");
}

#[test]
fn takes_lists_of_words_none_too() {
    let takes = [Arg::required(
        "visible",
        Kind::Choices(|| vec!["controls", "notes"]),
        "Which.",
    )];
    let checked = |args: Value| check("go", &takes, &args);
    for visible in [json!(["notes", "controls"]), json!(["notes"]), json!([])] {
        assert_eq!(checked(json!({ "visible": visible })), Ok(()), "{visible}");
    }
    assert_eq!(
        checked(json!({"visible": {}})),
        Ok(()),
        "none, as Lua's empty table is"
    );
    for visible in [
        json!(["notes", "bars"]),
        json!("notes"),
        json!({"notes": true}),
        json!([1]),
    ] {
        assert_eq!(
            checked(json!({ "visible": visible })),
            bad("visible is a list of any of controls, notes, none too"),
            "{visible}"
        );
    }
    assert_eq!(
        takes[0].describe(),
        json!({"name": "visible", "type": "choices", "choices": ["controls", "notes"], "required": true, "description": "Which."})
    );

    #[derive(Deserialize)]
    struct Visible {
        #[serde(deserialize_with = "words")]
        visible: Vec<String>,
    }
    let read = |args: Value| read::<Visible>("go", &args).unwrap().visible;
    assert_eq!(read(json!({"visible": ["notes"]})), ["notes"]);
    assert_eq!(read(json!({"visible": {}})), [] as [String; 0]);
    assert_eq!(read(json!({"visible": []})), [] as [String; 0]);
}

#[test]
fn takes_any_value_null_too_for_the_command_to_check() {
    let takes = [Arg::required("value", Kind::Any, "What to set.")];
    let checked = |args: Value| check("go", &takes, &args);
    for value in [
        json!(null),
        json!(true),
        json!(0.5),
        json!("dark"),
        json!({"id": "a"}),
    ] {
        assert_eq!(checked(json!({ "value": value })), Ok(()), "{value}");
    }
    assert_eq!(checked(json!({})), bad("needs value"), "but there");
    assert_eq!(
        takes[0].describe(),
        json!({"name": "value", "type": "any", "required": true, "description": "What to set."})
    );
}

#[test]
fn reads_them_into_the_commands_own_type() {
    #[derive(Deserialize, Debug, PartialEq)]
    struct Go {
        number: usize,
        name: Option<String>,
    }
    assert_eq!(
        read::<Go>("go", &json!({"number": 3})),
        Ok(Go {
            number: 3,
            name: None
        })
    );
    #[derive(Deserialize, Debug, PartialEq)]
    struct Nothing {}
    assert_eq!(read::<Nothing>("go", &Value::Null), Ok(Nothing {}));
    assert!(matches!(
        read::<Go>("go", &json!({})),
        Err(Error::BadArgs { .. })
    ));
}

#[test]
fn describes_each() {
    assert_eq!(
        TAKES[0].describe(),
        json!({"name": "number", "type": "integer", "min": 1, "required": true, "description": "Which."})
    );
    assert_eq!(
        TAKES[1].describe(),
        json!({"name": "name", "type": "text", "required": false, "description": "What it's called."})
    );
    assert_eq!(
        TAKES[2].describe(),
        json!({"name": "shape", "type": "choice", "choices": ["round", "square"], "required": false, "description": "Its shape."})
    );
}
