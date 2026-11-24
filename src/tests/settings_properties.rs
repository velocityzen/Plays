use proptest::prelude::*;

use super::*;
use crate::testing::{json, key, keys_in};

/// Fields a settings file might hold: Plays' own, and others, of any value.
fn fields() -> impl Strategy<Value = Map<String, Value>> {
    let known = keys_in(&serde_json::to_value(Settings::default()).unwrap());
    prop::collection::vec((key(known.clone()), json(known)), 0..8)
        .prop_map(|fields| fields.into_iter().collect())
}

proptest! {
    #[test]
    fn any_fields_read_leniently_to_settings_that_read_back_whole(fields in fields()) {
        let given: Vec<String> = fields.keys().cloned().collect();
        let (settings, unread) = leniently::<Settings>(fields);
        for at in &unread {
            let top = at.split('.').next().unwrap();
            prop_assert!(given.iter().any(|key| key == top), "{at}");
        }
        let Value::Object(written) = serde_json::to_value(&settings).unwrap() else {
            panic!("settings are written as an object");
        };
        let (again, unread) = leniently::<Settings>(written);
        prop_assert!(unread.is_empty(), "{unread:?}");
        prop_assert_eq!(again, settings);
    }

    #[test]
    fn any_text_imports_or_says_why(text in any::<String>()) {
        let _ = Settings::import(&text);
    }

    #[test]
    fn any_json_imports_or_says_why(fields in fields()) {
        let _ = Settings::import(&Value::Object(fields).to_string());
    }
}
