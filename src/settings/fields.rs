//! Every setting, as the `settings` commands and the palette know it: its
//! key, the path to it in the settings file (`subtitles.size`); what the
//! Settings window calls it and the pane it's on; and what it takes, as
//! the window offers it. The window's choices are the ones here, so a
//! command can set what a click can, and nothing else.

use std::borrow::Cow;
use std::path::Path;

use serde::Serialize;
use serde_json::{Map, Value};

use super::{
    ControlsVisibility, FloatOnTop, FullScreen, GAMMA_RANGE, JPEG_QUALITY_RANGE, MaxResolution,
    PanelLook, SUBTITLE_COLORS, SUBTITLE_POSITION_RANGE, SUBTITLE_SIZE_RANGE, ScreenshotFormat,
    ScreenshotHdr, Settings, SubtitleArea, SubtitleEdge, SubtitleWeight, ThemeMode, VideoFit,
};

pub const THEMES: [(ThemeMode, &str); 3] = [
    (ThemeMode::System, "System"),
    (ThemeMode::Dark, "Dark"),
    (ThemeMode::Light, "Light"),
];

/// None for as many as fit.
pub const COLUMNS: [(u32, &str); 5] = [(0, "Auto"), (1, "1"), (2, "2"), (3, "3"), (4, "4")];

pub const CONTROLS: [(ControlsVisibility, &str); 2] = [
    (ControlsVisibility::OnHover, "On hover"),
    (ControlsVisibility::Always, "Always"),
];

pub const PANELS: [(PanelLook, &str); 5] = [
    (PanelLook::Plain, "Plain"),
    (PanelLook::Frosted, "Frosted"),
    (PanelLook::Glass, "Glass"),
    (PanelLook::Glow, "Glow"),
    (PanelLook::Tint, "Tint"),
];

pub const FLOATING: [(FloatOnTop, &str); 3] = [
    (FloatOnTop::Off, "Off"),
    (FloatOnTop::Always, "Always"),
    (FloatOnTop::WhilePlaying, "While playing"),
];

pub const FULL_SCREENS: [(FullScreen, &str); 2] = [
    (FullScreen::OwnSpace, "Own Space"),
    (FullScreen::OverDisplay, "Over the display"),
];

pub const SCREENSAVER_MINUTES: [(u32, &str); 6] = [
    (1, "1 min"),
    (2, "2 min"),
    (5, "5 min"),
    (10, "10 min"),
    (15, "15 min"),
    (30, "30 min"),
];

pub const SEEK_STEPS: [(u32, &str); 4] = [(5, "5 s"), (10, "10 s"), (30, "30 s"), (60, "60 s")];

pub const FITS: [(VideoFit, &str); 2] = [(VideoFit::Contain, "Fit"), (VideoFit::Cover, "Fill")];

pub const RESOLUTIONS: [(MaxResolution, &str); 5] = [
    (MaxResolution::Original, "Full"),
    (MaxResolution::P2160, "2160p"),
    (MaxResolution::P1080, "1080p"),
    (MaxResolution::P720, "720p"),
    (MaxResolution::P480, "480p"),
];

pub const WEIGHTS: [(SubtitleWeight, &str); 3] = [
    (SubtitleWeight::Regular, "Regular"),
    (SubtitleWeight::Medium, "Medium"),
    (SubtitleWeight::Bold, "Bold"),
];

pub const EDGES: [(SubtitleEdge, &str); 3] = [
    (SubtitleEdge::None, "None"),
    (SubtitleEdge::Outline, "Outline"),
    (SubtitleEdge::Shadow, "Shadow"),
];

pub const AREAS: [(SubtitleArea, &str); 2] = [
    (SubtitleArea::Picture, "Picture"),
    (SubtitleArea::Tile, "Tile"),
];

pub const FORMATS: [(ScreenshotFormat, &str); 2] = [
    (ScreenshotFormat::Png, "PNG"),
    (ScreenshotFormat::Jpeg, "JPEG"),
];

