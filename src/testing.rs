//! What the tests share: JSON of any shape, its keys drawn from `keys` or
//! any.

use hegel::TestCase;
use hegel::generators::{self as gs, Generator};
use serde_json::{Map, Number, Value};

/// Any JSON value, three deep at most, its objects' keys `keys` or any.
#[hegel::composite]
pub fn json(tc: &TestCase, keys: Vec<String>) -> Value {
    let leaf = hegel::one_of!(
        gs::just(Value::Null),
        gs::booleans().map(Value::from),
        gs::integers::<i64>().map(Value::from),
        // JSON has no NaN, and a number reads back as written.
        gs::floats::<f64>()
            .filter(|&n| Number::from_f64(n).is_some_and(|number| reads_back(&number)))
            .map(|n| Value::Number(Number::from_f64(n).unwrap())),
        gs::text().map(Value::from),
    );
    let value = gs::recursive(leaf, move |inner| {
        hegel::one_of!(
            gs::vecs(inner.clone()).max_size(3).map(Value::Array),
            gs::vecs(hegel::tuples!(key(keys.clone()), inner))
                .max_size(5)
                .map(|fields| Value::Object(fields.into_iter().collect::<Map<_, _>>())),
        )
    })
    .max_depth(3)
    .max_leaves(32);
    tc.draw(value)
}

/// Whether `number` reads back from its text as itself.
fn reads_back(number: &Number) -> bool {
    number.to_string().parse::<Number>().ok().as_ref() == Some(number)
}

/// One of `keys`, or any.
#[hegel::composite]
pub fn key(tc: &TestCase, keys: Vec<String>) -> String {
    if !keys.is_empty() && tc.draw(gs::weighted_booleans(0.75)) {
        tc.draw(gs::sampled_from(keys))
    } else {
        tc.draw(gs::text())
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
