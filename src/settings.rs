use std::collections::BTreeMap;
use std::fs;
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui::{App, BorrowAppContext, Global};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::files::{self, Folder};
use crate::hotkeys::Hotkeys;

/// Which Plays' settings a file holds, written in it as `version`. Files
/// from before there was one are the first.
pub const VERSION: u64 = 1;
/// The settings' file, in the app's folder.
const FILE: &str = "settings.json";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeMode {
    System,
    Dark,
    Light,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VideoFit {
    /// Show the whole picture, letterboxed.
    Contain,
    /// Fill the tile, cropping the edges.
    Cover,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaxResolution {
    Original,
    P2160,
    P1080,
    P720,
    P480,
}

impl MaxResolution {
    pub fn short_side(self) -> Option<u32> {
        match self {
            Self::Original => None,
            Self::P2160 => Some(2160),
            Self::P1080 => Some(1080),
            Self::P720 => Some(720),
            Self::P480 => Some(480),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlsVisibility {
    Always,
    OnHover,
}

/// How panels over the video (its controls, notes, menus and console) show
/// the picture through them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PanelLook {
    /// See-through black.
    Plain,
    /// The picture blurred and darkened.
    Frosted,
    /// Glass: the picture a little frosted, under a clear, polished edge
    /// that magnifies it and catches the light.
    Glass,
    /// Glass whose edge glows with the picture's colors, inside and out.
    Glow,
    /// The colors around, without the picture.
    Tint,
}

/// Whether player windows stay above other apps' windows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FloatOnTop {
    Off,
    Always,
    /// Only a window with something playing in it.
    WhilePlaying,
}

impl FloatOnTop {
    /// `mode` turned on, or off if it already is, as its menu item does.
    pub fn toggled(self, mode: Self) -> Self {
        if self == mode { Self::Off } else { mode }
    }
}

/// How player windows go full screen, on macOS.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FullScreen {
    /// In a Space of their own, as macOS has it.
    #[default]
    OwnSpace,
    /// Over their display, leaving the other displays as they are.
    OverDisplay,
}

impl FullScreen {
    pub fn style(self) -> gpui::FullscreenStyle {
        match self {
            Self::OwnSpace => gpui::FullscreenStyle::OwnSpace,
            Self::OverDisplay => gpui::FullscreenStyle::OverDisplay,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubtitleWeight {
    Regular,
    Medium,
    Bold,
}

impl SubtitleWeight {
    pub fn value(self) -> f32 {
        match self {
            Self::Regular => 400.0,
            Self::Medium => 500.0,
            Self::Bold => 700.0,
        }
    }
}

/// What sets subtitle text off from the picture behind it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubtitleEdge {
    None,
    Outline,
    Shadow,
}

/// What subtitle text is placed in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubtitleArea {
    /// The picture, so text stays on it.
    Picture,
    /// The whole tile, so text can go in the bars around a letterboxed picture.
    Tile,
}

/// A color, written as `"#rrggbb"` in the settings file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb(pub u32);

impl Serialize for Rgb {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&format!("#{:06x}", self.0 & 0xff_ffff))
    }
}

impl<'de> Deserialize<'de> for Rgb {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        let hex = text.trim().trim_start_matches('#');
        match u32::from_str_radix(hex, 16) {
            Ok(value) if hex.len() == 6 => Ok(Rgb(value)),
            _ => Err(serde::de::Error::custom(format!(
                "{text:?} isn't a color like \"#ffcc00\""
            ))),
        }
    }
}

/// The colors the subtitle settings offer, with their names.
pub const SUBTITLE_COLORS: [(Rgb, &str); 8] = [
    (Rgb(0xffffff), "White"),
    (Rgb(0xffd60a), "Yellow"),
    (Rgb(0x30d158), "Green"),
    (Rgb(0x64d2ff), "Cyan"),
    (Rgb(0x0a84ff), "Blue"),
    (Rgb(0xbf5af2), "Magenta"),
    (Rgb(0xff453a), "Red"),
    (Rgb(0x000000), "Black"),
];

