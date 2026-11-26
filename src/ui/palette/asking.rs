//! What the palette asks for before running a command that takes
//! arguments: each it needs, and each it can do without that there's a
//! choice of. A choice is picked from a list, such as a block's audio
//! tracks, the displays or a setting's values, read from the queries the
//! command line reads too; anything else is typed, such as a time to seek
//! to, as 1:23:45.

use std::rc::Rc;

use gpui::{App, SharedString};
use serde_json::{Map, Value, json};

use super::aimed;
use super::places::{self, text, window_name};
use crate::api::args::{Arg, Kind};
use crate::api::commands::output;
use crate::api::commands::settings::protocols;
use crate::api::{self, Invocation, Source, Spec, Target};
use crate::files;
use crate::media::tracks;
use crate::settings::Settings;
use crate::settings::fields::{self, Field};
use crate::ui::player::format_time;
use crate::ui::sentence::capitalized;
use crate::ui::settings::installed_fonts;

/// One of the things to pick for an argument.
#[derive(Clone, Debug, PartialEq)]
pub struct Choice {
    pub label: SharedString,
    /// Dimmer, after it: a chapter's start, a device's protocol.
    pub detail: Option<SharedString>,
    /// The arguments it gives: `{number: 2}`, or `{device, kind}`; none
    /// for what the command does with none given.
    pub args: Map<String, Value>,
    /// It's what's picked now.
    pub current: bool,
}

impl Choice {
    fn new(label: impl Into<SharedString>, args: Value) -> Self {
        Self {
            label: label.into(),
            detail: None,
            args: match args {
                Value::Object(args) => args,
                _ => Map::new(),
            },
            current: false,
        }
    }

    fn detail(mut self, detail: Option<impl Into<SharedString>>) -> Self {
        self.detail = detail.map(Into::into);
        self
    }

    fn current(mut self, current: bool) -> Self {
        self.current = current;
        self
    }
}

/// Reads what's typed: the value of the argument, or why it won't do.
pub type Read = Rc<dyn Fn(&str) -> Result<Value, String>>;

/// How an argument is asked for.
#[derive(Clone)]
pub enum Ask {
    /// Picked from these.
    Choose(Vec<Choice>),
    /// Typed, and read so.
    Type(Read),
}

/// Asking for one argument.
#[derive(Clone)]
pub struct Question {
    /// The argument asked for.
    pub arg: &'static str,
    /// What the field says while nothing's typed.
    pub prompt: SharedString,
    pub ask: Ask,
}

/// What to ask next before running `spec` on `window` (a target naming
/// it), given the arguments it has and those asked for already: each it
/// needs, and each it can do without that there's a choice of, in order.
/// `None` once there's nothing more to ask.
pub fn next(
    spec: &'static Spec,
    asked: &[&'static str],
    args: &Map<String, Value>,
    window: &Target,
    cx: &mut App,
) -> Option<Question> {
    let mut on = On { window, cx };
    spec.args
        .iter()
        .filter(|arg| !asked.contains(&arg.name) && !args.contains_key(arg.name))
        .find_map(|arg| question(spec, arg, args, &mut on))
}

/// The window asked about, and the app.
struct On<'a, 'b> {
    window: &'a Target,
    cx: &'b mut App,
}

impl On<'_, '_> {
    /// What a query hands back about the window, or what's in it, or null.
    fn query(&mut self, command: &str) -> Value {
        let target =
            api::find(command).map_or_else(Target::default, |spec| aimed(spec, self.window));
        let mut invocation = Invocation::new(command).from(Source::Palette);
        invocation.target = target;
        api::run(invocation, self.cx).unwrap_or_default()
    }

    /// What `command` changes, as it is now, on the window or what's in
    /// it, or null.
    fn state(&mut self, command: &str) -> Value {
        let target =
            api::find(command).map_or_else(Target::default, |spec| aimed(spec, self.window));
        api::state(command, &target, self.cx).unwrap_or_default()
    }

    /// The API's name for the window.
    fn window_id(&self) -> Option<&str> {
        self.window.window.as_deref()
    }
}

/// Asking for `arg` of `spec`, given `args`: from its choices if it has
/// them, else typed if it's needed. `None` for one it can do without.
fn question(
    spec: &Spec,
    arg: &'static Arg,
    args: &Map<String, Value>,
    on: &mut On,
) -> Option<Question> {
    let asking = |prompt: &str, ask| {
        Some(Question {
            arg: arg.name,
            prompt: prompt.to_owned().into(),
            ask,
        })
    };
    if let Some((prompt, choices)) = choices(spec, arg, args, on) {
        return asking(prompt, Ask::Choose(choices));
    }
    if !arg.required {
        return None;
    }
    if let Some((prompt, read)) = typed(spec, arg, args, on.cx) {
        return asking(&prompt, Ask::Type(read));
    }
    let read: Read = Rc::new(move |typed| read_typed(arg, typed));
    asking(prompt_for(arg), Ask::Type(read))
}

/// What the field says for `arg`, typed: its description, without the
/// full stop.
fn prompt_for(arg: &Arg) -> &'static str {
    arg.description.trim_end_matches('.')
}