pub const HDRS: [(ScreenshotHdr, &str); 2] =
    [(ScreenshotHdr::Sdr, "SDR"), (ScreenshotHdr::Keep, "HDR")];

/// What a setting takes.
#[derive(Clone, Copy)]
pub enum Kind {
    /// On or off.
    Toggle,
    /// One of these, as the file keeps it, each with what the window calls
    /// it.
    Choice(fn() -> Vec<(Value, &'static str)>),
    /// A number from `min` to `max`.
    Number { min: f64, max: f64 },
    /// A font family the system has, or none for the system's own.
    Font,
    /// A folder's full path, or none.
    Folder,
    /// An output, `{id, name}`, or none for the system's default.
    Output,
    /// The shortcuts changed, by command.
    Keys,
}

impl Kind {
    /// What the commands call it.
    pub fn name(self) -> &'static str {
        match self {
            Self::Toggle => "toggle",
            Self::Choice(_) => "choice",
            Self::Number { .. } => "number",
            Self::Font => "font",
            Self::Folder => "folder",
            Self::Output => "output",
            Self::Keys => "keys",
        }
    }
}

/// One setting.
#[derive(Clone)]
pub struct Field {
    /// The path to it in the settings file, joined by dots.
    pub key: Cow<'static, str>,
    /// What the Settings window calls it.
    pub title: Cow<'static, str>,
    /// The Settings pane it's on.
    pub pane: &'static str,
    pub kind: Kind,
}

impl Field {
    const fn new(key: &'static str, title: &'static str, pane: &'static str, kind: Kind) -> Self {
        Self {
            key: Cow::Borrowed(key),
            title: Cow::Borrowed(title),
            pane,
            kind,
        }
    }
}

/// The choices `options` offer, as the file keeps each.
fn choices<T: Serialize>(options: &[(T, &'static str)]) -> Vec<(Value, &'static str)> {
    options
        .iter()
        .map(|(value, label)| (serde_json::to_value(value).unwrap_or_default(), *label))
        .collect()
}

const fn range(range: &std::ops::RangeInclusive<f32>) -> Kind {
    Kind::Number {
        min: *range.start() as f64,
        max: *range.end() as f64,
    }
}

const PERCENT: Kind = Kind::Number { min: 0.0, max: 1.0 };

/// Every setting but whether each protocol looks for devices, which
/// `all` adds for the protocols there are, in the order the Settings
/// window has them.
const FIELDS: &[Field] = &[
    Field::new(
        "autoplay",
        "Play files when opened",
        "Playback",
        Kind::Toggle,
    ),
    Field::new(
        "loop_by_default",
        "Loop new files",
        "Playback",
        Kind::Toggle,
    ),
    Field::new(
        "resume_files",
        "Resume files where they were left",
        "Playback",
        Kind::Toggle,
    ),
    Field::new(
        "chapter_marks",
        "Show chapters on the seek bar",
        "Playback",
        Kind::Toggle,
    ),
    Field::new(
        "default_volume",
        "Volume for new files",
        "Playback",
        PERCENT,
    ),
    Field::new(
        "seek_step_seconds",
        "Seek step",
        "Playback",
        Kind::Choice(|| choices(&SEEK_STEPS)),
    ),
    Field::new("master_volume", "Master volume", "Audio", PERCENT),
    Field::new("audio_output.device", "Output", "Audio", Kind::Output),
    Field::new(
        "audio_output.wait_for_device",
        "Wait for it",
        "Audio",
        Kind::Toggle,
    ),
    Field::new(
        "sound_follows_display",
        "Play on the display's speakers",
        "Audio",
        Kind::Toggle,
    ),
    Field::new(
        "video_fit",
        "Scaling",
        "Video",
        Kind::Choice(|| choices(&FITS)),
    ),
    Field::new("sdr_gamma", "SDR gamma", "Video", range(&GAMMA_RANGE)),
    Field::new("hdr_gamma", "HDR gamma", "Video", range(&GAMMA_RANGE)),
    Field::new(
        "max_resolution",
        "Resolution limit",
        "Video",
        Kind::Choice(|| choices(&RESOLUTIONS)),
    ),
    Field::new(
        "hardware_decoding",
        "Hardware decoding",
        "Video",
        Kind::Toggle,
    ),
    Field::new("subtitles.font", "Font", "Subtitles", Kind::Font),
    Field::new(
        "subtitles.weight",
        "Weight",
        "Subtitles",
        Kind::Choice(|| choices(&WEIGHTS)),
    ),
    Field::new(
        "subtitles.size",
        "Size",
        "Subtitles",
        range(&SUBTITLE_SIZE_RANGE),
    ),
    Field::new(
        "subtitles.color",
        "Color",
        "Subtitles",
        Kind::Choice(|| choices(&SUBTITLE_COLORS)),
    ),
    Field::new(
        "subtitles.edge",
        "Edge",
        "Subtitles",
        Kind::Choice(|| choices(&EDGES)),
    ),
    Field::new(
        "subtitles.edge_color",
        "Edge color",
        "Subtitles",
        Kind::Choice(|| choices(&SUBTITLE_COLORS)),
    ),
    Field::new(
        "subtitles.background_opacity",
        "Background",
        "Subtitles",
        PERCENT,
    ),
    Field::new(
        "subtitles.background",
        "Background color",
        "Subtitles",
        Kind::Choice(|| choices(&SUBTITLE_COLORS)),
    ),
    Field::new(
        "subtitles.area",
        "Place on",
        "Subtitles",
        Kind::Choice(|| choices(&AREAS)),
    ),
    Field::new(
        "subtitles.position",
        "Height",
        "Subtitles",
        range(&SUBTITLE_POSITION_RANGE),
    ),
    Field::new("screenshots.folder", "Folder", "Screenshots", Kind::Folder),
    Field::new(
        "screenshots.format",
        "Format",
        "Screenshots",
        Kind::Choice(|| choices(&FORMATS)),
    ),
    Field::new(
        "screenshots.jpeg_quality",
        "JPEG quality",
        "Screenshots",
        Kind::Number {
            min: *JPEG_QUALITY_RANGE.start() as f64,
            max: *JPEG_QUALITY_RANGE.end() as f64,
        },
    ),
    Field::new(
        "screenshots.subtitles",
        "Subtitles",
        "Screenshots",
        Kind::Toggle,
    ),
    Field::new(
        "screenshots.hdr",
        "HDR video",
        "Screenshots",
        Kind::Choice(|| choices(&HDRS)),
    ),
    Field::new("hotkeys", "Shortcuts", "Hotkeys", Kind::Keys),
    Field::new(
        "theme",
        "Theme",
        "Appearance",
        Kind::Choice(|| choices(&THEMES)),
    ),
    Field::new(
        "columns",
        "Columns",
        "Appearance",
        Kind::Choice(|| choices(&COLUMNS)),
    ),
    Field::new(
        "controls",
        "Player controls",
        "Appearance",
        Kind::Choice(|| choices(&CONTROLS)),
    ),
    Field::new(
        "panels",
        "Panels over the video",
        "Appearance",
        Kind::Choice(|| choices(&PANELS)),
    ),
    Field::new("show_titles", "Show file names", "Appearance", Kind::Toggle),
    Field::new(
        "float_on_top",
        "Float on top",
        "Appearance",
        Kind::Choice(|| choices(&FLOATING)),
    ),
    Field::new(
        "full_screen",
        "Full screen",
        "Appearance",
        Kind::Choice(|| choices(&FULL_SCREENS)),
    ),
    Field::new(
        "black_out_displays",
        "Black out other displays",
        "Appearance",
        Kind::Toggle,
    ),
    Field::new("screensaver", "Screensaver", "Appearance", Kind::Toggle),
    Field::new(
        "screensaver_minutes",
        "Start after",
        "Appearance",
        Kind::Choice(|| choices(&SCREENSAVER_MINUTES)),
    ),
    Field::new(
        "command_line_control",
        "Allow control from the command line",
        "Command Line",
        Kind::Toggle,
    ),
];

/// Where the settings of which protocols look for devices are.
const LOOK_WITH: &str = "casting.look_with";

/// The key of whether `protocol` looks for devices.
pub fn look_with(protocol: &str) -> String {
    format!("{LOOK_WITH}.{protocol}")
}

/// Every setting, with whether each of `protocols` looks for devices.
pub fn all(protocols: &[&'static str]) -> Vec<Field> {
    let looking = protocols.iter().map(|&protocol| Field {
        key: look_with(protocol).into(),
        title: protocol.into(),
        pane: "Casting",
        kind: Kind::Toggle,
    });
    FIELDS.iter().cloned().chain(looking).collect()
}

/// The setting `key` names, of those `all` gives for `protocols`.
pub fn find(key: &str, protocols: &[&'static str]) -> Option<Field> {
    all(protocols).into_iter().find(|field| field.key == key)
}

/// The path to `key` in the file, as a JSON pointer.
fn pointer(key: &str) -> String {
    format!("/{}", key.replace('.', "/"))
}

/// `key`'s value in `settings`. A protocol left out looks for devices.
pub fn get(settings: &Settings, key: &str) -> Value {
    let json = serde_json::to_value(settings).unwrap_or_default();
    match json.pointer(&pointer(key)) {
        Some(value) => value.clone(),
        None if key.starts_with(LOOK_WITH) => Value::Bool(true),
        None => Value::Null,
    }
}

/// The keys of the settings `new` has otherwise than `old`, in the order
/// Settings has them, with whether each of `protocols` looks for devices.
pub fn changed(old: &Settings, new: &Settings, protocols: &[&'static str]) -> Vec<String> {
    all(protocols)
        .into_iter()
        .filter(|field| get(old, &field.key) != get(new, &field.key))
        .map(|field| field.key.into_owned())
        .collect()
}

/// `settings` with `field` set to `value`, if it's one the Settings window
/// could set: else what it takes instead. A font is checked against
/// `fonts`, those the system has.
pub fn with(
    settings: &Settings,
    field: &Field,
    value: Value,
    fonts: &[String],
) -> Result<Settings, String> {
    let key = &field.key;
    let takes = |what: &str| Err(format!("{key} is {what}"));
    match field.kind {
        Kind::Toggle if !value.is_boolean() => return takes("true or false"),
        Kind::Choice(choices) => {
            let choices = choices();
            if !choices.iter().any(|(choice, _)| *choice == value) {
                let named: Vec<String> = choices
                    .iter()
                    .map(|(choice, _)| choice.to_string())
                    .collect();
                return takes(&format!("one of {}", named.join(", ")));
            }
        }
        Kind::Number { min, max } => match value.as_f64() {
            Some(number) if (min..=max).contains(&number) => {}
            _ => return takes(&format!("a number from {min} to {max}")),
        },
        Kind::Font => match value.as_str() {
            Some("") => {}
            Some(font) if fonts.iter().any(|known| known == font) => {}
            _ => return takes("a font this system has, or \"\" for the system's own"),
        },
        Kind::Folder => match &value {
            Value::Null => {}
            Value::String(path) if Path::new(path).is_absolute() && !Path::new(path).is_file() => {}
            _ => return takes("a folder's full path, or null for Pictures"),
        },
        Kind::Output if !(value.is_null() || value.is_object()) => {
            return takes("an output, {id, name}, or null for the system's default");
        }
        Kind::Keys if !value.is_object() => return takes("the shortcuts changed, by command"),
        Kind::Toggle | Kind::Output | Kind::Keys => {}
    }
    let mut json = serde_json::to_value(settings).map_err(|err| err.to_string())?;
    put(&mut json, key, value);
    serde_json::from_value(json).map_err(|err| format!("{key} can't be that: {err}"))
}

/// Put `value` at `key` in `json`, making the objects on the way.
fn put(json: &mut Value, key: &str, value: Value) {
    let mut here = json;
    let mut parts = key.split('.').peekable();
    while let Some(part) = parts.next() {
        if !here.is_object() {
            *here = Value::Object(Map::new());
        }
        let Value::Object(fields) = here else {
            unreachable!("an object")
        };
        if parts.peek().is_none() {
            fields.insert(part.to_owned(), value);
            return;
        }
        here = fields.entry(part.to_owned()).or_insert(Value::Null);
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;

    use super::*;
    use crate::settings::{AudioDevice, Rgb};

    /// The keys of the values in `value` at `at`, going into each object
    /// that has something in it.
    fn leaves(value: &Value, at: &str, keys: &mut Vec<String>) {
        match value {
            Value::Object(fields) if !fields.is_empty() => {
                for (key, value) in fields {
                    let key = if at.is_empty() {
                        key.clone()
                    } else {
                        format!("{at}.{key}")
                    };
                    leaves(value, &key, keys);
                }
            }
            _ => keys.push(at.to_owned()),
        }
    }

    #[test]
    fn every_setting_the_file_keeps_has_a_key() {
        let file = serde_json::to_value(Settings::default()).unwrap();
        let mut kept = Vec::new();
        leaves(&file, "", &mut kept);
        // Each protocol's is listed for the protocols there are.
        kept.retain(|key| key != LOOK_WITH);
        kept.sort();
        let mut keys: Vec<String> = all(&[]).into_iter().map(|f| f.key.into_owned()).collect();
        keys.sort();
        assert_eq!(keys, kept);

        let protocols = all(&["Chromecast", "DLNA"]);
        let looking: Vec<(&str, &str, &str)> = protocols[protocols.len() - 2..]
            .iter()
            .map(|field| (field.key.as_ref(), field.title.as_ref(), field.pane))
            .collect();
        assert_eq!(
            looking,
            [
                ("casting.look_with.Chromecast", "Chromecast", "Casting"),
                ("casting.look_with.DLNA", "DLNA", "Casting"),
            ]
        );
    }

    #[test]
    fn takes_what_the_window_offers() {
        let settings = Settings::default();
        let fonts = ["Menlo".to_owned()];
        let set = |key: &str, value: Value| {
            let field = find(key, &["Chromecast"]).unwrap();
            with(&settings, &field, value, &fonts).unwrap()
        };
        assert!(!set("autoplay", json!(false)).autoplay);
        assert_eq!(set("theme", json!("dark")).theme, ThemeMode::Dark);
        assert_eq!(set("columns", json!(3)).columns, 3);
        assert_eq!(set("seek_step_seconds", json!(30)).seek_step_seconds, 30);
        assert_eq!(set("master_volume", json!(0.25)).master_volume, 0.25);
        assert_eq!(set("subtitles.size", json!(1.5)).subtitles.size, 1.5);
        assert_eq!(
            set("screenshots.jpeg_quality", json!(70))
                .screenshots
                .jpeg_quality,
            70
        );
        assert_eq!(
            set("subtitles.color", json!("#ffd60a")).subtitles.color,
            Rgb(0xffd60a)
        );
        assert_eq!(
            set("subtitles.font", json!("Menlo")).subtitles.font,
            "Menlo"
        );
        assert_eq!(set("subtitles.font", json!("")).subtitles.font, "");
        let folder = std::env::temp_dir().join("plays-shots");
        assert_eq!(
            set("screenshots.folder", json!(folder)).screenshots.folder,
            Some(folder)
        );
        assert_eq!(
            set("audio_output.device", json!({"id": "desk", "name": "Desk"}))
                .audio_output
                .device,
            Some(AudioDevice {
                id: "desk".into(),
                name: "Desk".into(),
            })
        );
        let keys = set("hotkeys", json!({"playback.play_pause": ["cmd-p"]})).hotkeys;
        assert_eq!(keys.keys("playback.play_pause"), ["cmd-p"]);
        let looking = set("casting.look_with.Chromecast", json!(false)).casting;
        assert!(!looking.looks_with("Chromecast"));
        assert!(looking.looks_with("DLNA"), "the others left be");
        let unset = set("screenshots.folder", Value::Null);
        assert_eq!(unset.screenshots.folder, None::<PathBuf>);
    }

    #[test]
    fn turns_the_rest_away_saying_what_it_takes() {
        let settings = Settings::default();
        let fonts = ["Menlo".to_owned()];
        let set = |key: &str, value: Value| {
            let field = find(key, &[]).unwrap();
            with(&settings, &field, value, &fonts).unwrap_err()
        };
        assert_eq!(set("autoplay", json!("yes")), "autoplay is true or false");
        assert_eq!(
            set("theme", json!("purple")),
            r#"theme is one of "system", "dark", "light""#
        );
        assert_eq!(set("columns", json!(7)), "columns is one of 0, 1, 2, 3, 4");
        assert_eq!(
            set("seek_step_seconds", json!(7)),
            "seek_step_seconds is one of 5, 10, 30, 60"
        );
        assert_eq!(
            set("subtitles.size", json!(3.0)),
            "subtitles.size is a number from 0.5 to 2"
        );
        assert_eq!(
            set("master_volume", json!("loud")),
            "master_volume is a number from 0 to 1"
        );
        assert!(
            set("subtitles.color", json!("#123456"))
                .starts_with(r##"subtitles.color is one of "#ffffff""##),
            "a color the window doesn't offer"
        );
        assert_eq!(
            set("subtitles.font", json!("Comic Sans")),
            r#"subtitles.font is a font this system has, or "" for the system's own"#
        );
        assert_eq!(
            set("screenshots.folder", json!("shots")),
            "screenshots.folder is a folder's full path, or null for Pictures"
        );
        assert_eq!(
            set("audio_output.device", json!("desk")),
            "audio_output.device is an output, {id, name}, or null for the system's default"
        );
        assert!(
            set("audio_output.device", json!({"id": "desk"}))
                .starts_with("audio_output.device can't be that: missing field `name`"),
            "an output with no name"
        );
        assert!(
            set("screenshots.jpeg_quality", json!(70.5))
                .starts_with("screenshots.jpeg_quality can't be that"),
            "a quality between whole numbers"
        );
        assert_eq!(
            set("hotkeys", json!(["cmd-p"])),
            "hotkeys is the shortcuts changed, by command"
        );
    }

    #[test]
    fn reads_a_setting_by_its_key() {
        let mut settings = Settings::default();
        settings.subtitles.size = 1.5;
        assert_eq!(get(&settings, "subtitles.size"), json!(1.5));
        assert_eq!(get(&settings, "theme"), json!("system"));
        assert_eq!(get(&settings, "audio_output.device"), Value::Null);
        assert_eq!(
            get(&settings, "casting.look_with.Chromecast"),
            json!(true),
            "a protocol left out looks"
        );
        settings
            .casting
            .look_with
            .insert("Chromecast".into(), false);
        assert_eq!(get(&settings, "casting.look_with.Chromecast"), json!(false));
    }

    #[test]
    fn tells_which_settings_differ_in_their_order() {
        let old = Settings::default();
        let mut new = old.clone();
        assert!(changed(&old, &new, &["Chromecast"]).is_empty());
        new.subtitles.size += 1.0;
        new.autoplay = !new.autoplay;
        new.casting.look_with.insert("Chromecast".into(), false);
        assert_eq!(
            changed(&old, &new, &["Chromecast"]),
            ["autoplay", "subtitles.size", "casting.look_with.Chromecast"]
        );
        assert_eq!(
            changed(&old, &new, &[]),
            ["autoplay", "subtitles.size"],
            "a protocol not looked with isn't a setting"
        );
    }
}
