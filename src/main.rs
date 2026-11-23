//! Plays: a small video and audio player that plays many files at once.

mod api;
mod app;
mod assets;
mod cast;
mod cli;
mod files;
mod hotkeys;
mod media;
mod progress;
mod recents;
mod settings;
mod ui;

use std::ffi::OsString;
use std::path::PathBuf;

use ffmpeg_next as ffmpeg;
use gpui::Application;

use crate::media::Source;

fn main() {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    #[cfg(feature = "smoke")]
    let (smoke, args) = app::smoke::from_args(args);
    // The command line asks the Plays running, without a UI of its own.
    if cli::wanted(&args) {
        std::process::exit(cli::main(args).into());
    }
    // Logs the same, keeping the errors for the smoke run to report.
    #[cfg(feature = "smoke")]
    app::smoke::init_logging();
    #[cfg(not(feature = "smoke"))]
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("plays=info"))
        .init();

    // One Plays runs: another, started with files, hands them to it and
    // goes, before setting anything up.
    let listener = match api::socket::start(&args) {
        api::socket::Start::Runs(listener) => listener,
        api::socket::Start::HandedOver(status) => std::process::exit(status.into()),
    };

    ffmpeg::init().expect("initializing FFmpeg");
    ffmpeg::util::log::set_level(ffmpeg::util::log::Level::Error);
    // Sets up TLS libraries before any thread opens an https:// stream.
    // SAFETY: called once, before any other FFmpeg use on other threads.
    unsafe {
        ffmpeg::ffi::avformat_network_init();
    }

    // Files, folders and stream URLs can be given on the command line.
    let sources: Vec<Source> = args
        .into_iter()
        .map(|arg| Source::from_arg(PathBuf::from(arg)))
        .collect();

    let settings = settings::Settings::load(&files::Folder::app());
    let app = Application::new().with_assets(assets::Assets);
    // Files opened from Finder or dropped on the Dock icon. The first ones
    // arrive before the app has finished launching, so they wait here.
    let (opened, opened_rx) = futures::channel::mpsc::unbounded();
    app.on_open_urls(move |urls| {
        opened.unbounded_send(urls).ok();
    });
    app.run(move |cx| {
        app::run(cx, settings, sources, opened_rx, listener);
        #[cfg(feature = "smoke")]
        if let Some(scenario) = smoke {
            app::smoke::start(scenario, cx);
        }
    });
}