/// The command whose arguments `id`'s are asked as: a window's, or every
/// window's, command for its blocks asks as the block's does, from the
/// block in front.
fn asked_as(id: &'static str) -> &'static str {
    match id.split_once('.') {
        Some(("window" | "app", rest)) if rest.contains('.') => {
            api::find(&format!("block.{rest}")).map_or(id, |spec| spec.id)
        }
        _ => id,
    }
}

/// The choices of `arg` of `spec`, and what the field says: a block's
/// tracks, the displays, and so on; for one of a set of words, those.
fn choices(
    spec: &Spec,
    arg: &Arg,
    args: &Map<String, Value>,
    on: &mut On,
) -> Option<(&'static str, Vec<Choice>)> {
    let listed = match (asked_as(spec.id), arg.name) {
        ("block.audio.select_track", "number") => ("Audio track", tracks("audio", None, on)),
        ("block.video.select_track", "number") => ("Video track", tracks("video", None, on)),
        ("block.subtitles.select_track", "number") => {
            ("Subtitles", tracks("subtitles", Some("Off"), on))
        }
        ("block.playback.go_to_chapter", "number") => ("Chapter", chapters(on)),
        ("block.playback.play_file", "path") => ("File", folder_files(on)),
        ("window.move", "position") => ("Where", window_places(on)),
        ("window.show_on_display", "display") => ("Display", displays(on)),
        ("block.move", "position") => ("Where", block_places(on)),
        ("block.move_to_window", "window") => ("Window", other_windows(on)),
        ("cast.to" | "cast.pair", "device") => ("Device", devices(on)),
        ("output.set_device", "id") => ("Output", outputs(spec.id, "System Default", on)),
        ("output.set_block_device", "id") => ("Output", outputs(spec.id, output::SAME_AS_APP, on)),
        ("app.settings.get" | "app.settings.reset", "key") => ("Setting", settings(true, on.cx)),
        ("app.settings.set", "key") => ("Setting", settings(false, on.cx)),
        ("app.settings.set", "value") => ("Value", setting_values(args, on)?),
        // Opened as Settings say: the palette asks only what to open.
        ("app.open", _) => return None,
        (_, name) => match arg.kind {
            Kind::Choice(words) => {
                let none = (!arg.required).then_some(match asked_as(spec.id) {
                    "block.video.set_aspect_ratio" => "Original",
                    "block.video.set_crop" => "No Crop",
                    _ => "None",
                });
                ("Pick one", words_of(spec, name, words(), none, on))
            }
            Kind::Choices(words) => ("Pick", lists_of(spec, name, words(), on)),
            _ => return None,
        },
    };
    Some(listed)
}

/// The front block's tracks of `kind` (`audio`, `video`, `subtitles`),
/// after `none` for none, if they can be turned off.
fn tracks(kind: &str, none: Option<&'static str>, on: &mut On) -> Vec<Choice> {
    let block = on.query("block.inspect");
    let picked = block["picked"][kind].as_u64();
    let off = none.map(|label| Choice::new(label, json!({})).current(picked.is_none()));
    let listed = block["tracks"][kind]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|track| {
            let number = track["number"].as_u64()?;
            let label =
                text(&track["label"]).map_or_else(|| format!("Track {number}"), str::to_owned);
            Some(Choice::new(label, json!({ "number": number })).current(picked == Some(number)))
        });
    off.into_iter().chain(listed).collect()
}

/// The media files in the front block's folder, the one it plays picked.
fn folder_files(on: &mut On) -> Vec<Choice> {
    let (files, playing) = places::folder_files(on.window, on.cx);
    files
        .into_iter()
        .map(|file| {
            let current = playing.as_ref() == Some(&file);
            Choice::new(files::name(&file), json!({ "path": file })).current(current)
        })
        .collect()
}

