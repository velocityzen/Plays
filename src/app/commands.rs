//! The app's actions, from keys, menus and the controls that send them:
//! each is a command of the API's, which runs it.

use gpui::{Action, App};
use serde_json::{Value, json};

use super::{
    About, CastTo, CloseAll, CloseFile, CloseWindow, GoToChapter, GoToWindow, NextWindow,
    OpenFiles, OpenSettings, OpenUrl, PreviousWindow, Quit, SetAspectRatio, SetAudioDevice,
    SetCrop, SetWindowAudioDevice, ShowOnDisplay,
};
use crate::api::{self, Invocation, Source};
use crate::hotkeys::{self, Command, PauseAll, PlayAll};
use crate::settings::AudioDevice;

pub(super) fn register(cx: &mut App) {
    hotkeys::register_actions(cx, route);
    macro_rules! route {
        ($($action:ty),+ $(,)?) => {
            $(cx.on_action(|action: &$action, cx| route(action, cx));)+
        };
    }
    route!(
        About,
        OpenFiles,
        OpenUrl,
        OpenSettings,
        Quit,
        CloseFile,
        CloseWindow,
        CloseAll,
        NextWindow,
        PreviousWindow,
        PlayAll,
        PauseAll,
        GoToWindow,
        SetAspectRatio,
        SetCrop,
        SetAudioDevice,
        SetWindowAudioDevice,
        ShowOnDisplay,
        GoToChapter,
        CastTo,
    );
}

/// Run `action`'s command once the event it came with is handled: gpui
/// holds the window in front until then, which the command may need.
fn route(action: &dyn Action, cx: &mut App) {
    match invocation(action) {
        Some(invocation) => api::dispatch(invocation.from(Source::Action), cx),
        None => log::warn!("{} is no command", action.name()),
    }
}

/// The command `action` runs, with its arguments.
pub(crate) fn invocation(action: &dyn Action) -> Option<Invocation> {
    if let Some(command) = Command::ALL
        .into_iter()
        .find(|command| command.action().partial_eq(action))
    {
        return api::for_hotkey(command).map(|spec| Invocation::new(spec.id));
    }
    let action = action.as_any();
    let plain: [(bool, &str); 12] = [
        (action.is::<About>(), "app.about"),
        (action.is::<OpenSettings>(), "app.settings"),
        (action.is::<Quit>(), "app.quit"),
        (action.is::<OpenFiles>(), "window.open_files"),
        (action.is::<OpenUrl>(), "window.open_url"),
        (action.is::<CloseWindow>(), "window.close"),
        (action.is::<NextWindow>(), "window.next"),
        (action.is::<PreviousWindow>(), "window.previous"),
        (action.is::<CloseFile>(), "tile.close"),
        (action.is::<CloseAll>(), "tile.close_all"),
        (action.is::<PlayAll>(), "playback.play_all"),
        (action.is::<PauseAll>(), "playback.pause_all"),
    ];
    if let Some((_, command)) = plain.into_iter().find(|(is, _)| *is) {
        return Some(Invocation::new(command));
    }
    let invocation = if let Some(GoToWindow(nth)) = action.downcast_ref() {
        // Counted from 1.
        Invocation::new("window.focus").in_window((nth + 1).to_string())
    } else if let Some(SetAspectRatio(ratio)) = action.downcast_ref() {
        Invocation::new("video.set_aspect_ratio")
            .with_args(json!({"ratio": ratio.map(|ratio| ratio.name)}))
    } else if let Some(SetCrop(crop)) = action.downcast_ref() {
        Invocation::new("video.set_crop").with_args(json!({"crop": crop.map(|crop| crop.name)}))
    } else if let Some(SetAudioDevice(device)) = action.downcast_ref() {
        Invocation::new("output.set_device").with_args(output(device.as_ref()))
    } else if let Some(SetWindowAudioDevice(device)) = action.downcast_ref() {
        Invocation::new("output.set_window_device").with_args(output(device.as_ref()))
    } else if let Some(ShowOnDisplay(display)) = action.downcast_ref() {
        Invocation::new("window.show_on_display").with_args(json!({"display": display}))
    } else if let Some(GoToChapter(index)) = action.downcast_ref() {
        // Counted from 1.
        Invocation::new("playback.go_to_chapter").with_args(json!({"number": index + 1}))
    } else if let Some(CastTo(device)) = action.downcast_ref() {
        match device {
            Some(device) => Invocation::new("cast.to")
                .with_args(json!({"device": device.id, "kind": device.kind})),
            None => Invocation::new("cast.stop"),
        }
    } else {
        return None;
    };
    Some(invocation)
}

