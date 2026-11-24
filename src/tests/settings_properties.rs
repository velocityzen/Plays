use hegel::TestCase;
use hegel::generators as gs;

use super::*;
use crate::testing::{json, key, keys_in};

/// Fields a settings file might hold: Plays' own, and others, of any value.
#[hegel::composite]
fn fields(tc: &TestCase) -> Map<String, Value> {
    let known = keys_in(&serde_json::to_value(Settings::default()).unwrap());
    let fields = tc.draw(gs::vecs(hegel::tuples!(key(known.clone()), json(known))).max_size(7));
    fields.into_iter().collect()
}

#[hegel::test]
#[hegel::explicit_test_case(
    fields = serde_json::json!({ "hdr_gamma": 1.3719949776039003e293 }).as_object().unwrap().clone(),
)]
fn any_fields_read_leniently_to_settings_that_read_back_whole(tc: TestCase) {
    let fields = tc.draw(fields());
    let given: Vec<String> = fields.keys().cloned().collect();
    let (settings, unread) = leniently::<Settings>(fields);
    for at in &unread {
        let top = at.split('.').next().unwrap();
        assert!(given.iter().any(|key| key == top), "{at}");
    }
    let Value::Object(written) = serde_json::to_value(&settings).unwrap() else {
        panic!("settings are written as an object");
    };
    let (again, unread) = leniently::<Settings>(written);
    assert!(unread.is_empty(), "{unread:?}");
    assert_eq!(again, settings);
}

#[hegel::test]
fn any_text_imports_or_says_why(tc: TestCase) {
    let _ = Settings::import(&tc.draw(gs::text()));
}

#[hegel::test]
fn any_json_imports_or_says_why(tc: TestCase) {
    let _ = Settings::import(&Value::Object(tc.draw(fields())).to_string());
}