/// The front block's chapters, each with where it starts.
fn chapters(on: &mut On) -> Vec<Choice> {
    let block = on.query("block.inspect");
    let current = block["chapter"].as_u64();
    block["chapters"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|chapter| {
            let number = chapter["number"].as_u64()?;
            let name =
                text(&chapter["name"]).map_or_else(|| format!("Chapter {number}"), str::to_owned);
            let start = chapter["start"].as_f64().map(format_time);
            Some(
                Choice::new(name, json!({ "number": number }))
                    .detail(start)
                    .current(current == Some(number)),
            )
        })
        .collect()
}

/// Each player window's place, ⌘1 and on, by the window there now.
fn window_places(on: &mut On) -> Vec<Choice> {
    let windows = on.query("window.ls");
    let this = on.window_id();
    windows
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|window| {
            let number = window["number"].as_u64()?;
            Some(
                Choice::new(number.to_string(), json!({ "position": number }))
                    .detail(Some(window_name(window)))
                    .current(this.is_some_and(|this| window["id"] == this)),
            )
        })
        .collect()
}

/// Each block's place in the window, by the block there now, the one keys
/// go to picked.
fn block_places(on: &mut On) -> Vec<Choice> {
    let blocks = on.query("block.ls");
    blocks
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|block| {
            let number = block["number"].as_u64()?;
            let name = text(&block["label"]).or_else(|| text(&block["title"]));
            Some(
                Choice::new(number.to_string(), json!({ "position": number }))
                    .detail(name.map(str::to_owned))
                    .current(block["front"] == true),
            )
        })
        .collect()
}

/// The player windows but this one.
fn other_windows(on: &mut On) -> Vec<Choice> {
    let windows = on.query("window.ls");
    let this = on.window_id();
    windows
        .as_array()
        .into_iter()
        .flatten()
        .filter(|window| this.is_none_or(|this| window["id"] != this))
        .filter_map(|window| {
            let id = text(&window["id"])?;
            let blocks = window["blocks"].as_u64().unwrap_or_default();
            let files = match blocks {
                1 => "1 file".to_owned(),
                blocks => format!("{blocks} files"),
            };
            Some(Choice::new(window_name(window), json!({ "window": id })).detail(Some(files)))
        })
        .collect()
}

/// The displays, the one the window is on picked.
fn displays(on: &mut On) -> Vec<Choice> {
    let shown_on = on.query("window.inspect")["display"].as_u64();
    let displays = on.query("display.ls");
    displays
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|display| {
            let id = display["id"].as_u64()?;
            let name =
                text(&display["name"]).map_or_else(|| format!("Display {id}"), str::to_owned);
            Some(Choice::new(name, json!({ "display": id })).current(shown_on == Some(id)))
        })
        .collect()
}

/// The devices found to cast to, each with its protocol, which a device's
/// id is told apart by.
fn devices(on: &mut On) -> Vec<Choice> {
    let casting = on.state("cast.to");
    let found = on.query("cast.ls");
    found
        .as_object()
        .into_iter()
        .flatten()
        .flat_map(|(kind, devices)| {
            devices
                .as_array()
                .into_iter()
                .flatten()
                .map(move |device| (kind, device))
        })
        .filter_map(|(kind, device)| {
            let id = text(&device["id"])?;
            let name = text(&device["name"]).unwrap_or(id);
            let current = casting["id"] == id && casting["kind"] == kind.as_str();
            Some(
                Choice::new(name.to_owned(), json!({ "device": id, "kind": kind }))
                    .detail(Some(kind.clone()))
                    .current(current),
            )
        })
        .collect()
}

/// The outputs connected, after `none`, for what the command does given
/// none; the one `command` has picked now.
fn outputs(command: &str, none: &str, on: &mut On) -> Vec<Choice> {
    let picked = on.state(command);
    let listed = on.query("output.ls");
    let default = Choice::new(none.to_owned(), json!({})).current(picked.is_null());
    let connected = listed["outputs"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|output| {
            let id = text(&output["id"])?;
            let name = text(&output["name"]).unwrap_or(id);
            Some(
                Choice::new(name.to_owned(), json!({ "id": id, "name": name }))
                    .detail(text(&output["kind"]).map(str::to_owned))
                    .current(picked["id"] == id),
            )
        })
        .collect::<Vec<_>>();
    std::iter::once(default).chain(connected).collect()
}

