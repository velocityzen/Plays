use serde_json::json;

use super::route;
use crate::app::Route;
use crate::media::output::Playing;

#[test]
fn says_where_the_sound_goes() {
    let playing = Playing {
        id: None,
        name: "Speakers".into(),
        rate: 48_000,
        channels: 2,
    };
    let device = Route::Device {
        playing,
        missing: Some("Headphones".into()),
    };
    assert_eq!(
        route(Some(&device)),
        json!({"playing": {
            "device": "Speakers",
            "rate": 48_000,
            "channels": 2,
            "instead_of": "Headphones",
        }})
    );
    let waiting = Route::Waiting("Headphones".into());
    assert_eq!(route(Some(&waiting)), json!({"waiting": "Headphones"}));
    let opening = Route::Opening("Headphones".into());
    assert_eq!(route(Some(&opening)), json!({"opening": "Headphones"}));
    let unavailable = Route::Unavailable("no device".into());
    assert_eq!(
        route(Some(&unavailable)),
        json!({"unavailable": "no device"})
    );
    assert_eq!(route(None), json!({"unavailable": "there's no sound"}));
}