/// How subtitles look and where they go.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SubtitleStyle {
    /// A font family; empty for the system's own.
    pub font: String,
    pub weight: SubtitleWeight,
    /// Relative to the standard size, which is 5% of the picture's height.
    pub size: f32,
    pub color: Rgb,
    pub edge: SubtitleEdge,
    pub edge_color: Rgb,
    pub background: Rgb,
    /// 0 for no background.
    pub background_opacity: f32,
    pub area: SubtitleArea,
    /// How far above the bottom of the area text sits, as a fraction of
    /// the area's height.
    pub position: f32,
}

pub const SUBTITLE_SIZE_RANGE: RangeInclusive<f32> = 0.5..=2.0;
pub const SUBTITLE_POSITION_RANGE: RangeInclusive<f32> = 0.0..=0.4;

impl Default for SubtitleStyle {
    fn default() -> Self {
        Self {
            font: String::new(),
            weight: SubtitleWeight::Medium,
            size: 1.0,
            color: Rgb(0xffffff),
            edge: SubtitleEdge::Outline,
            edge_color: Rgb(0x000000),
            background: Rgb(0x000000),
            background_opacity: 0.0,
            area: SubtitleArea::Picture,
            position: 0.04,
        }
    }
}

impl SubtitleStyle {
    /// The style with every number in its range, whatever the file says.
    pub fn clamped(&self) -> Self {
        let clamp = |value: f32, range: RangeInclusive<f32>, default: f32| {
            if value.is_finite() {
                value.clamp(*range.start(), *range.end())
            } else {
                default
            }
        };
        let default = Self::default();
        Self {
            size: clamp(self.size, SUBTITLE_SIZE_RANGE, default.size),
            background_opacity: clamp(self.background_opacity, 0.0..=1.0, 0.0),
            position: clamp(self.position, SUBTITLE_POSITION_RANGE, default.position),
            ..self.clone()
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScreenshotFormat {
    Png,
    Jpeg,
}

impl ScreenshotFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
        }
    }
}

/// How screenshots of HDR video are saved.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScreenshotHdr {
    /// Tone mapped, as an SDR screen shows it.
    Sdr,
    /// As it is, in a 16-bit PNG tagged BT.2020 PQ or HLG.
    Keep,
}

/// Where screenshots go and what they are like.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ScreenshotSettings {
    /// `None` for the Pictures folder.
    pub folder: Option<PathBuf>,
    pub format: ScreenshotFormat,
    /// JPEG quality, from 1 to 100.
    pub jpeg_quality: u8,
    /// With the subtitles showing drawn in.
    pub subtitles: bool,
    pub hdr: ScreenshotHdr,
}

pub const JPEG_QUALITY_RANGE: RangeInclusive<u8> = 50..=100;

impl Default for ScreenshotSettings {
    fn default() -> Self {
        Self {
            folder: None,
            format: ScreenshotFormat::Png,
            jpeg_quality: 90,
            subtitles: true,
            hdr: ScreenshotHdr::Sdr,
        }
    }
}

impl ScreenshotSettings {
    /// The folder screenshots go to: the one picked, else Pictures (or
    /// home, on a system without one).
    pub fn folder(&self) -> Option<PathBuf> {
        self.folder
            .clone()
            .or_else(dirs::picture_dir)
            .or_else(dirs::home_dir)
    }

    /// Whether HDR video is saved as HDR, which only PNG can be.
    pub fn keeps_hdr(&self) -> bool {
        self.hdr == ScreenshotHdr::Keep && self.format == ScreenshotFormat::Png
    }

    pub fn jpeg_quality(&self) -> u8 {
        self.jpeg_quality
            .clamp(*JPEG_QUALITY_RANGE.start(), *JPEG_QUALITY_RANGE.end())
    }
}

/// An audio output picked in Settings → Audio.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioDevice {
    /// What the system knows it by, from run to run.
    pub id: String,
    /// What it is called, to show while it isn't connected.
    pub name: String,
}

