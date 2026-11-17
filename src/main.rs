//! Plays: a small video and audio player that plays many files at once.

mod app;
mod assets;
mod media;
mod settings;
mod ui;

use std::path::PathBuf;

use ffmpeg_next as ffmpeg;
use gpui::Application;

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("plays=info"))
        .init();

    ffmpeg::init().expect("initializing FFmpeg");
    ffmpeg::util::log::set_level(ffmpeg::util::log::Level::Error);

    let files: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();

    Application::new()
        .with_assets(assets::Assets)
        .run(move |cx| app::run(cx, files));
}
