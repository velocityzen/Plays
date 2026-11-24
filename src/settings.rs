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

pub mod fields;

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
    /// Fill the block, cropping the edges.
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

/// Which panels show over the video, each turned off to keep the picture
/// clear of it. Notes on what's wrong show whatever these say.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct OverVideo {
    /// The seek bar and the buttons at the bottom.
    pub controls: bool,
    /// The file's name and the buttons at the top.
    pub top_bar: bool,
    /// The reel of tracks a shortcut rolls to the audio, video or
    /// subtitles picked.
    pub track_switcher: bool,
    /// What a shortcut changed, like the volume.
    pub notes: bool,
}

impl Default for OverVideo {
    fn default() -> Self {
        Self {
            controls: true,
            top_bar: true,
            track_switcher: true,
            notes: true,
        }
    }
}

/// One of the panels over the video, picked from those `OverVideo` has.
pub type Panel = fn(&mut OverVideo) -> &mut bool;

impl OverVideo {
    /// None shown.
    pub const NONE: Self = Self {
        controls: false,
        top_bar: false,
        track_switcher: false,
        notes: false,
    };

    /// The panels shown, by their keys here.
    pub fn visible(self) -> Vec<&'static str> {
        [
            ("controls", self.controls),
            ("top_bar", self.top_bar),
            ("track_switcher", self.track_switcher),
            ("notes", self.notes),
        ]
        .into_iter()
        .filter_map(|(panel, shown)| shown.then_some(panel))
        .collect()
    }

    /// With `panel` turned on, or off.
    #[must_use]
    pub fn toggled(mut self, panel: Panel) -> Self {
        let shown = panel(&mut self);
        *shown = !*shown;
        self
    }

    /// Each panel hidden while any shows, else each shown.
    #[must_use]
    pub fn all_toggled(self) -> Self {
        if self == Self::NONE {
            Self::default()
        } else {
            Self::NONE
        }
    }

    /// As it is, but for the panels `old` and `new` tell apart, which are
    /// as `new` has them: what a window or block shows as those it's in
    /// change.
    #[must_use]
    pub fn follow(self, old: Self, new: Self) -> Self {
        let pick = |own, old, new| if old == new { own } else { new };
        Self {
            controls: pick(self.controls, old.controls, new.controls),
            top_bar: pick(self.top_bar, old.top_bar, new.top_bar),
            track_switcher: pick(self.track_switcher, old.track_switcher, new.track_switcher),
            notes: pick(self.notes, old.notes, new.notes),
        }
    }
}

/// How an audio file's block looks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MusicMode {
    /// What plays behind its cover.
    pub visualizer: Visualizer,
    /// Its codec, bits, sample rate, channels and bit rate, under its tags.
    pub show_quality: bool,
}

impl Default for MusicMode {
    fn default() -> Self {
        Self {
            visualizer: Visualizer::Speaker,
            show_quality: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Visualizer {
    None,
    /// A speaker whose cone moves with the level.
    Speaker,
}

/// The languages to play a file's audio and show its subtitles in, first
/// first, each by its code (`tracks::language`): a file opens with the
/// first it has. None leaves it to the file.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Languages {
    pub audio: Vec<String>,
    pub subtitles: Vec<String>,
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

/// What several files opened at once, or a folder's, do.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SeveralFiles {
    /// Each opens in a block of its own, all playing.
    #[default]
    Together,
    /// They open in one block, which plays them one after another.
    Playlist,
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
    /// The whole block, so text can go in the bars around a letterboxed picture.
    Block,
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
    pub music_mode: MusicMode,
    pub casting: CastingSettings,
    /// Volume new players start at.
    pub default_volume: f32,
    pub autoplay: bool,
    /// Several files opened at once play together or one after another.
    pub several_files: SeveralFiles,
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
    pub languages: Languages,
    /// 0 picks a column count automatically.
    pub columns: u32,
    pub controls: ControlsVisibility,
    pub show_titles: bool,
    pub panels: PanelLook,
    pub over_video: OverVideo,
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
    /// Let a command line, `plays` and scripts, run commands and hear what
    /// changes, over the socket. Off until turned on in Settings: a command
    /// line can't turn it on, as it's refused.
    pub command_line_control: bool,
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
            music_mode: MusicMode::default(),
            casting: CastingSettings::default(),
            default_volume: 0.8,
            autoplay: true,
            several_files: SeveralFiles::Together,
            loop_by_default: false,
            resume_files: true,
            // How far the arrow keys seek.
            seek_step_seconds: 5,
            chapter_marks: true,
            video_fit: VideoFit::Contain,
            max_resolution: MaxResolution::P1080,
            hardware_decoding: true,
            languages: Languages::default(),
            columns: 0,
            controls: ControlsVisibility::OnHover,
            show_titles: true,
            panels: PanelLook::Frosted,
            over_video: OverVideo::default(),
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
            command_line_control: false,
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
    /// rest. Read as they're loaded, each value on its own: one this Plays
    /// can't take, as from a later one, leaves just that setting as it
    /// starts out. One with none of them in it is some other file.
    pub fn import(text: &str) -> anyhow::Result<Self> {
        let value: serde_json::Value = serde_json::from_str(text)?;
        let known = serde_json::to_value(Self::default())?;
        let (Value::Object(mut fields), Some(known)) = (value, known.as_object()) else {
            anyhow::bail!("It isn't a Plays settings file.");
        };
        if !fields.keys().any(|key| known.contains_key(key)) {
            anyhow::bail!("It isn't a Plays settings file.");
        }
        fields.remove("version");
        let (settings, unread) = leniently(fields);
        if !unread.is_empty() {
            log::warn!("importing settings: {} can't be read", unread.join(", "));
        }
        Ok(settings)
    }

    /// Volume on top of every player's own, full at most whatever the file
    /// says.
    pub fn master_volume(&self) -> f32 {
        self.master_volume.clamp(0.0, 1.0)
    }

    /// Volume new players start at, full at most whatever the file says.
    pub fn default_volume(&self) -> f32 {
        self.default_volume.clamp(0.0, 1.0)
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
/// the value laid over it (`reads_whole`).
fn lay_over<T: Serialize + DeserializeOwned>(
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
                if reads_whole::<T>(root) {
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

/// Whether `T` reads `value`, and reads again what it writes of it: a
/// number too big for its setting reads as infinity, which JSON can't hold.
fn reads_whole<T: Serialize + DeserializeOwned>(value: &Value) -> bool {
    serde_json::from_value::<T>(value.clone())
        .ok()
        .and_then(|read| serde_json::to_value(read).ok())
        .is_some_and(|written| serde_json::from_value::<T>(written).is_ok())
}

fn object_at<'a>(root: &'a mut Value, path: &[String]) -> Option<&'a mut Map<String, Value>> {
    path.iter()
        .try_fold(root, |value, key| value.get_mut(key))?
        .as_object_mut()
}

#[cfg(test)]
#[path = "tests/settings.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/settings_properties.rs"]
mod properties;
