//! `settings`: every setting, read, changed and put back by its key, as
//! the Settings window changes it; and the settings file.

use gpui::App;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::api::Error;
use crate::api::args::{Arg, Kind};
use crate::api::registry::{Args, Outcome, Scope, Spec};
use crate::cast::Casting;
use crate::files::Folder;
use crate::settings::Settings;
use crate::settings::fields::{self, Field};
use crate::ui::settings::installed_fonts;

const KEY: Arg = Arg::required(
    "key",
    Kind::Text,
    "The setting's key, its path in the settings file: autoplay, subtitles.size.",
);

const SET: &[Arg] = &[
    KEY,
    Arg::required(
        "value",
        Kind::Any,
        "What to set it to, as the settings file keeps it: true, 0.5, \"dark\".",
    ),
];

pub const COMMANDS: &[Spec] = &[
    Spec::query(
        "settings.ls",
        "List Settings",
        "List every setting: its key, what Settings calls it and the pane it's on, what \
         it takes, and its value.",
        Scope::App,
        "[{key, title, pane, kind, value, default}], in the order Settings has them: \
         kind is toggle, choice (with choices: [{value, label}]), number (with min and \
         max), font, folder, output or keys.",
        |_, _, cx| Ok(listing(cx)),
    )
    .keywords(&["list", "ls", "preferences", "options"]),
    Spec::query_with(
        "settings.get",
        "Get Setting",
        "Read a setting by its key, such as subtitles.size.",
        Scope::App,
        &[KEY],
        "Its value, as the settings file keeps it.",
        |args, _, cx| {
            let field = field(args, cx)?;
            Ok(fields::get(cx.global::<Settings>(), &field.key))
        },
    )
    .keywords(&["preferences", "options", "read"]),
    Spec::app_with(
        "settings.set",
        "Change Setting",
        "Change a setting by its key, to a value Settings offers, and save it.",
        SET,
        set,
    )
    .keywords(&["preferences", "options"])
    .returns("Its value now."),
    Spec::app_with(
        "settings.reset",
        "Reset Setting",
        "Put a setting back as Plays starts it out, and save it.",
        &[KEY],
        reset,
    )
    .keywords(&["preferences", "options", "default", "restore"])
    .returns("Its value now."),
    Spec::app_with(
        "settings.reveal_file",
        "Show Settings File",
        "Show the settings file where the system shows files, saving it first if it \
         isn't there yet.",
        &[],
        reveal,
    )
    .keywords(&["preferences", "json", "finder", "reveal"])
    .returns("The file's path."),
    Spec::app_with(
        "settings.reload",
        "Reload Settings",
        "Read the settings file again, after it was changed by hand.",
        &[],
        reload,
    )
    .keywords(&["preferences", "json", "refresh"])
    .returns("[key]: the settings the file changed."),
];

/// The protocols that look for devices to cast to, each a setting.
fn protocols(cx: &App) -> Vec<&'static str> {
    Casting::protocols(cx)
        .into_iter()
        .map(|(kind, _, _)| kind)
        .collect()
}

/// Each setting changed from how Plays starts it out, with its value, in
/// the order Settings has them.
pub fn changed(cx: &App) -> Vec<(String, Value)> {
    let settings = cx.global::<Settings>();
    let defaults = Settings::default();
    fields::all(&protocols(cx))
        .into_iter()
        .filter_map(|field| {
            let value = fields::get(settings, &field.key);
            (value != fields::get(&defaults, &field.key)).then(|| (field.key.into_owned(), value))
        })
        .collect()
}

/// What `settings.ls` says.
fn listing(cx: &App) -> Value {
    let settings = cx.global::<Settings>();
    let defaults = Settings::default();
    let listed: Vec<Value> = fields::all(&protocols(cx))
        .iter()
        .map(|field| {
            let mut about = json!({
                "key": field.key,
                "title": field.title,
                "pane": field.pane,
                "kind": field.kind.name(),
                "value": fields::get(settings, &field.key),
                "default": fields::get(&defaults, &field.key),
            });
            match field.kind {
                fields::Kind::Choice(choices) => {
                    let choices: Vec<Value> = choices()
                        .into_iter()
                        .map(|(value, label)| json!({"value": value, "label": label}))
                        .collect();
                    about["choices"] = choices.into();
                }
                fields::Kind::Number { min, max } => {
                    about["min"] = min.into();
                    about["max"] = max.into();
                }
                _ => {}
            }
            about
        })
        .collect();
    listed.into()
}

#[derive(Deserialize)]
struct Key {
    key: String,
}

/// The setting `args` names by its key.
fn field(args: Args, cx: &App) -> Result<Field, Error> {
    let Key { key } = args.read()?;
    fields::find(&key, &protocols(cx)).ok_or_else(|| Error::not_found("setting", key))
}

/// Set `field` to `value`, if it takes it, and save it.
fn put(field: &Field, value: Value, command: &str, cx: &mut App) -> Outcome {
    let fonts: Vec<String> = match field.kind {
        fields::Kind::Font => installed_fonts(cx)
            .into_iter()
            .map(|font| font.to_string())
            .collect(),
        _ => Vec::new(),
    };
    let changed = fields::with(cx.global::<Settings>(), field, value, &fonts)
        .map_err(|why| Error::bad_args(command, why))?;
    Settings::update(cx, true, |settings| *settings = changed);
    Ok(fields::get(cx.global::<Settings>(), &field.key))
}

fn set(args: Args, cx: &mut App) -> Outcome {
    #[derive(Deserialize)]
    struct Set {
        value: Value,
    }
    let field = field(args, cx)?;
    let Set { value } = args.read()?;
    put(&field, value, args.command, cx)
}

fn reset(args: Args, cx: &mut App) -> Outcome {
    let field = field(args, cx)?;
    let value = fields::get(&Settings::default(), &field.key);
    put(&field, value, args.command, cx)
}

/// Where the settings are kept, or why they aren't.
fn file(cx: &App) -> Result<std::path::PathBuf, Error> {
    Settings::path(cx).ok_or_else(|| Error::failed("the settings aren't kept in a file here"))
}

fn reveal(_: Args, cx: &mut App) -> Outcome {
    let path = file(cx)?;
    if !path.exists() {
        cx.global::<Settings>().save(cx);
    }
    // gpui's test platform can't.
    #[cfg(not(test))]
    cx.reveal_path(&path);
    Ok(json!(path.display().to_string()))
}

fn reload(_: Args, cx: &mut App) -> Outcome {
    let path = file(cx)?;
    if !path.exists() {
        return Err(Error::failed(
            "there's no settings file yet: every setting is as Plays starts it out",
        ));
    }
    let loaded = Settings::load(&Folder::of(cx));
    let before = cx.global::<Settings>();
    let changed: Vec<String> = fields::all(&protocols(cx))
        .into_iter()
        .filter(|field| fields::get(before, &field.key) != fields::get(&loaded, &field.key))
        .map(|field| field.key.into_owned())
        .collect();
    // Read from the file, so not saved over it.
    Settings::update(cx, false, |settings| *settings = loaded);
    Ok(json!(changed))
}
