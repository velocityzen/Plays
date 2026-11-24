//! What the tests share: JSON of any shape, its keys drawn from `keys` or
//! any.

use proptest::prelude::*;
use serde_json::{Map, Number, Value};

/// Any JSON value, three deep at most, its objects' keys `keys` or any.
pub fn json(keys: Vec<String>) -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::from),
        any::<i64>().prop_map(Value::from),
        any::<f64>().prop_filter_map("JSON has no NaN, and reads back", |n| {
            Number::from_f64(n)
                .filter(|number| number.to_string().parse::<Number>().ok().as_ref() == Some(number))
                .map(Value::Number)
        }),
        any::<String>().prop_map(Value::from),
    ];
    leaf.prop_recursive(3, 32, 4, move |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..4).prop_map(Value::Array),
            prop::collection::vec((key(keys.clone()), inner), 0..6)
                .prop_map(|fields| Value::Object(fields.into_iter().collect::<Map<_, _>>())),
        ]
    })
}

/// One of `keys`, or any.
pub fn key(keys: Vec<String>) -> impl Strategy<Value = String> {
    if keys.is_empty() {
        any::<String>().boxed()
    } else {
        prop_oneof![3 => prop::sample::select(keys), 1 => any::<String>()].boxed()
    }
}

/// Every key in `value`, however deep.
pub fn keys_in(value: &Value) -> Vec<String> {
    let mut keys = Vec::new();
    let mut left = vec![value];
    while let Some(value) = left.pop() {
        if let Value::Object(fields) = value {
            keys.extend(fields.keys().cloned());
            left.extend(fields.values());
        }
    }
    keys.sort();
    keys.dedup();
    keys
}
