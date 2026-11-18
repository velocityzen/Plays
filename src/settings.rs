use std::fs;
use std::ops::RangeInclusive;
use std::path::PathBuf;

use gpui::Global;
use serde::{Deserialize, Serialize};

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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub theme: ThemeMode,
    /// Volume applied on top of every player's own volume.
    pub master_volume: f32,
    /// Volume new players start at.
    pub default_volume: f32,
    pub autoplay: bool,
    pub loop_by_default: bool,
    pub seek_step_seconds: u32,
    pub video_fit: VideoFit,
    pub max_resolution: MaxResolution,
    /// Decode video on the GPU when the hardware supports it.
    pub hardware_decoding: bool,
    /// 0 picks a column count automatically.
    pub columns: u32,
    pub controls: ControlsVisibility,
    pub show_titles: bool,
    /// Gamma for SDR video, for every open video at once; 1 leaves it be.
    pub sdr_gamma: f32,
    /// Gamma for HDR video (Dolby Vision included).
    pub hdr_gamma: f32,
    pub subtitles: SubtitleStyle,
}

/// The gamma the settings offer.
pub const GAMMA_RANGE: RangeInclusive<f32> = 0.5..=2.0;

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: ThemeMode::System,
            master_volume: 1.0,
            default_volume: 0.8,
            autoplay: true,
            loop_by_default: false,
            seek_step_seconds: 10,
            video_fit: VideoFit::Contain,
            max_resolution: MaxResolution::P1080,
            hardware_decoding: true,
            columns: 0,
            controls: ControlsVisibility::OnHover,
            show_titles: true,
            sdr_gamma: 1.0,
            hdr_gamma: 1.0,
            subtitles: SubtitleStyle::default(),
        }
    }
}

impl Global for Settings {}

impl Settings {
    pub fn path() -> Option<PathBuf> {
        dirs::config_dir().map(|dir| dir.join("plays").join("settings.json"))
    }

    pub fn load() -> Self {
        let Some(path) = Self::path() else {
            return Self::default();
        };
        match fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|err| {
                log::warn!("ignoring invalid settings in {}: {err}", path.display());
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) {
        let Some(path) = Self::path() else {
            return;
        };
        let result = path
            .parent()
            .map_or(Ok(()), fs::create_dir_all)
            .and_then(|()| {
                fs::write(
                    &path,
                    serde_json::to_string_pretty(self).unwrap_or_default(),
                )
            });
        if let Err(err) = result {
            log::warn!("could not save settings to {}: {err}", path.display());
        }
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

    pub fn columns_for(&self, count: usize) -> usize {
        match self.columns {
            0 => (count as f64).sqrt().ceil().max(1.0) as usize,
            n => n as usize,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        let settings: Settings =
            serde_json::from_str(r#"{ "master_volume": 0.5, "theme": "light" }"#).unwrap();
        assert_eq!(settings.master_volume, 0.5);
        assert_eq!(settings.theme, ThemeMode::Light);
        assert_eq!(settings.max_resolution, Settings::default().max_resolution);
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
}