/// Which audio output everything plays through.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioOutputSettings {
    /// `None` for the system's default output, whichever it is.
    pub device: Option<AudioDevice>,
    /// While the device picked isn't connected, play through nothing until
    /// it is, rather than the system's default.
    pub wait_for_device: bool,
}

impl Default for AudioOutputSettings {
    fn default() -> Self {
        Self {
            device: None,
            wait_for_device: true,
        }
    }
}

/// Which protocols look for devices to cast to.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CastingSettings {
    /// Each protocol turned on or off, by its name ("Chromecast", "DLNA").
    /// One left out is on.
    pub look_with: BTreeMap<String, bool>,
}

impl CastingSettings {
    pub fn looks_with(&self, protocol: &str) -> bool {
        self.look_with.get(protocol).copied().unwrap_or(true)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub theme: ThemeMode,
    /// Volume applied on top of every player's own volume.
    pub master_volume: f32,
    pub audio_output: AudioOutputSettings,
    /// A window Output Device puts full screen on another display plays
    /// through that display's speakers while it is there, unless it plays
    /// through an output picked for it.
    pub sound_follows_display: bool,
    pub casting: CastingSettings,
    /// Volume new players start at.
    pub default_volume: f32,
    pub autoplay: bool,
    pub loop_by_default: bool,
    /// Start a file opened again where it was left, within a while
    /// (`progress::KEEP_DAYS`).
    pub resume_files: bool,
    pub seek_step_seconds: u32,
    /// Mark where a file's chapters start on its seek bar.
    pub chapter_marks: bool,
    pub video_fit: VideoFit,
    pub max_resolution: MaxResolution,
    /// Decode video on the GPU when the hardware supports it.
    pub hardware_decoding: bool,
    /// 0 picks a column count automatically.
    pub columns: u32,
    pub controls: ControlsVisibility,
    pub show_titles: bool,
    pub panels: PanelLook,
    pub float_on_top: FloatOnTop,
    pub full_screen: FullScreen,
    /// Cover the other displays in black while a window is full screen.
    pub black_out_displays: bool,
    /// An empty window left alone in front for `screensaver_minutes` fades
    /// its buttons out and bounces its logo off its edges.
    pub screensaver: bool,
    pub screensaver_minutes: u32,
    /// Gamma for SDR video, for every open video at once; 1 leaves it be.
    pub sdr_gamma: f32,
    /// Gamma for HDR video (Dolby Vision included).
    pub hdr_gamma: f32,
    pub subtitles: SubtitleStyle,
    pub screenshots: ScreenshotSettings,
    pub hotkeys: Hotkeys,
}

/// The gamma the settings offer.
pub const GAMMA_RANGE: RangeInclusive<f32> = 0.5..=2.0;

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: ThemeMode::System,
            master_volume: 1.0,
            audio_output: AudioOutputSettings::default(),
            sound_follows_display: true,
            casting: CastingSettings::default(),
            default_volume: 0.8,
            autoplay: true,
            loop_by_default: false,
            resume_files: true,
            // How far the arrow keys seek.
            seek_step_seconds: 5,
            chapter_marks: true,
            video_fit: VideoFit::Contain,
            max_resolution: MaxResolution::P1080,
            hardware_decoding: true,
            columns: 0,
            controls: ControlsVisibility::OnHover,
            show_titles: true,
            panels: PanelLook::Frosted,
            float_on_top: FloatOnTop::Off,
            full_screen: FullScreen::OwnSpace,
            black_out_displays: false,
            screensaver: true,
            screensaver_minutes: 5,
            sdr_gamma: 1.0,
            hdr_gamma: 1.0,
            subtitles: SubtitleStyle::default(),
            screenshots: ScreenshotSettings::default(),
            hotkeys: Hotkeys::default(),
        }
    }
}

impl Global for Settings {}

impl Settings {
    /// Change the settings, re-render every window and optionally write
    /// them to disk.
    pub fn update(cx: &mut App, save: bool, change: impl FnOnce(&mut Settings)) {
        cx.update_global::<Settings, _>(|settings, cx| {
            change(settings);
            if save {
                settings.save(cx);
            }
        });
        cx.refresh_windows();
    }

