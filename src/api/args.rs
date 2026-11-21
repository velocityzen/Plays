//! A command's arguments: what each is, for `describe` to say and for the
//! arguments given to be checked against before the command runs, then
//! read into the command's own type.

use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};

use super::Error;

/// One of a command's arguments.
pub struct Arg {
    pub name: &'static str,
    pub kind: Kind,
    pub required: bool,
    pub description: &'static str,
}

/// What an argument's value is.
pub enum Kind {
    /// A whole number, at least `min`.
    Integer { min: i64 },
    /// Any text.
    Text,
    /// One of these words.
    Choice(fn() -> Vec<&'static str>),
}

impl Arg {
    pub const fn required(name: &'static str, kind: Kind, description: &'static str) -> Self {
        Self {
            name,
            kind,
            required: true,
            description,
        }
    }

    pub const fn optional(name: &'static str, kind: Kind, description: &'static str) -> Self {
        Self {
            name,
            kind,
            required: false,
            description,
        }
    }

    /// As `describe` says it.
    pub fn describe(&self) -> Value {
        let mut described = json!({
            "name": self.name,
            "required": self.required,
            "description": self.description,
        });
        let fields = described.as_object_mut().expect("an object");
        match &self.kind {
            Kind::Integer { min } => {
                fields.insert("type".into(), "integer".into());
                fields.insert("min".into(), (*min).into());
            }
            Kind::Text => {
                fields.insert("type".into(), "text".into());
            }
            Kind::Choice(choices) => {
                fields.insert("type".into(), "choice".into());
                fields.insert("choices".into(), choices().into());
            }
        }
        described
    }

    /// Whether `value` is one of its values; null is none, for one not
    /// required.
    fn check(&self, value: &Value) -> Result<(), String> {
        let name = self.name;
        match (&self.kind, value) {
            (_, Value::Null) if !self.required => Ok(()),
            (Kind::Integer { min }, Value::Number(number)) => match number.as_i64() {
                Some(n) if n >= *min => Ok(()),
                Some(_) => Err(format!("{name} is at least {min}")),
                None => Err(format!("{name} is a whole number")),
            },
            (Kind::Integer { .. }, _) => Err(format!("{name} is a whole number")),
            (Kind::Text, Value::String(_)) => Ok(()),
            (Kind::Text, _) => Err(format!("{name} is text")),
            (Kind::Choice(choices), value) => {
                let choices = choices();
                match value.as_str() {
                    Some(word) if choices.contains(&word) => Ok(()),
                    _ => Err(format!("{name} is one of {}", choices.join(", "))),
                }
            }
        }
    }
}

/// Check `args` against what `command` takes, `takes`: an object of them,
/// or null for none, with none it doesn't take, none missing that it
/// needs, and each a value it takes.
pub fn check(command: &str, takes: &[Arg], args: &Value) -> Result<(), Error> {
    let empty = Map::new();
    let given = match args {
        Value::Null => &empty,
        Value::Object(given) => given,
        _ => {
            return Err(Error::bad_args(
                command,
                "its arguments are an object of them",
            ));
        }
    };
    for name in given.keys() {
        if !takes.iter().any(|arg| arg.name == name) {
            let names: Vec<&str> = takes.iter().map(|arg| arg.name).collect();
            let message = match names[..] {
                [] => format!("takes no {name}: it takes no arguments"),
                _ => format!("takes no {name}: it takes {}", names.join(", ")),
            };
            return Err(Error::bad_args(command, message));
        }
    }
    for arg in takes {
        match given.get(arg.name) {
            None if arg.required => {
                return Err(Error::bad_args(command, format!("needs {}", arg.name)));
            }
            None => {}
            Some(value) => arg
                .check(value)
                .map_err(|message| Error::bad_args(command, message))?,
        }
    }
    Ok(())
}

/// `args`, checked already, as `T`.
pub fn read<T: DeserializeOwned>(command: &str, args: &Value) -> Result<T, Error> {
    let args = match args {
        Value::Null => Value::Object(Map::new()),
        args => args.clone(),
    };
    serde_json::from_value(args).map_err(|err| Error::bad_args(command, err.to_string()))
}

#[cfg(test)]
mod tests {
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
}
