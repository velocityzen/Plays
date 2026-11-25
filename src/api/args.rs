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
    /// A number, fractions too, from `min` to `max` if there is one.
    Number { min: f64, max: Option<f64> },
    /// Any text.
    Text,
    /// A list of text, at least one.
    Texts,
    /// One of these words.
    Choice(fn() -> Vec<&'static str>),
    /// A list of these words, none too.
    Choices(fn() -> Vec<&'static str>),
    /// Any value, null too, which the command checks itself.
    Any,
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
            Kind::Number { min, max } => {
                fields.insert("type".into(), "number".into());
                fields.insert("min".into(), (*min).into());
                if let Some(max) = max {
                    fields.insert("max".into(), (*max).into());
                }
            }
            Kind::Text => {
                fields.insert("type".into(), "text".into());
            }
            Kind::Texts => {
                fields.insert("type".into(), "texts".into());
            }
            Kind::Choice(choices) => {
                fields.insert("type".into(), "choice".into());
                fields.insert("choices".into(), choices().into());
            }
            Kind::Choices(choices) => {
                fields.insert("type".into(), "choices".into());
                fields.insert("choices".into(), choices().into());
            }
            Kind::Any => {
                fields.insert("type".into(), "any".into());
            }
        }
        described
    }

    /// Whether `value` is one of its values; null is none, for one not
    /// required.
    pub fn check(&self, value: &Value) -> Result<(), String> {
        let name = self.name;
        match (&self.kind, value) {
            (_, Value::Null) if !self.required => Ok(()),
            (Kind::Integer { min }, Value::Number(number)) => match number.as_i64() {
                Some(n) if n >= *min => Ok(()),
                Some(_) => Err(format!("{name} is at least {min}")),
                None => Err(format!("{name} is a whole number")),
            },
            (Kind::Integer { .. }, _) => Err(format!("{name} is a whole number")),
            (Kind::Number { min, max }, Value::Number(number)) => {
                let n = number.as_f64().unwrap_or(f64::NAN);
                match max {
                    _ if n < *min => Err(format!("{name} is at least {min}")),
                    Some(max) if n > *max => Err(format!("{name} is at most {max}")),
                    _ => Ok(()),
                }
            }
            (Kind::Number { .. }, _) => Err(format!("{name} is a number")),
            (Kind::Text, Value::String(_)) => Ok(()),
            (Kind::Text, _) => Err(format!("{name} is text")),
            (Kind::Texts, Value::Array(items))
                if !items.is_empty() && items.iter().all(Value::is_string) =>
            {
                Ok(())
            }
            (Kind::Texts, _) => Err(format!("{name} is a list of text, at least one")),
            (Kind::Any, _) => Ok(()),
            (Kind::Choice(choices), value) => {
                let choices = choices();
                match value.as_str() {
                    Some(word) if choices.contains(&word) => Ok(()),
                    _ => Err(format!("{name} is one of {}", choices.join(", "))),
                }
            }
            (Kind::Choices(choices), value) => {
                let choices = choices();
                let listed = match value {
                    Value::Array(items) => items
                        .iter()
                        .all(|item| item.as_str().is_some_and(|word| choices.contains(&word))),
                    // Lua's empty table.
                    Value::Object(fields) => fields.is_empty(),
                    _ => false,
                };
                if listed {
                    Ok(())
                } else {
                    Err(format!(
                        "{name} is a list of any of {}, none too",
                        choices.join(", ")
                    ))
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

/// A list of words, as `Kind::Choices` takes it, for a command's own type
/// to read with `#[serde(deserialize_with = "args::words")]`: an empty
/// object, Lua's empty table, is none.
pub fn words<'de, D: serde::Deserializer<'de>>(given: D) -> Result<Vec<String>, D::Error> {
    #[derive(serde::Deserialize)]
    #[serde(untagged)]
    enum Words {
        Listed(Vec<String>),
        Empty {},
    }
    Ok(match serde::Deserialize::deserialize(given)? {
        Words::Listed(words) => words,
        Words::Empty {} => Vec::new(),
    })
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
#[path = "tests/args.rs"]
mod tests;