    /// Where the settings are saved: in the app's folder, so nowhere in
    /// tests and smoke runs, which must never read or write the real ones.
    pub fn path(cx: &App) -> Option<PathBuf> {
        Folder::of(cx).file(FILE)
    }

    /// The settings saved in `folder`, or the defaults.
    pub fn load(folder: &Folder) -> Self {
        folder
            .file(FILE)
            .map_or_else(Self::default, |path| Self::load_from(&path))
    }

    /// The settings in `path`, each value read on its own: one a setting
    /// can't take leaves just that setting as Plays starts it out. A file
    /// not read whole, or from a later Plays, whose settings this one would
    /// lose saving over it, is kept beside it as `settings.json.bad`.
    fn load_from(path: &Path) -> Self {
        let Ok(text) = fs::read_to_string(path) else {
            return Self::default();
        };
        let mut wrong = Vec::new();
        let settings = match serde_json::from_str(&text) {
            Ok(Value::Object(mut fields)) => {
                let version = fields.remove("version").and_then(|v| v.as_u64());
                if let Some(version) = version.filter(|version| *version > VERSION) {
                    wrong.push(format!("it's from a later Plays (version {version})"));
                }
                let (settings, unread) = leniently(fields);
                wrong.extend(unread.iter().map(|at| format!("{at} can't be read")));
                settings
            }
            Ok(_) => {
                wrong.push("it isn't settings".into());
                Self::default()
            }
            Err(err) => {
                wrong.push(err.to_string());
                Self::default()
            }
        };
        if !wrong.is_empty() {
            let aside = files::with_suffix(path, ".bad");
            log::warn!(
                "{}: {}; it's kept as {}",
                path.display(),
                wrong.join(", "),
                aside.display()
            );
            if let Err(err) = files::write_whole(&aside, text.as_bytes()) {
                log::warn!("could not keep {}: {err}", aside.display());
            }
        }
        settings
    }

    /// Save them in the app's folder.
    pub fn save(&self, cx: &App) {
        let Some(path) = Self::path(cx) else {
            return;
        };
        if let Err(err) = files::write_whole(&path, self.to_json().as_bytes()) {
            log::warn!("could not save settings to {}: {err}", path.display());
        }
    }

    /// The settings as their file holds them, this Plays' version first.
    pub fn to_json(&self) -> String {
        #[derive(Serialize)]
        struct File<'a> {
            version: u64,
            #[serde(flatten)]
            settings: &'a Settings,
        }
        let file = File {
            version: VERSION,
            settings: self,
        };
        serde_json::to_string_pretty(&file).unwrap_or_default()
    }

    /// Settings from a file someone picked, such as one exported: a JSON
    /// object with some of the settings in it, and the defaults for the
    /// rest. One with none of them in it is some other file.
    pub fn import(text: &str) -> anyhow::Result<Self> {
        let value: serde_json::Value = serde_json::from_str(text)?;
        let known = serde_json::to_value(Self::default())?;
        let (Some(fields), Some(known)) = (value.as_object(), known.as_object()) else {
            anyhow::bail!("It isn't a Plays settings file.");
        };
        if !fields.keys().any(|key| known.contains_key(key)) {
            anyhow::bail!("It isn't a Plays settings file.");
        }
        Ok(serde_json::from_value(value)?)
    }

    /// Gamma for SDR and HDR video, within `GAMMA_RANGE` whatever the file
    /// says.
    pub fn gamma(&self) -> [f32; 2] {
        [self.sdr_gamma, self.hdr_gamma].map(|gamma| {
            if gamma.is_finite() {
                gamma.clamp(*GAMMA_RANGE.start(), *GAMMA_RANGE.end())
            } else {
                1.0
            }
        })
    }

    /// How long an empty window rests before its screensaver starts: a
    /// minute at least.
    pub fn screensaver_after(&self) -> Duration {
        Duration::from_secs(u64::from(self.screensaver_minutes.max(1)) * 60)
    }

    pub fn columns_for(&self, count: usize) -> usize {
        match self.columns {
            0 => (count as f64).sqrt().ceil().max(1.0) as usize,
            n => n as usize,
        }
    }
}

