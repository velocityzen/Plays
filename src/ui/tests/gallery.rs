use gpui::{TestAppContext, VisualTestContext};
use serde_json::Value;

use super::*;
use crate::api::{self, Invocation};
use crate::ui::testing;

#[gpui::test]
fn opens_once_from_its_command_and_draws_every_size(cx: &mut TestAppContext) {
    let (_, cx) = testing::start(cx);
    for _ in 0..2 {
        let done = TestAppContext::update(&cx, |cx| {
            api::run(Invocation::new("app.show_components"), cx)
        });
        cx.run_until_parked();
        assert_eq!(done, Ok(Value::Null));
    }
    let galleries: Vec<_> = TestAppContext::update(&cx, |cx| {
        cx.windows()
            .into_iter()
            .filter_map(|window| window.downcast::<GalleryView>())
            .collect()
    });
    assert_eq!(galleries.len(), 1, "brought to the front, not opened again");

    let mut gallery = VisualTestContext::from_window(galleries[0].into(), &cx);
    for button in [
        "normal-small",
        "normal-selected-medium",
        "primary-disabled-large",
    ] {
        testing::find(&mut gallery, button);
    }
}