/// The arguments the output commands take for `device`: by id and name,
/// as it may be one not connected now.
fn output(device: Option<&AudioDevice>) -> Value {
    device.map_or(
        Value::Null,
        |device| json!({"id": device.id, "name": device.name}),
    )
}

#[cfg(test)]
mod tests {
    use gpui::TestAppContext;

    use super::*;
    use crate::cast::{Device, Shows};
    use crate::ui::framing::{ASPECT_RATIOS, CROPS};

    /// Every action the app has: each command's, and the app's own.
    fn every_action() -> Vec<Box<dyn Action>> {
        let device = Device {
            kind: "Chromecast",
            id: "tv-1".into(),
            name: "Den".into(),
            shows: Shows::Video,
            address: [192, 0, 2, 1].into(),
        };
        let speakers = AudioDevice {
            id: "coreaudio:Speakers".into(),
            name: "Speakers".into(),
        };
        let mut actions: Vec<Box<dyn Action>> =
            Command::ALL.into_iter().map(Command::action).collect();
        actions.extend([
            Box::new(About) as Box<dyn Action>,
            Box::new(OpenFiles),
            Box::new(OpenUrl),
            Box::new(OpenSettings),
            Box::new(Quit),
            Box::new(CloseFile),
            Box::new(CloseWindow),
            Box::new(CloseAll),
            Box::new(NextWindow),
            Box::new(PreviousWindow),
            Box::new(PlayAll),
            Box::new(PauseAll),
            Box::new(GoToWindow(0)),
            Box::new(SetAspectRatio(None)),
            Box::new(SetAspectRatio(Some(ASPECT_RATIOS[1]))),
            Box::new(SetCrop(None)),
            Box::new(SetCrop(Some(CROPS[2]))),
            Box::new(SetAudioDevice(None)),
            Box::new(SetAudioDevice(Some(speakers.clone()))),
            Box::new(SetWindowAudioDevice(None)),
            Box::new(SetWindowAudioDevice(Some(speakers))),
            Box::new(ShowOnDisplay(1)),
            Box::new(GoToChapter(0)),
            Box::new(CastTo(None)),
            Box::new(CastTo(Some(device))),
        ]);
        actions
    }

    #[gpui::test]
    fn every_action_is_a_command_with_arguments_it_takes(cx: &mut TestAppContext) {
        cx.update(|cx| {
            register(cx);
            for action in every_action() {
                assert!(
                    cx.is_action_available(&*action),
                    "nothing does {}",
                    action.name()
                );
                let invocation = invocation(&*action)
                    .unwrap_or_else(|| panic!("{} is no command", action.name()));
                let spec = api::find(&invocation.command)
                    .unwrap_or_else(|| panic!("no command {}", invocation.command));
                api::check_args(spec, &invocation.args)
                    .unwrap_or_else(|err| panic!("{action:?}: {err}"));
            }
        });
    }

    #[test]
    fn hands_on_what_actions_carry() {
        let run = |action: &dyn Action| invocation(action).unwrap();
        assert_eq!(
            run(&GoToWindow(2)),
            Invocation::new("window.focus").in_window("3"),
            "⌘3, the third window"
        );
        assert_eq!(
            run(&GoToChapter(0)),
            Invocation::new("playback.go_to_chapter").with_args(json!({"number": 1}))
        );
        assert_eq!(
            run(&SetCrop(Some(CROPS[2]))),
            Invocation::new("video.set_crop").with_args(json!({"crop": CROPS[2].name}))
        );
        assert_eq!(
            run(&SetAspectRatio(None)),
            Invocation::new("video.set_aspect_ratio").with_args(json!({"ratio": null}))
        );
        assert_eq!(
            run(&SetWindowAudioDevice(Some(AudioDevice {
                id: "a".into(),
                name: "Radio".into()
            }))),
            Invocation::new("output.set_window_device")
                .with_args(json!({"id": "a", "name": "Radio"}))
        );
        assert_eq!(
            run(&SetAudioDevice(None)),
            Invocation::new("output.set_device")
        );
        assert_eq!(run(&ShowOnDisplay(7)).args, json!({"display": 7}));
        assert_eq!(run(&CastTo(None)), Invocation::new("cast.stop"));
        assert_eq!(
            run(&hotkeys::PlayPause),
            Invocation::new("playback.play_pause")
        );
        assert_eq!(run(&CloseFile), Invocation::new("tile.close"));
        assert_eq!(run(&CloseWindow), Invocation::new("window.close"));
    }
}