/// `fields` laid over `T` as it starts out, a value at a time, each kept
/// only if `T` still reads with it; and where those it couldn't keep were.
fn leniently<T: Default + Serialize + DeserializeOwned>(
    fields: Map<String, Value>,
) -> (T, Vec<String>) {
    let mut read = serde_json::to_value(T::default()).unwrap_or_default();
    let mut unread = Vec::new();
    lay_over::<T>(&mut read, &mut Vec::new(), fields, &mut unread);
    (serde_json::from_value(read).unwrap_or_default(), unread)
}

/// Lay `fields` over the object at `path` in `root`, into objects there
/// too, putting back what was there wherever `T` can't read the whole with
/// the value laid over it.
fn lay_over<T: DeserializeOwned>(
    root: &mut Value,
    path: &mut Vec<String>,
    fields: Map<String, Value>,
    unread: &mut Vec<String>,
) {
    for (key, value) in fields {
        let Some(here) = object_at(root, path) else {
            return;
        };
        let into = matches!(here.get(&key), Some(Value::Object(_)));
        match value {
            Value::Object(inner) if into => {
                path.push(key);
                lay_over::<T>(root, path, inner, unread);
                path.pop();
            }
            value => {
                let before = here.insert(key.clone(), value);
                if serde_json::from_value::<T>(root.clone()).is_ok() {
                    continue;
                }
                if let Some(here) = object_at(root, path) {
                    match before {
                        Some(before) => here.insert(key.clone(), before),
                        None => here.remove(&key),
                    };
                }
                let mut at = path.join(".");
                if !at.is_empty() {
                    at.push('.');
                }
                unread.push(at + &key);
            }
        }
    }
}

fn object_at<'a>(root: &'a mut Value, path: &[String]) -> Option<&'a mut Map<String, Value>> {
    path.iter()
        .try_fold(root, |value, key| value.get_mut(key))?
        .as_object_mut()
}

#[cfg(test)]
mod tests {
    use gpui::TestAppContext;

    use super::*;

