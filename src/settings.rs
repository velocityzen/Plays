use std::fs;
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
}

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
