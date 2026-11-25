use futures::channel::mpsc::UnboundedReceiver;
use gpui::TestAppContext;
use serde_json::{Value, json};

use super::*;
use crate::api::{self, Invocation, Source};
use crate::app;

/// What `listener` has been told since last asked: each event's type and
/// payload.
pub fn heard(listener: &mut UnboundedReceiver<Event>) -> Vec<(&'static str, Value)> {
    std::iter::from_fn(|| listener.try_recv().ok())
        .map(|event| (event.kind, event.payload))
        .collect()
}

#[gpui::test]
fn settings_are_told_by_key_and_shortcuts_by_command(cx: &mut TestAppContext) {
    cx.update(app::set_up_for_tests);
    let mut app = cx.update(|cx| listen(Scope::App, false, cx));
    let mut all = cx.update(|cx| listen(Scope::All, false, cx));

    cx.update(|cx| {
        Settings::update(cx, false, |settings| {
            settings.autoplay = !settings.autoplay;
            settings.subtitles.size += 1.0;
        });
    });
    cx.run_until_parked();
    let changed = [(
        "app.settings.changed",
        json!({"keys": ["autoplay", "subtitles.size"]}),
    )];
    assert_eq!(heard(&mut app), changed);
    assert_eq!(heard(&mut all), changed);

    cx.update(|cx| {
        Settings::update(cx, false, |settings| {
            settings.hotkeys.add("playback.play_pause", "ctrl-p");
        });
    });
    cx.run_until_parked();
    assert_eq!(
        heard(&mut app),
        [
            ("app.settings.changed", json!({"keys": ["hotkeys"]})),
            (
                "app.settings.keymap_changed",
                json!({"commands": ["playback.play_pause"]})
            ),
        ]
    );

    // Set as it was: nothing changed.
    cx.update(|cx| Settings::update(cx, false, |_| {}));
    cx.run_until_parked();
    assert!(heard(&mut app).is_empty());
}

#[gpui::test]
fn the_panels_settings_show_over_the_video_are_told_as_they_change(cx: &mut TestAppContext) {
    cx.update(app::set_up_for_tests);
    let mut app = cx.update(|cx| listen(Scope::App, false, cx));
    let told = |app: &mut UnboundedReceiver<Event>| -> Vec<_> {
        heard(app)
            .into_iter()
            .filter(|(kind, _)| *kind != "app.command_run")
            .collect()
    };

    cx.update(|cx| Settings::update(cx, false, |settings| settings.over_video.notes = false));
    cx.run_until_parked();
    assert_eq!(
        told(&mut app),
        [
            (
                "app.settings.changed",
                json!({"keys": ["over_video.notes"]})
            ),
            ("app.notes.off", json!({})),
        ]
    );

    let run = |cx: &mut TestAppContext, command| {
        cx.update(|cx| api::run(Invocation::new(command), cx))
            .unwrap();
        cx.run_until_parked();
    };
    run(cx, "app.ui.toggle");
    assert_eq!(
        told(&mut app),
        [
            (
                "app.settings.changed",
                json!({"keys": [
                    "over_video.controls",
                    "over_video.top_bar",
                    "over_video.track_switcher",
                ]})
            ),
            ("app.ui.off", json!({})),
            ("app.controls.off", json!({})),
            ("app.top_bar.off", json!({})),
            ("app.track_switcher.off", json!({})),
        ]
    );
    run(cx, "app.top_bar.toggle");
    assert_eq!(
        told(&mut app),
        [
            (
                "app.settings.changed",
                json!({"keys": ["over_video.top_bar"]})
            ),
            ("app.ui.on", json!({})),
            ("app.top_bar.on", json!({})),
        ]
    );
}

#[gpui::test]
fn a_command_is_told_before_what_it_changes_and_a_query_isnt(cx: &mut TestAppContext) {
    cx.update(app::set_up_for_tests);
    let mut app = cx.update(|cx| listen(Scope::App, false, cx));
    let run = |cx: &mut TestAppContext, invocation| {
        cx.update(|cx| api::run(invocation, cx)).unwrap();
        cx.run_until_parked();
    };

    run(cx, Invocation::new("app.settings.ls"));
    run(
        cx,
        Invocation::new("app.settings.get").with_args(json!({"key": "autoplay"})),
    );
    assert!(heard(&mut app).is_empty(), "queries only read");

    let set = json!({"key": "autoplay", "value": false});
    run(
        cx,
        Invocation::new("app.settings.set")
            .with_args(set.clone())
            .from(Source::Palette),
    );
    assert_eq!(
        heard(&mut app),
        [
            (
                "app.command_run",
                json!({"command": "app.settings.set", "source": "palette", "args": set})
            ),
            ("app.settings.changed", json!({"keys": ["autoplay"]})),
        ]
    );

    // Aimed somewhere, it says where.
    let aimed = Invocation::new("window.rename")
        .with_args(json!({"label": "news"}))
        .in_window("2");
    assert!(
        cx.update(|cx| api::run(aimed, cx)).is_err(),
        "no such window"
    );
    assert_eq!(
        heard(&mut app),
        [(
            "app.command_run",
            json!({
                "command": "window.rename",
                "source": "ui",
                "args": {"label": "news"},
                "target": {"window": "2"},
            })
        )]
    );
}

#[gpui::test]
fn looks_only_while_someone_listens(cx: &mut TestAppContext) {
    cx.update(app::set_up_for_tests);
    let looking = |cx: &mut TestAppContext| {
        cx.update(|cx| {
            cx.try_global::<Events>()
                .map(|events| (events.looking, events.seen.is_some()))
        })
    };
    assert_eq!(looking(cx), None, "no one has listened");

    let first = cx.update(|cx| listen(Scope::App, false, cx));
    let second = cx.update(|cx| listen(Scope::All, true, cx));
    assert_eq!(looking(cx), Some((true, true)));
    drop(first);
    cx.executor().advance_clock(LOOK_EVERY);
    cx.run_until_parked();
    assert_eq!(looking(cx), Some((true, true)), "one still listens");

    drop(second);
    cx.executor().advance_clock(LOOK_EVERY);
    cx.run_until_parked();
    assert_eq!(looking(cx), Some((false, false)), "what it saw is let go");

    // Listened to again, it looks again, once.
    let _again = cx.update(|cx| listen(Scope::App, false, cx));
    let _more = cx.update(|cx| listen(Scope::App, false, cx));
    assert_eq!(looking(cx), Some((true, true)));
}