/// Every setting, as Settings calls it, by its pane; with the shortcuts,
/// which can't be typed here but can be read and put back, if `keys`.
fn settings(keys: bool, cx: &mut App) -> Vec<Choice> {
    fields::all(&protocols(cx))
        .into_iter()
        .filter(|field| keys || !matches!(field.kind, fields::Kind::Keys))
        .map(|field| {
            Choice::new(field.title.to_string(), json!({ "key": field.key }))
                .detail(Some(field.pane))
        })
        .collect()
}

/// The setting `args` names.
fn field(args: &Map<String, Value>, cx: &App) -> Option<Field> {
    fields::find(args.get("key")?.as_str()?, &protocols(cx))
}

/// The values the setting `args` names can be picked from, as Settings
/// offers them, the one it has now picked; `None` for one typed.
fn setting_values(args: &Map<String, Value>, on: &mut On) -> Option<Vec<Choice>> {
    let field = field(args, on.cx)?;
    let now = fields::get(on.cx.global::<Settings>(), &field.key);
    let value = |label: &str, value: Value| {
        let current = value == now;
        Choice::new(label.to_owned(), json!({ "value": value })).current(current)
    };
    let values = match field.kind {
        fields::Kind::Toggle => vec![value("On", json!(true)), value("Off", json!(false))],
        fields::Kind::Choice(choices) => choices()
            .into_iter()
            .map(|(choice, label)| value(label, choice))
            .collect(),
        fields::Kind::Font => std::iter::once(value("System", json!("")))
            .chain(
                installed_fonts(on.cx)
                    .iter()
                    .map(|font| value(font, json!(font.as_ref()))),
            )
            .collect(),
        fields::Kind::Output => {
            let listed = on.query("output.ls");
            std::iter::once(value("System Default", Value::Null))
                .chain(
                    listed["outputs"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|output| {
                            let id = text(&output["id"])?;
                            let name = text(&output["name"]).unwrap_or(id);
                            let current = now["id"] == id;
                            Some(
                                Choice::new(
                                    name.to_owned(),
                                    json!({ "value": { "id": id, "name": name } }),
                                )
                                .current(current),
                            )
                        }),
                )
                .collect()
        }
        fields::Kind::Number { .. }
        | fields::Kind::Folder
        | fields::Kind::Keys
        | fields::Kind::Languages => return None,
    };
    Some(values)
}

/// One of `words`, after `none` if it can be done without: the one
/// `spec` has now picked.
fn words_of(
    spec: &Spec,
    name: &str,
    words: Vec<&'static str>,
    none: Option<&'static str>,
    on: &mut On,
) -> Vec<Choice> {
    let now = on.state(spec.id);
    let off = none.map(|label| Choice::new(label, json!({})).current(now.is_null()));
    let listed = words.into_iter().map(|word| {
        let mut args = Map::new();
        args.insert(name.to_owned(), json!(word));
        Choice::new(word, Value::Object(args)).current(now == word)
    });
    off.into_iter().chain(listed).collect()
}

/// Lists of `words` to pick from for the argument `name`, as `spec` takes
/// them: none, all of them, and each alone; the one it has now picked.
fn lists_of(spec: &Spec, name: &str, words: Vec<&'static str>, on: &mut On) -> Vec<Choice> {
    let now = on.state(spec.id);
    let list = |label: String, listed: Vec<&'static str>| {
        let current = now == json!(listed);
        let mut args = Map::new();
        args.insert(name.to_owned(), json!(listed));
        Choice::new(label, Value::Object(args)).current(current)
    };
    let mut lists = vec![
        list("None".to_owned(), Vec::new()),
        list("All".to_owned(), words.clone()),
    ];
    if words.len() > 1 {
        lists.extend(
            words
                .iter()
                .map(|&word| list(format!("Only {}", titled(word)), vec![word])),
        );
    }
    lists
}

