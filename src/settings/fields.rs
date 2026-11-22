//! Every setting, as the `settings` commands and the palette know it: its
//! key, the path to it in the settings file (`subtitles.size`); what the
//! Settings window calls it and the pane it's on; and what it takes, as
//! the window offers it. The window's choices are the ones here, so a
//! command can set what a click can, and nothing else.

use std::borrow::Cow;
use std::path::Path;

use serde::Serialize;
use serde_json::{Map, Value};

use crate::media::tracks;

use super::{
    ControlsVisibility, FloatOnTop, FullScreen, GAMMA_RANGE, JPEG_QUALITY_RANGE, MaxResolution,
    PanelLook, SUBTITLE_COLORS, SUBTITLE_POSITION_RANGE, SUBTITLE_SIZE_RANGE, ScreenshotFormat,
    ScreenshotHdr, Settings, SeveralFiles, SubtitleArea, SubtitleEdge, SubtitleWeight, ThemeMode,
    VideoFit,
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

pub const SEVERAL_FILES: [(SeveralFiles, &str); 2] = [
    (SeveralFiles::Together, "Together"),
    (SeveralFiles::Playlist, "One after another"),
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
    (SubtitleArea::Block, "Block"),
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
    /// Languages, first first, each by its name or code: kept by its code.
    Languages,
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
            Self::Languages => "languages",
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
        "several_files",
        "Play several files",
        "Playback",
        Kind::Choice(|| choices(&SEVERAL_FILES)),
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
        "languages.audio",
        "Audio languages",
        "Audio",
        Kind::Languages,
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
    Field::new(
        "languages.subtitles",
        "Subtitle languages",
        "Subtitles",
        Kind::Languages,
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
        "over_video.controls",
        "Show controls",
        "Appearance",
        Kind::Toggle,
    ),
    Field::new(
        "over_video.top_bar",
        "Show the top bar",
        "Appearance",
        Kind::Toggle,
    ),
    Field::new(
        "over_video.track_switcher",
        "Show the track switcher",
        "Appearance",
        Kind::Toggle,
    ),
    Field::new("over_video.notes", "Show notes", "Appearance", Kind::Toggle),
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
    mut value: Value,
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
        Kind::Languages => match languages(&value) {
            Some(codes) => value = Value::from(codes),
            None => {
                return takes("a list of languages by name or code, like [\"English\", \"fr\"]");
            }
        },
        Kind::Toggle | Kind::Output | Kind::Keys => {}
    }
    let mut json = serde_json::to_value(settings).map_err(|err| err.to_string())?;
    put(&mut json, key, value);
    serde_json::from_value(json).map_err(|err| format!("{key} can't be that: {err}"))
}

/// The languages `value` lists, each by its name or code, as Settings keeps
/// them: by code (`tracks::language`), each once, in the order given.
/// `None` if it isn't a list of languages.
pub fn languages(value: &Value) -> Option<Vec<&'static str>> {
    let mut codes = Vec::new();
    for word in value.as_array()? {
        let code = tracks::language(word.as_str()?.trim())?;
        if !codes.contains(&code) {
            codes.push(code);
        }
    }
    Some(codes)
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
#[path = "tests/fields.rs"]
mod tests;
