use std::collections::HashSet;

use serde_json::{Value, json};

use super::super::{Event, Reach};
use super::*;
use crate::api::{self, Id};

#[test]
fn each_event_is_registered_once_and_said_whole() {
    let mut names = HashSet::new();
    for (namespace, kind) in all() {
        assert!(names.insert(kind.name), "{} twice", kind.name);
        assert!(
            api::all().any(|spec| spec.namespace() == namespace),
            "{namespace} has commands"
        );
        assert!(kind.description.ends_with('.'), "{}", kind.name);
        let mut fields = HashSet::new();
        for field in kind.fields {
            assert!(
                !kind.heard.ids().contains(&field.name),
                "{}'s ids come first, with what hears it",
                kind.name
            );
            assert!(
                fields.insert(field.name),
                "{}: {} twice",
                kind.name,
                field.name
            );
            assert!(
                field.description.ends_with('.'),
                "{}: {}",
                kind.name,
                field.name
            );
        }
        for (at, cause) in kind.causes.iter().enumerate() {
            assert!(
                !cause.is_empty() && !cause.ends_with('.'),
                "{}'s cause goes on a sentence",
                kind.name
            );
            assert!(
                !kind.causes[..at].contains(cause),
                "{}'s causes say {cause} twice",
                kind.name
            );
        }
    }
    assert_eq!(
        find("audio.volume_changed").map(|kind| kind.heard),
        Some(Heard::Block)
    );
    assert!(find("volume").is_none());
}

#[test]
fn each_event_is_named_in_its_namespace_as_commands_are() {
    for (namespace, kind) in all() {
        let name = kind.name.strip_prefix(namespace);
        let name = name.and_then(|name| name.strip_prefix('.'));
        assert!(
            name.is_some_and(
                |name| !name.is_empty() && name.chars().all(|c| c.is_ascii_lowercase() || c == '_')
            ),
            "{} is {namespace}. and lowercase words",
            kind.name
        );
    }
}

#[test]
fn each_event_is_told_by_a_command_or_says_what_else_tells_it() {
    for (_, kind) in all() {
        assert!(
            !kind.causes.is_empty() || api::all().any(|spec| spec.tells.contains(&kind.name)),
            "{} is told by nothing",
            kind.name
        );
    }
}

#[test]
fn commands_say_they_tell_events_there_are_each_once() {
    for spec in api::all() {
        let mut told = HashSet::new();
        for &name in spec.tells {
            assert!(
                find(name).is_some(),
                "{} tells {name}, which isn't one",
                spec.id
            );
            assert!(told.insert(name), "{} tells {name} twice", spec.id);
        }
        assert!(
            !spec.query || spec.tells.is_empty(),
            "{} only reads",
            spec.id
        );
    }
}

/// `kind`, told with `payload` to the app's listeners if `app`, and to
/// those of `windows`.
fn told(kind: &'static str, payload: Value, app: bool, windows: &[Id]) -> Event {
    Event {
        kind,
        payload,
        reach: Reach {
            app,
            windows: windows.to_vec(),
            everyone: false,
        },
    }
}

#[test]
fn an_event_fits_only_as_registered() {
    let (window, other) = (Id::next(), Id::next());
    let payload = |fields: Value| {
        let mut payload = json!({
            "window_id": window.to_string(),
            "window_title": "Plays",
            "block_id": Id::next().to_string(),
            "block_title": "clip.mp4",
        });
        let fields = fields.as_object().expect("fields").clone();
        payload.as_object_mut().expect("an object").extend(fields);
        payload
    };
    let volume = |fields: Value| told("audio.volume_changed", payload(fields), false, &[window]);
    let loud = json!({"volume": 40, "muted": false});
    assert_eq!(fits(&volume(loud.clone())), Ok(()));
    assert_eq!(
        fits(&volume(json!({"volume": 40}))),
        Err("its payload has no muted".into())
    );
    assert_eq!(
        fits(&volume(json!({"volume": 40, "muted": false, "loud": true}))),
        Err("its payload has loud".into())
    );
    assert_eq!(
        fits(&told(
            "audio.volume_changed",
            loud.clone(),
            false,
            &[window]
        )),
        Err("its payload has no window_id".into())
    );
    let elsewhere = told(
        "audio.volume_changed",
        payload(loud.clone()),
        false,
        &[other],
    );
    assert!(fits(&elsewhere).is_err(), "heard by another window");
    let app_too = told(
        "audio.volume_changed",
        payload(loud.clone()),
        true,
        &[window],
    );
    assert!(
        fits(&app_too).is_err(),
        "the app's listeners hear only theirs"
    );
    let unheard = told("audio.volume_changed", payload(loud), false, &[]);
    assert!(fits(&unheard).is_err(), "heard by none");
    assert_eq!(
        fits(&told("loudness", json!({}), true, &[])),
        Err("it isn't in it".into())
    );
    assert!(
        fits(&told("app.quit", json!([]), true, &[])).is_err(),
        "a list"
    );

    // An optional field may be left out; the app's events name no window.
    let run = json!({"command": "app.about", "source": "cli"});
    assert_eq!(
        fits(&told("app.command_run", run.clone(), true, &[])),
        Ok(())
    );
    assert!(fits(&told("app.command_run", run.clone(), true, &[window])).is_err());
    assert!(fits(&told("app.command_run", run, false, &[])).is_err());

    // The app's listeners and the window's, or none in front.
    let front = |id: Option<Id>, windows: &[Id], app: bool| {
        let payload = json!({"window_id": id.map(|id| id.to_string()), "window_title": null});
        fits(&told("window.active_changed", payload, app, windows))
    };
    assert_eq!(front(Some(window), &[window], true), Ok(()));
    assert_eq!(front(None, &[], true), Ok(()));
    assert!(front(Some(window), &[], true).is_err());
    assert!(front(None, &[window], true).is_err());
    assert!(front(Some(window), &[other], true).is_err());
    assert!(front(Some(window), &[window], false).is_err());

    // Every listener, or not.
    assert_eq!(fits(&Event::to_everyone("app.quit", json!({}))), Ok(()));
    assert!(fits(&told("app.quit", json!({}), true, &[])).is_err());
    let run = json!({"command": "app.quit", "source": "ui"});
    assert!(fits(&Event::to_everyone("app.command_run", run)).is_err());
}

#[gpui::test]
#[should_panic(expected = "audio.volume_changed isn't told as the events reference says")]
fn an_event_told_otherwise_than_registered_fails_the_test(cx: &mut gpui::TestAppContext) {
    let event = Event::app("audio.volume_changed", json!({}));
    cx.update(|cx| super::super::tell(&event, cx));
}