    /// A settings file of a test's own, holding `text`.
    fn file(name: &str, text: &str) -> PathBuf {
        let folder = std::env::temp_dir()
            .join("plays-tests")
            .join(format!("settings-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        let path = folder.join("settings.json");
        files::write_whole(&path, text.as_bytes()).unwrap();
        path
    }

    /// What was kept of a settings file read with something wrong in it.
    fn kept_aside(path: &Path) -> Option<String> {
        fs::read_to_string(files::with_suffix(path, ".bad")).ok()
    }

    #[test]
    fn a_bad_value_costs_only_itself() {
        let text = r#"{
            "master_volume": 0.5,
            "theme": "purple",
            "subtitles": { "color": "red", "size": 1.5 },
            "hotkeys": { "play_pause": 7, "loop": ["cmd-l"] }
        }"#;
        let path = file("bad-value", text);
        let settings = Settings::load_from(&path);
        let default = Settings::default();
        assert_eq!(settings.master_volume, 0.5);
        assert_eq!(settings.theme, default.theme);
        assert_eq!(settings.subtitles.color, default.subtitles.color);
        assert_eq!(settings.subtitles.size, 1.5);
        assert_eq!(
            settings.hotkeys.keys("playback.play_pause"),
            default.hotkeys.keys("playback.play_pause")
        );
        assert_eq!(settings.hotkeys.keys("playback.toggle_loop"), ["cmd-l"]);
        assert_eq!(kept_aside(&path).as_deref(), Some(text), "the file kept");
        let Ok(Value::Object(fields)) = serde_json::from_str(text) else {
            panic!("an object");
        };
        let (_, mut unread) = leniently::<Settings>(fields);
        unread.sort();
        assert_eq!(unread, ["hotkeys.play_pause", "subtitles.color", "theme"]);
    }

    #[test]
    fn a_file_half_written_is_kept_aside() {
        let text = r#"{ "master_volume": 0.5, "the"#;
        let path = file("half", text);
        assert_eq!(Settings::load_from(&path), Settings::default());
        assert_eq!(kept_aside(&path).as_deref(), Some(text));
    }

    #[test]
    fn a_file_from_a_later_plays_is_kept_aside() {
        let text = r#"{ "version": 99, "master_volume": 0.5 }"#;
        let path = file("later", text);
        assert_eq!(Settings::load_from(&path).master_volume, 0.5);
        assert_eq!(kept_aside(&path).as_deref(), Some(text));
    }

    #[test]
    fn a_good_file_is_read_as_it_is() {
        let settings = Settings {
            master_volume: 0.25,
            ..Settings::default()
        };
        let json = settings.to_json();
        assert!(
            json.starts_with(&format!("{{\n  \"version\": {VERSION},")),
            "{json}"
        );
        let path = file("good", &json);
        assert_eq!(Settings::load_from(&path), settings);
        assert_eq!(kept_aside(&path), None);
        // As are ones from before there was a version.
        let path = file("first", r#"{ "master_volume": 0.25 }"#);
        assert_eq!(Settings::load_from(&path), settings);
        assert_eq!(kept_aside(&path), None);
    }

    #[test]
    fn exported_settings_import_as_they_were() {
        let settings = Settings {
            master_volume: 0.25,
            theme: ThemeMode::Dark,
            ..Settings::default()
        };
        let imported = Settings::import(&settings.to_json()).unwrap();
        assert_eq!(imported, settings);
    }

    #[test]
    fn imports_take_the_defaults_for_what_they_leave_out() {
        let imported = Settings::import(r#"{ "master_volume": 0.5 }"#).unwrap();
        assert_eq!(imported.master_volume, 0.5);
        assert_eq!(imported.theme, Settings::default().theme);
    }

    #[test]
    fn other_files_dont_import() {
        assert!(Settings::import("not json").is_err());
        assert!(Settings::import("[1, 2]").is_err());
        assert!(Settings::import(r#"{ "name": "package", "version": "1.0" }"#).is_err());
    }

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        let settings: Settings =
            serde_json::from_str(r#"{ "master_volume": 0.5, "theme": "light" }"#).unwrap();
        assert_eq!(settings.master_volume, 0.5);
        assert_eq!(settings.theme, ThemeMode::Light);
        assert_eq!(settings.max_resolution, Settings::default().max_resolution);
    }

    #[test]
    fn the_screensaver_starts_after_five_minutes_and_a_minute_at_least() {
        let settings = Settings::default();
        assert!(settings.screensaver);
        assert_eq!(settings.screensaver_after(), Duration::from_secs(5 * 60));
        let read = |minutes: u32| Settings {
            screensaver_minutes: minutes,
            ..Settings::default()
        };
        assert_eq!(read(30).screensaver_after(), Duration::from_secs(30 * 60));
        assert_eq!(read(0).screensaver_after(), Duration::from_secs(60));
    }

    #[test]
    fn reads_the_audio_output() {
        let settings = Settings::default();
        assert_eq!(settings.audio_output.device, None);
        assert!(settings.audio_output.wait_for_device);
        let settings: Settings = serde_json::from_str(
            r#"{ "audio_output": { "device": { "id": "coreaudio:abc", "name": "DAC" } } }"#,
        )
        .unwrap();
        let device = settings.audio_output.device.as_ref().unwrap();
        assert_eq!(
            (device.id.as_str(), device.name.as_str()),
            ("coreaudio:abc", "DAC")
        );
        assert!(settings.audio_output.wait_for_device);
    }

    #[test]
    fn every_protocol_looks_but_those_turned_off() {
        let settings = Settings::default();
        assert!(settings.casting.looks_with("DLNA"));
        let settings: Settings =
            serde_json::from_str(r#"{ "casting": { "look_with": { "DLNA": false } } }"#).unwrap();
        assert!(!settings.casting.looks_with("DLNA"));
        assert!(settings.casting.looks_with("Chromecast"));
    }

    #[test]
    fn reads_and_writes_subtitle_styles() {
        let settings: Settings = serde_json::from_str(
            r##"{ "subtitles": { "color": "#FFD60A", "size": 9, "edge": "shadow" } }"##,
        )
        .unwrap();
        let style = settings.subtitles.clamped();
        assert_eq!(style.color, Rgb(0xffd60a));
        assert_eq!(style.size, 2.0);
        assert_eq!(style.edge, SubtitleEdge::Shadow);
        assert_eq!(style.weight, SubtitleWeight::Medium);
        let json = serde_json::to_string(&settings).unwrap();
        assert!(json.contains(r##""color":"#ffd60a""##), "{json}");
        assert!(
            serde_json::from_str::<Settings>(r#"{ "subtitles": { "color": "red" } }"#).is_err()
        );
    }

    #[test]
    fn reads_screenshot_settings() {
        let settings: Settings = serde_json::from_str(
            r#"{ "screenshots": { "folder": "/tmp/shots", "format": "jpeg", "jpeg_quality": 7 } }"#,
        )
        .unwrap();
        let shots = &settings.screenshots;
        assert_eq!(shots.folder(), Some(PathBuf::from("/tmp/shots")));
        assert_eq!(shots.format.extension(), "jpg");
        assert_eq!(shots.jpeg_quality(), 50);
        assert!(shots.subtitles);
        // HDR is kept only in PNGs.
        let keep = ScreenshotSettings {
            hdr: ScreenshotHdr::Keep,
            ..shots.clone()
        };
        assert!(!keep.keeps_hdr());
        assert!(
            ScreenshotSettings {
                format: ScreenshotFormat::Png,
                ..keep
            }
            .keeps_hdr()
        );
    }

    #[test]
    fn picks_columns() {
        let auto = Settings::default();
        assert_eq!(auto.columns_for(1), 1);
        assert_eq!(auto.columns_for(2), 2);
        assert_eq!(auto.columns_for(4), 2);
        assert_eq!(auto.columns_for(5), 3);
        let fixed = Settings {
            columns: 4,
            ..Settings::default()
        };
        assert_eq!(fixed.columns_for(2), 4);
    }

    #[gpui::test]
    fn tests_keep_no_settings_without_a_folder_of_their_own(cx: &mut TestAppContext) {
        assert_eq!(Folder::app(), Folder::default());
        assert_eq!(Settings::load(&Folder::app()), Settings::default());
        cx.update(|cx| assert_eq!(Settings::path(cx), None));
    }

    #[gpui::test]
    fn settings_changed_are_saved_in_the_apps_folder_and_read_back(cx: &mut TestAppContext) {
        let folder = Folder::for_test("settings-saved");
        let path = folder.file(FILE).unwrap();
        cx.update(|cx| {
            cx.set_global(folder.clone());
            cx.set_global(Settings::default());
            assert_eq!(Settings::path(cx).as_ref(), Some(&path));
            Settings::update(cx, false, |settings| settings.master_volume = 0.5);
            assert!(!path.exists(), "not saved unless asked");
            Settings::update(cx, true, |settings| settings.resume_files = false);
            let saved = Settings::load(&folder);
            assert_eq!(&saved, cx.global::<Settings>());
            assert_eq!((saved.master_volume, saved.resume_files), (0.5, false));
        });
    }

    #[gpui::test]
    fn a_broken_file_is_kept_aside_and_saved_over_whole(cx: &mut TestAppContext) {
        let folder = Folder::for_test("settings-broken");
        let path = folder.file(FILE).unwrap();
        let half = r#"{ "master_volume": 0.5, "the"#;
        files::write_whole(&path, half.as_bytes()).unwrap();
        let settings = Settings::load(&folder);
        assert_eq!(settings, Settings::default());
        cx.update(|cx| {
            cx.set_global(folder.clone());
            cx.set_global(settings);
            Settings::update(cx, true, |settings| settings.master_volume = 0.75);
        });
        assert_eq!(Settings::load(&folder).master_volume, 0.75);
        assert_eq!(kept_aside(&path).as_deref(), Some(half));
    }
}