/// A word as a label: `top_bar` as Top Bar.
fn titled(word: &str) -> String {
    word.split('_')
        .map(|part| {
            let mut chars = part.chars();
            chars.next().map_or_else(String::new, |first| {
                first.to_uppercase().chain(chars).collect()
            })
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// For one typed that's read otherwise than its kind says, what the
/// field says and how it's read: a time, a path, a setting's number.
fn typed(
    spec: &Spec,
    arg: &'static Arg,
    args: &Map<String, Value>,
    cx: &App,
) -> Option<(String, Read)> {
    match (asked_as(spec.id), arg.name) {
        ("block.playback.seek", "time") => Some((
            "A time, such as 1:23:45 or 90".to_owned(),
            Rc::new(move |typed| {
                let seconds = seconds(typed.trim())
                    .ok_or_else(|| "That's not a time: try 1:23:45 or 90".to_owned())?;
                Ok(json!(seconds))
            }),
        )),
        ("app.open", "urls") => Some((
            "A file's or folder's full path, or a stream's URL".to_owned(),
            Rc::new(move |typed| {
                let typed = typed.trim();
                if typed.is_empty() {
                    return Err("Type a path or a URL".to_owned());
                }
                Ok(json!([typed]))
            }),
        )),
        ("app.settings.set", "value") => {
            let field = field(args, cx)?;
            match field.kind {
                fields::Kind::Number { min, max } => Some((
                    format!("A number from {min} to {max}"),
                    Rc::new(move |typed| match typed.trim().parse::<f64>() {
                        Ok(number) if (min..=max).contains(&number) => Ok(json!(number)),
                        _ => Err(format!("Type a number from {min} to {max}")),
                    }),
                )),
                fields::Kind::Folder => Some((
                    "A folder's full path; none for Pictures".to_owned(),
                    Rc::new(|typed| {
                        let typed = typed.trim();
                        Ok(if typed.is_empty() {
                            Value::Null
                        } else {
                            json!(typed)
                        })
                    }),
                )),
                fields::Kind::Languages => Some((
                    "Languages, first first, such as English, fr; none for each file's own"
                        .to_owned(),
                    Rc::new(|typed| languages_typed(typed).map(|codes| json!(codes))),
                )),
                _ => None,
            }
        }
        _ => None,
    }
}

/// The languages typed, by name or code and split by commas, as Settings
/// keeps them; none is none.
fn languages_typed(typed: &str) -> Result<Vec<&'static str>, String> {
    let words: Vec<&str> = typed
        .split(',')
        .map(str::trim)
        .filter(|word| !word.is_empty())
        .collect();
    if let Some(unknown) = words.iter().find(|word| tracks::language(word).is_none()) {
        return Err(format!("{unknown} isn't a language Plays knows"));
    }
    fields::languages(&json!(words)).ok_or_else(|| "Type languages, such as English, fr".to_owned())
}

/// What's typed for `arg`, read as its kind says, and checked.
fn read_typed(arg: &Arg, typed: &str) -> Result<Value, String> {
    let typed = typed.trim();
    if typed.is_empty() {
        return Err(format!("Type {}", prompt_for(arg).to_lowercase()));
    }
    let value = match arg.kind {
        Kind::Integer { .. } => typed
            .parse::<i64>()
            .map(Value::from)
            .map_err(|_| "Type a whole number".to_owned())?,
        Kind::Number { .. } => match typed.parse::<f64>() {
            Ok(number) if number.is_finite() => json!(number),
            _ => return Err("Type a number".to_owned()),
        },
        Kind::Texts => json!([typed]),
        Kind::Toggle => match typed.to_lowercase().as_str() {
            "true" | "on" | "yes" => json!(true),
            "false" | "off" | "no" => json!(false),
            _ => return Err("Type true or false".to_owned()),
        },
        Kind::Choices(_) => json!(
            typed
                .split([',', ' '])
                .filter(|word| !word.is_empty())
                .collect::<Vec<_>>()
        ),
        Kind::Any => serde_json::from_str(typed).unwrap_or_else(|_| json!(typed)),
        Kind::Text | Kind::Choice(_) => json!(typed),
    };
    arg.check(&value).map_err(|why| capitalized(&why))?;
    Ok(value)
}

/// The seconds in a time typed as 1:23:45, 23:45 or 90, a fraction of a
/// second too; `None` if it isn't one.
pub fn seconds(typed: &str) -> Option<f64> {
    let parts: Vec<&str> = typed.split(':').collect();
    if parts.len() > 3 {
        return None;
    }
    let mut seconds = 0.0;
    for (at, part) in parts.iter().enumerate() {
        let last = at + 1 == parts.len();
        let number: f64 = if last {
            part.parse().ok().filter(|n: &f64| n.is_finite())?
        } else {
            f64::from(part.parse::<u32>().ok()?)
        };
        if number < 0.0 || at > 0 && number >= 60.0 {
            return None;
        }
        seconds = seconds * 60.0 + number;
    }
    Some(seconds)
}

#[cfg(test)]
#[path = "tests/asking.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/asking_properties.rs"]
mod properties;
