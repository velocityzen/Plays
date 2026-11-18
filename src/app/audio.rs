//! The audio output every player plays through.

use std::sync::Arc;

use gpui::{App, Global};

use crate::media::{AudioOutput, Mixer};

/// The single audio output shared by every player.
pub struct AudioEngine {
    output: Option<AudioOutput>,
    error: Option<String>,
}

impl Global for AudioEngine {}

impl AudioEngine {
    /// Open the system's audio output at `master_volume`. Without one, files
    /// play their video only.
    pub(super) fn start(master_volume: f32) -> Self {
        match AudioOutput::new(master_volume) {
            Ok(output) => Self {
                output: Some(output),
                error: None,
            },
            Err(err) => {
                log::error!("audio output unavailable, playing video only: {err:#}");
                Self {
                    output: None,
                    error: Some(err.to_string()),
                }
            }
        }
    }

    pub fn mixer(cx: &App) -> Option<Arc<Mixer>> {
        cx.try_global::<Self>()?
            .output
            .as_ref()
            .map(|o| o.mixer.clone())
    }

    pub fn device_description(cx: &App) -> String {
        match cx.try_global::<Self>() {
            Some(Self {
                output: Some(output),
                ..
            }) => format!(
                "{} · {} kHz",
                output.mixer.device_name,
                output.mixer.sample_rate as f32 / 1000.0
            ),
            Some(Self {
                error: Some(error), ..
            }) => format!("Unavailable: {error}"),
            _ => "Unavailable".into(),
        }
    }
}
